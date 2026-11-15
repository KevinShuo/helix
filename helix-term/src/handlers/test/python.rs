use crate::handlers::ai_test::runner;
use anyhow::{ensure, Context, Result};
use helix_view::{
    ai_test::RunStage,
    test::{Outcome, RunRequest, Scope, TestCase},
};
use serde_json::{json, Value};
use std::{path::Path, time::Duration};

pub(super) fn arguments(request: &RunRequest) -> Vec<String> {
    match &request.scope {
        Scope::Project => vec![],
        Scope::File(path) => vec![path.to_string_lossy().into_owned()],
        Scope::Cases(ids) => ids.clone(),
        Scope::Declaration {
            path,
            declaration: d,
        } => {
            let mut id = path.to_string_lossy().into_owned();
            if let Some(suite) = &d.suite {
                id.push_str("::");
                id.push_str(suite);
            }
            id.push_str("::");
            id.push_str(&d.name);
            vec![id]
        }
    }
}
async fn consume(id: u64, report: &Path, offset: &mut usize) -> Result<()> {
    let text = match tokio::fs::read_to_string(report).await {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    ensure!(text.len() <= 32_000_000, "pytest 报告超过 32MB");
    for line in text[*offset..].split_inclusive('\n') {
        if !line.ends_with('\n') {
            break;
        }
        *offset += line.len();
        let value: Value = serde_json::from_str(line).context("pytest 结构化报告无效")?;
        match value["event"].as_str() {
            Some("collected" | "result") => {
                let mut case: TestCase = serde_json::from_value(value["case"].clone())?;
                case.path = super::normalized(case.path);
                case.failure_path = case.failure_path.map(super::normalized);
                super::publish(id, case).await;
            }
            Some("started") => {
                let case_id = value["id"].as_str().unwrap_or_default().to_owned();
                super::update(id, move |s| {
                    if let Some(c) = s.cases.iter_mut().find(|c| c.id == case_id) {
                        c.outcome = Outcome::Running;
                    }
                })
                .await;
            }
            _ => {}
        }
    }
    Ok(())
}
pub(super) async fn run(id: u64, request: &RunRequest) -> Result<()> {
    super::phase(id, "收集并运行 pytest 测试").await;
    // TempDir is owned by this cancellable future and cleaned up on cancellation.
    let temp = tempfile::tempdir()?;
    let input = temp.path().join("request.json");
    let report = temp.path().join("report.jsonl");
    tokio::fs::write(
        &input,
        serde_json::to_vec(&json!({"args": arguments(request)}))?,
    )
    .await?;
    let args = vec![
        "-c".into(),
        include_str!("pytest_bridge.py").into(),
        input.to_string_lossy().into_owned(),
        report.to_string_lossy().into_owned(),
    ];
    let python = runner::python(&request.root, &request.config);
    let process = runner::command(
        &python,
        &args,
        &request.root,
        RunStage::Test,
        request.config.run_timeout_ms,
    );
    tokio::pin!(process);
    let mut interval = tokio::time::interval(Duration::from_millis(100));
    let mut offset = 0;
    let result = loop {
        tokio::select! {
            result = &mut process => break result,
            _ = interval.tick() => consume(id, &report, &mut offset).await?,
        }
    };
    consume(id, &report, &mut offset).await?;
    // Avoid logging the embedded Python source supplied in argv.
    let log = result
        .log
        .split_once("\n")
        .map(|(_, log)| log)
        .unwrap_or(&result.log)
        .to_owned();
    super::log(id, log.clone()).await;
    ensure!(
        report.is_file(),
        "无法运行 pytest；请检查所选解释器是否安装 pytest\n{log}"
    );
    // Exit 1 is an ordinary failed test; other exits represent collection/environment errors.
    let text = tokio::fs::read_to_string(&report).await?;
    let finished = text
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .find(|v| v["event"] == "finished");
    let code = finished
        .as_ref()
        .and_then(|v| v["code"].as_i64())
        .context("pytest 中断或超时，报告未完成")?;
    ensure!(matches!(code, 0 | 1), "pytest 收集/运行失败，退出码 {code}");
    Ok(())
}
