use helix_view::editor::Config;

#[test]
fn ai_completion_configuration_keeps_regular_completion_enabled() {
    let config: Config = toml::from_str(
        r#"
[ai-completion]
enabled = true
endpoint = "http://127.0.0.1:19239/v1/chat/completions"
model = "Qwen3.8-27B-UD-Q5_K_M"
api-key-env = "HELIX_AI_API_KEY"
debounce-ms = 350
timeout-ms = 5000
max-tokens = 128
prefix-chars = 8000
suffix-chars = 2000
"#,
    )
    .expect("AI provider configuration must coexist with normal completion");
    assert!(config.auto_completion);
    assert!(config.path_completion);
}
