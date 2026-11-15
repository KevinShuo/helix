use super::*;
use helix_view::ai_merge::{AiMergeConfig, Target};
use serde_json::json;

fn target() -> Target {
    Target {
        block: 2,
        path: "src/计算.rs".into(),
        language: "rust".into(),
        base: "    base();\n".into(),
        current: "    current();\n".into(),
        incoming: "    incoming();\n".into(),
        result: "    手动修改();\n".into(),
        context: vec!["use example::Thing;\nfn merge() {".into()],
        intent: "保留双方校验".into(),
        range: 10..22,
    }
}

fn response(status: &str, code: Value) -> Value {
    json!({"choices":[{"finish_reason":"stop","message":{"content":json!({
        "block_id":2,"status":status,"code":code,"explanation":"保留双方的行为。"
    }).to_string()}}]})
}

#[test]
fn ai_merge_request_keeps_complete_required_inputs_and_intent() {
    let target = target();
    let model = AiProviderConfig {
        context_size: 32768,
        ..Default::default()
    };
    let config = AiMergeConfig {
        enabled: true,
        ..Default::default()
    };
    let body = request(&model, &config, &target).unwrap();
    let data: Value =
        serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
    assert_eq!(data["block_id"], 2);
    assert_eq!(data["base"], "    base();\n");
    assert_eq!(data["current"], "    current();\n");
    assert_eq!(data["incoming"], "    incoming();\n");
    assert_eq!(data["draft"], "    手动修改();\n");
    assert_eq!(data["intent"], "保留双方校验");
    assert_eq!(data["draft_is_base"], false);
    assert!(data["context"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s.as_str().unwrap().contains("use example::Thing")));
    assert_eq!(body["stream"], false);
    assert_eq!(body["max_tokens"], 1536);
}

#[test]
fn ai_merge_identifies_base_seed_as_draft_instead_of_user_selection() {
    let mut target = target();
    target.result = target.base.clone();
    let model = AiProviderConfig {
        context_size: 8192,
        ..Default::default()
    };
    let config = AiMergeConfig {
        enabled: true,
        ..Default::default()
    };
    let body = request(&model, &config, &target).unwrap();
    let data: Value =
        serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
    assert_eq!(data["draft_is_base"], true);
    assert_eq!(data["current"], "    current();\n");
    assert_eq!(data["incoming"], "    incoming();\n");
}

#[test]
fn ai_merge_whole_block_delete_against_edit_requires_user_intent() {
    let mut target = target();
    target.intent.clear();
    target.base = "legacy();\n".into();
    target.current.clear();
    target.incoming = "safe_legacy();\n".into();
    assert!(requires_manual_choice(&target));
    target.intent = "保留删除".into();
    assert!(!requires_manual_choice(&target));
    target.intent.clear();
    target.incoming = target.base.clone();
    assert!(!requires_manual_choice(&target));
    target.incoming.clear();
    assert!(!requires_manual_choice(&target));
    target.current = "safe_legacy();\n".into();
    assert!(requires_manual_choice(&target));
    target.base.clear();
    assert!(!requires_manual_choice(&target));
}

#[test]
fn ai_merge_budget_drops_optional_context_but_never_truncates_a_conflict() {
    let mut target = target();
    target.context = vec!["optional".repeat(5000)];
    let model = AiProviderConfig {
        context_size: 8192,
        ..Default::default()
    };
    let config = AiMergeConfig {
        enabled: true,
        ..Default::default()
    };
    let body = request(&model, &config, &target).unwrap();
    let data: Value =
        serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
    assert!(data["context"].as_array().unwrap().is_empty());
    assert_eq!(data["draft"], "    手动修改();\n");
    target.base = "required".repeat(3000);
    assert!(request(&model, &config, &target).is_err());
    target.base.clear();
    let small = AiMergeConfig {
        max_input_bytes: 10,
        ..config.clone()
    };
    assert!(request(&model, &small, &target).is_err());
    let disabled = AiMergeConfig {
        enabled: false,
        ..config
    };
    assert!(request(&model, &disabled, &target).is_err());
}

#[test]
fn ai_merge_response_allows_empty_deletion_and_normalizes_crlf() {
    assert_eq!(
        parse(&response("resolved", json!("")), 2)
            .unwrap()
            .code
            .as_deref(),
        Some("")
    );
    assert_eq!(
        parse(&response("resolved", json!("双方();\r\n")), 2)
            .unwrap()
            .code
            .as_deref(),
        Some("双方();\n")
    );
    let manual = parse(&response("needs-manual", Value::Null), 2).unwrap();
    assert!(manual.code.is_none());
    assert!(!manual.explanation.is_empty());
}

#[test]
fn ai_merge_response_rejects_wrong_target_incomplete_and_tool_output() {
    let valid = response("resolved", json!("merged();\n"));
    assert!(parse(&valid, 1).is_err());
    let mut truncated = valid.clone();
    truncated["choices"][0]["finish_reason"] = json!("length");
    assert!(parse(&truncated, 2).is_err());
    let mut tools = valid.clone();
    tools["choices"][0]["message"]["tool_calls"] =
        json!([{"type":"function","function":{"name":"write_file","arguments":"{}"}}]);
    assert!(parse(&tools, 2).is_err());
    let mut unknown = valid;
    let mut content: Value = serde_json::from_str(
        unknown["choices"][0]["message"]["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    content["path"] = json!("other.rs");
    unknown["choices"][0]["message"]["content"] = json!(content.to_string());
    assert!(parse(&unknown, 2).is_err());
    assert!(parse(&response("resolved", Value::Null), 2).is_err());
    assert!(parse(&response("needs-manual", json!("code")), 2).is_err());
    assert!(parse(&response("maybe", json!("code")), 2).is_err());
}

#[test]
fn ai_merge_response_rejects_markers_controls_and_excessive_code() {
    for code in [
        "<<<<<<< current\nx\n=======\ny\n>>>>>>> incoming\n",
        "x\u{1b}[2J",
        "x\0",
        "x\r",
        "x\u{202e}y",
    ] {
        assert!(
            parse(&response("resolved", json!(code)), 2).is_err(),
            "{code:?}"
        );
    }
    assert!(parse(&response("resolved", json!("x".repeat(65537))), 2).is_err());
}

#[test]
fn ai_merge_thinking_reserves_reasoning_and_final_json_in_the_context() {
    let mut model = AiProviderConfig {
        kind: helix_view::ai::AiProviderType::LlamaCpp,
        context_size: 8192,
        thinking: true,
        thinking_budget: Some(2048),
        ..Default::default()
    };
    let config = AiMergeConfig {
        enabled: true,
        ..Default::default()
    };
    assert_eq!(
        request(&model, &config, &target()).unwrap()["max_tokens"],
        3584
    );
    model.context_size = 4096;
    assert!(request(&model, &config, &target()).is_err());
}

#[tokio::test]
async fn ai_merge_managed_27b_restores_completion_9b_after_success_and_invalid_response() {
    use crate::handlers::ai_completion::server::{
        lease_tests::{models, reachable, ready},
        LocalServers,
    };
    let _messages = helix_event::status::setup();
    let dir = tempfile::tempdir().unwrap();
    let (baseline, temporary) = models(dir.path(), 0);
    let mut pool = LocalServers::new(&[baseline.clone(), temporary.clone()]);
    ready(&baseline).await;
    let config = AiMergeConfig {
        enabled: true,
        max_tokens: 128,
        ..Default::default()
    };
    let model_path = temporary.model_path.as_ref().unwrap();
    for valid in [true, false] {
        let reply = if valid {
            response("resolved", json!("merged();\n"))
        } else {
            json!({})
        };
        std::fs::write(
            model_path.with_extension("response.json"),
            reply.to_string(),
        )
        .unwrap();
        let result = generate(&temporary, &config, &target()).await;
        if valid {
            assert_eq!(result.unwrap().code.as_deref(), Some("merged();\n"));
        } else {
            assert!(result.is_err());
        }
        assert!(
            model_path.with_extension("requested").exists(),
            "Merge must use the managed 27B server"
        );
        assert!(!reachable(&temporary), "27B must unload after the request");
        ready(&baseline).await;
    }
    let starts = std::fs::read_to_string(
        baseline
            .model_path
            .as_ref()
            .unwrap()
            .with_extension("started"),
    )
    .unwrap();
    assert_eq!(
        starts.lines().count(),
        3,
        "9B must restart after each Merge request"
    );
    pool.stop().await;
}

#[tokio::test]
async fn ai_merge_cancelled_27b_inference_restores_completion_9b() {
    use crate::handlers::ai_completion::server::{
        lease_tests::{models, reachable, ready},
        LocalServers,
    };
    let _messages = helix_event::status::setup();
    let dir = tempfile::tempdir().unwrap();
    let (baseline, temporary) = models(dir.path(), 0);
    let model_path = temporary.model_path.as_ref().unwrap();
    use std::io::Write;
    writeln!(
        std::fs::OpenOptions::new()
            .append(true)
            .open(model_path)
            .unwrap(),
        "response-delay=3000"
    )
    .unwrap();
    std::fs::write(
        model_path.with_extension("response.json"),
        response("resolved", json!("late();\n")).to_string(),
    )
    .unwrap();
    let mut pool = LocalServers::new(&[baseline.clone(), temporary.clone()]);
    ready(&baseline).await;
    let model = temporary.clone();
    let task = tokio::spawn(async move {
        generate(
            &model,
            &AiMergeConfig {
                enabled: true,
                max_tokens: 128,
                ..Default::default()
            },
            &target(),
        )
        .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while !model_path.with_extension("requested").exists() {
            assert!(
                !task.is_finished(),
                "Merge must start the managed 27B request"
            );
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(!reachable(&baseline), "9B must unload while Merge uses 27B");
    task.abort();
    let _ = task.await;
    ready(&baseline).await;
    assert!(!reachable(&temporary));
    pool.stop().await;
}

#[tokio::test]
async fn ai_merge_local_delete_modify_refusal_does_not_acquire_a_model_lease() {
    let model = AiProviderConfig {
        on_demand: true,
        context_size: 8192,
        ..Default::default()
    };
    let config = AiMergeConfig {
        enabled: true,
        ..Default::default()
    };
    let mut target = target();
    target.current.clear();
    target.intent.clear();
    assert!(generate(&model, &config, &target)
        .await
        .unwrap()
        .code
        .is_none());
}
