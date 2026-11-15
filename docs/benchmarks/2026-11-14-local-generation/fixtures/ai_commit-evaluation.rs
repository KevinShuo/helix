use super::*;
use std::time::Instant;
mod support {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/quality_eval_support.rs"
    ));
}

fn file(id: usize, path: &str, diff: &str, staged: bool) -> AiFileInput {
    AiFileInput {
        id,
        path: path.into(),
        diff: diff.into(),
        staged,
        partial: false,
        eligible: true,
    }
}

#[tokio::test]
#[ignore = "controlled local-model quality evaluation; requires HELIX_AI_EVAL_* environment"]
async fn ai_commit_quality_evaluation() {
    let model = support::model();
    let config = AiCommitConfig {
        enabled: true,
        max_tokens: 1024,
        timeout_ms: 120000,
        ..Default::default()
    };
    let fix = "@@ -1,2 +1,4 @@\n def mean(values):\n+    if not values:\n+        return 0.0\n     return sum(values) / len(values)\n";
    let refactor = "@@ -1,6 +1,9 @@\n+def normalize(value):\n+    return value.strip().lower()\n+\n def save_name(name):\n-    key = name.strip().lower()\n+    key = normalize(name)\n     return key\n def find_name(name):\n-    return name.strip().lower()\n+    return normalize(name)\n";
    let mut partial = file(
        2,
        "core/stats.py",
        "@@ -1 +1 @@\n-old = 1\n+new = 2\n",
        false,
    );
    partial.partial = true;
    let mut sensitive = file(
        3,
        ".env.production",
        "API_SECRET=synthetic-evaluation-only\n",
        false,
    );
    sensitive.eligible = false;
    let selections = [
        ("commit_select_fix", "只提交修复mean空输入除零的问题，README改字留待后续", vec![file(1,"core/stats.py",fix,false),file(2,"README.md","@@ -1 +1 @@\n-Hello\n+Welcome\n",false)]),
        ("commit_select_tests", "实现已在暂存区，本次只选择parse_port边界测试，不选择纯格式调整", vec![file(1,"tests/test_port.py","@@ -0,0 +1,4 @@\n+def test_port_boundaries():\n+    assert parse_port('1') == 1\n+    assert parse_port('65535') == 65535\n+    with pytest.raises(ValueError): parse_port('0')\n",false),file(2,"unrelated.py","@@ -1 +1 @@\n-x=1\n+x = 1\n",false),file(3,"core/port.py","@@ -1 +1,2 @@\n def parse_port(text):\n+    if not 1 <= int(text) <= 65535: raise ValueError()\n",true)]),
        ("commit_select_partial", "只提交mean空输入修复，部分暂存文件及环境配置必须保留原状", vec![file(1,"core/stats.py",fix,false),partial,sensitive]),
    ];
    for (id, intent, files) in selections {
        let started = Instant::now();
        let result = recommend(&model, &config, &files, intent)
            .await
            .map(|r| json!({"selected":r.selected,"reasons":r.reasons,"incomplete":r.incomplete}));
        support::record(
            &model,
            "ai-commit",
            id,
            json!({"operation":"selection","intent":intent,"expected_selected":[1],"files":files.iter().map(|f|json!({"id":f.id,"path":f.path,"diff":f.diff,"staged":f.staged,"partial":f.partial,"eligible":f.eligible})).collect::<Vec<_>>()}),
            result,
            started.elapsed(),
        );
    }
    for (id,intent,kind,core,files) in [
        ("commit_message_fix", "说明修复mean对空输入除零，未运行测试", "fix", "空输入返回零，避免除零", vec![file(1,"core/stats.py",fix,true)]),
        ("commit_message_feat", "新增可配置请求超时，不声称运行测试", "feat", "调用方可传入timeout，默认10秒并传给底层请求", vec![file(1,"client.py","@@ -1,2 +1,2 @@\n-def fetch(url):\n-    return session.get(url)\n+def fetch(url, timeout=10):\n+    return session.get(url, timeout=timeout)\n",true)]),
        ("commit_message_refactor", "仅提取重复的名称规范化逻辑，不改变行为，未运行测试", "refactor", "提取strip加lower的公共函数并在两处复用，行为保持不变", vec![file(1,"names.py",refactor,true)]),
    ] {
        let started=Instant::now();
        let result=generate_message(&model,&config,&files,intent).await.map(|r| json!({"text":r.text,"incomplete":r.incomplete}));
        support::record(&model,"ai-commit",id,json!({"operation":"message","intent":intent,"expected_type":kind,"core":core,"files":files.iter().map(|f|json!({"id":f.id,"path":f.path,"diff":f.diff,"staged":f.staged})).collect::<Vec<_>>()}),result,started.elapsed());
    }
}
