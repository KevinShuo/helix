use helix_view::ai::{AiProviderConfig, AiProviderType, AiProxy};
use serde_json::{json, Value};
use std::{fs::OpenOptions, io::Write, time::Duration};

pub(super) fn model() -> AiProviderConfig {
    if let Some(path) = std::env::var_os("HELIX_AI_EVAL_PROVIDER_CONFIG") {
        let bytes = std::fs::read(path).expect("evaluation provider file");
        let provider: AiProviderConfig =
            serde_json::from_slice(&bytes).expect("evaluation provider");
        provider.validate().expect("valid evaluation provider");
        return provider;
    }
    let thinking = std::env::var("HELIX_AI_EVAL_THINKING").as_deref() == Ok("true");
    AiProviderConfig {
        kind: AiProviderType::LlamaCpp,
        endpoint: std::env::var("HELIX_AI_EVAL_ENDPOINT").expect("evaluation endpoint"),
        model: std::env::var("HELIX_AI_EVAL_MODEL").expect("evaluation model alias"),
        context_size: 16384,
        thinking,
        thinking_budget: thinking.then_some(2048),
        reasoning_effort: thinking.then(|| "medium".into()),
        proxy: AiProxy::None,
        ..Default::default()
    }
}

pub(super) fn record(
    model: &AiProviderConfig,
    feature: &str,
    id: &str,
    input: Value,
    result: anyhow::Result<Value>,
    elapsed: Duration,
) {
    let row = match result {
        Ok(value) => json!({"result":value,"error":null}),
        Err(error) => json!({"result":null,"error":format!("{error:#}")}),
    };
    let row = json!({"feature":feature,"id":id,"input":input,"model":model.model,"thinking":model.thinking,"thinking_budget":model.thinking_budget,"reasoning_effort":model.reasoning_effort,"elapsed_ms":elapsed.as_millis(),"result":row["result"],"error":row["error"]});
    let output = std::env::var("HELIX_AI_EVAL_OUTPUT").expect("evaluation output path");
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(output)
        .unwrap();
    writeln!(file, "{row}").unwrap();
    file.flush().unwrap();
    println!(
        "EVAL {feature}/{id}: {} ({} ms)",
        if row["error"].is_null() {
            "ok"
        } else {
            "error"
        },
        elapsed.as_millis()
    );
}

pub(super) fn selected_case(id: &str) -> bool {
    std::env::var("HELIX_AI_EVAL_CASE").map_or(true, |selected| selected == id)
}
