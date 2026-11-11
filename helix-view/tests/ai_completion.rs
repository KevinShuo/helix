use helix_view::{ai_completion::AiCompletionPromptMode, editor::Config};

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
    assert_eq!(
        config.ai_completion.prompt_mode,
        AiCompletionPromptMode::Insertion
    );
}

#[test]
fn ai_completion_line_mode_coexists_with_regular_completion() {
    let config: Config = toml::from_str(
        r#"
[ai-completion]
enabled = true
prompt-mode = "line"
"#,
    )
    .expect("line completion must be an opt-in mode of the existing provider");
    assert!(config.auto_completion);
    assert!(config.path_completion);
    assert_eq!(
        config.ai_completion.prompt_mode,
        AiCompletionPromptMode::Line
    );
}

#[test]
fn ai_completion_configures_a_local_gguf_with_windows_paths() {
    let config: Config = toml::from_str(
        r#"
[ai-completion]
enabled = true
model = "Qwen2.5-Coder-7B-Instruct-Q6_K"
model-path = 'C:\Users\wangshuo\.local\share\junie-local\models\Qwen2.5-Coder-7B-Instruct-Q6_K.gguf'
server-path = 'C:\Program Files\llama.cpp\llama-server.exe'
endpoint = "http://127.0.0.1:19240/v1/chat/completions"
prompt-mode = "line"
context-size = 4096
gpu-layers = 99
startup-timeout-ms = 120000
"#,
    )
    .expect("a local GGUF must be configurable without the PowerShell launcher");
    let settings = toml::Value::try_from(&config.ai_completion).unwrap();
    assert_eq!(
        settings["model-path"].as_str(),
        Some(
            r"C:\Users\wangshuo\.local\share\junie-local\models\Qwen2.5-Coder-7B-Instruct-Q6_K.gguf"
        )
    );
    assert_eq!(
        settings["server-path"].as_str(),
        Some(r"C:\Program Files\llama.cpp\llama-server.exe")
    );
    assert!(config.ai_completion.enabled);
    assert!(config.auto_completion);
    assert!(config.path_completion);
}

#[test]
fn ai_completion_can_disable_a_configured_local_model() {
    let config: Config = toml::from_str(
        r#"
[ai-completion]
enabled = false
model-path = 'missing-model.gguf'
"#,
    )
    .expect("a disabled provider must retain its local model setting");
    assert!(!config.ai_completion.enabled);
    assert!(config.auto_completion);
}
