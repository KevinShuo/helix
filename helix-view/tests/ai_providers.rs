use helix_view::{
    ai::{AiFeature, AiProviderConfig, AiProviderType, AiProxy},
    editor::Config,
};

#[test]
fn local_runtime_settings_parse_and_round_trip_without_changing_defaults() {
    let provider = toml::from_str::<AiProviderConfig>(
        "type = 'llama-cpp'\nmodel = 'local'\n[runtime]\ncache-type-k = 'q8_0'\ncache-type-v = 'q8_0'\nspeculative-decoding = 'mtp'\ndraft-tokens = 3\nthreads = 8\nthreads-batch = 12\nbatch-size = 2048\nubatch-size = 256\n",
    );
    assert!(
        provider.is_ok(),
        "local runtime settings must parse: {provider:?}"
    );
    let provider = provider.unwrap();
    let serialized = toml::to_string(&provider).unwrap();
    let round_trip: AiProviderConfig = toml::from_str(&serialized).unwrap();
    assert_eq!(provider, round_trip);
    assert!(serialized.contains("cache-type-k = \"q8_0\""));
    assert!(serialized.contains("speculative-decoding = \"mtp\""));
    let legacy = AiProviderConfig::from_legacy(&Config::default().ai_completion);
    assert_eq!(
        legacy.context_size,
        Config::default().ai_completion.context_size
    );
}

#[test]
fn local_runtime_rejects_unknown_values_and_invalid_resource_budgets() {
    for runtime in [
        "cache-type-k = 'typo'",
        "cache-type-v = 'typo'",
        "speculative-decoding = 'typo'",
        "threads = 0",
        "threads = 1025",
        "threads-batch = 0",
        "batch-size = 0",
        "ubatch-size = 0",
        "batch-size = 65537",
        "draft-tokens = 0",
        "draft-tokens = 65",
        "batch-size = 256\nubatch-size = 512",
        "extra-args = '--host 0.0.0.0'",
    ] {
        let parsed = toml::from_str::<AiProviderConfig>(&format!(
            "type = 'llama-cpp'\nmodel = 'test'\nendpoint = 'http://127.0.0.1:19240/v1/chat/completions'\n[runtime]\n{runtime}\n"
        ));
        if let Ok(provider) = parsed {
            let mut config = Config::default();
            config.ai.default_provider = Some("test".into());
            config.ai.providers.insert("test".into(), provider);
            assert!(
                config.ai_provider(AiFeature::Completion).is_err(),
                "{runtime}"
            );
        }
    }
}

#[test]
fn local_runtime_settings_are_rejected_for_remote_provider_types() {
    for kind in ["deepseek", "openai-compatible"] {
        let provider: AiProviderConfig = toml::from_str(&format!(
            "type = '{kind}'\nmodel = 'test'\nendpoint = 'http://127.0.0.1:19240/v1/chat/completions'\n[runtime]\ncache-type-k = 'q8_0'\n"
        )).unwrap();
        let mut config = Config::default();
        config.ai.default_provider = Some("test".into());
        config.ai.providers.insert("test".into(), provider);
        assert!(config
            .ai_provider(AiFeature::Completion)
            .unwrap_err()
            .to_string()
            .contains("runtime"));
    }
}

#[test]
fn local_runtime_backend_sampling_is_opt_in_and_round_trips() {
    let parsed = toml::from_str::<AiProviderConfig>(
        "type = 'llama-cpp'\n[runtime]\nbackend-sampling = true\n",
    );
    assert!(
        parsed.is_ok(),
        "backend sampling must be configurable: {parsed:?}"
    );
    let provider = parsed.unwrap();
    let serialized = toml::to_string(&provider).unwrap();
    assert!(serialized.contains("backend-sampling = true"));
    assert_eq!(provider, toml::from_str(&serialized).unwrap());
    assert!(toml::from_str::<AiProviderConfig>("[runtime]\nbackend-sampling = 'yes'\n").is_err());
    assert!(toml::to_string(&AiProviderConfig::default())
        .unwrap()
        .contains("backend-sampling = false"));
}

#[test]
fn local_runtime_model_loading_mode_is_typed_and_opt_in() {
    for mode in ["auto", "mmap", "none", "dio"] {
        let parsed = toml::from_str::<AiProviderConfig>(&format!(
            "type = 'llama-cpp'\n[runtime]\nload-mode = '{mode}'\n"
        ));
        assert!(parsed.is_ok(), "load mode {mode} must parse: {parsed:?}");
        assert!(toml::to_string(&parsed.unwrap())
            .unwrap()
            .contains(&format!("load-mode = \"{mode}\"")));
    }
    assert!(toml::from_str::<AiProviderConfig>("[runtime]\nload-mode = 'typo'\n").is_err());
    assert!(toml::to_string(&AiProviderConfig::default())
        .unwrap()
        .contains("load-mode = \"auto\""));
}

#[test]
fn merge_loads_27b_on_demand_without_changing_resident_completion_9b() {
    let parsed = toml::from_str::<Config>(
        r#"
[ai]
default-provider = "local"
[ai.providers.local]
type = "llama-cpp"
endpoint = "http://127.0.0.1:19240/v1/chat/completions"
model = "9B"
model-path = '9b.gguf'
[ai.providers.merge-27b]
type = "llama-cpp"
endpoint = "http://127.0.0.1:19242/v1/chat/completions"
model = "27B"
model-path = '27b.gguf'
context-size = 16384
[ai-completion]
enabled = true
provider = "local"
[ai-merge]
enabled = true
local-provider = "merge-27b"
"#,
    );
    assert!(
        parsed.is_ok(),
        "Merge provider config must parse: {parsed:?}"
    );
    let config = parsed.unwrap();
    config.validate_ai().unwrap();
    let providers = config.active_ai_providers().unwrap();
    assert_eq!(providers.len(), 2);
    assert!(providers.iter().any(|p| p.model == "9B" && !p.on_demand));
    assert!(providers.iter().any(|p| p.model == "27B" && p.on_demand));
    let completion = config.ai_provider(AiFeature::Completion).unwrap();
    assert_eq!(completion.model, "9B");
    assert!(!completion.on_demand);
    let merge = config.ai_provider(AiFeature::Merge).unwrap();
    assert_eq!(merge.model, "27B");
    assert!(merge.on_demand);
    assert!(!merge.thinking);
    let mut disabled = config.clone();
    disabled.ai_merge.enabled = false;
    assert_eq!(disabled.active_ai_providers().unwrap().len(), 1);
    let mut invalid = config;
    invalid.ai_merge.local_provider = Some("missing".into());
    assert!(invalid.validate_ai().is_err());
}

#[test]
fn unified_ai_provider_configuration_is_accepted() {
    let parsed = toml::from_str::<Config>(
        r#"
[ai]
default-provider = "deepseek"
[ai.providers.deepseek]
type = "deepseek"
endpoint = "https://api.deepseek.com/chat/completions"
model = "deepseek-flash"
api-key-env = "DEEPSEEK_API_KEY"
thinking = false
proxy = "system"
[ai-completion]
enabled = true
provider = "deepseek"
[ai-doc]
enabled = true
provider = "deepseek"
[ai-commit]
enabled = true
provider = "deepseek"
"#,
    );
    assert!(
        parsed.is_ok(),
        "unified AI configuration must parse: {parsed:?}"
    );
    let config = parsed.unwrap();
    config.validate_ai().unwrap();
    for feature in [
        AiFeature::Completion,
        AiFeature::Doc,
        AiFeature::Commit,
        AiFeature::Merge,
    ] {
        let provider = config.ai_provider(feature).unwrap();
        assert_eq!(provider.model, "deepseek-flash");
        assert_eq!(provider.kind, AiProviderType::Deepseek);
        assert_eq!(provider.proxy, AiProxy::System);
        assert!(!provider.thinking);
        assert!(provider.model_path.is_none());
    }
}

#[test]
fn shipped_ai_provider_example_switches_local_and_online_generation_with_one_setting() {
    let example: toml::Value =
        toml::from_str(include_str!("../../contrib/config-ai-providers.toml")).unwrap();
    let mut config: Config = example["editor"].clone().try_into().unwrap();
    config.validate_ai().unwrap();
    assert_eq!(
        config.ai_provider(AiFeature::Completion).unwrap().kind,
        AiProviderType::LlamaCpp
    );
    for feature in [AiFeature::Doc, AiFeature::Commit] {
        let model = config.ai_provider(feature).unwrap();
        assert_eq!(model.kind, AiProviderType::LlamaCpp);
        assert!(model.on_demand);
        assert!(model.thinking);
        assert_eq!(model.thinking_budget, Some(2048));
    }
    let merge = config.ai_provider(AiFeature::Merge).unwrap();
    assert_eq!(merge.model, "Qwen3.8-27B-UD-Q5_K_M");
    assert!(merge.on_demand && !merge.thinking);
    config.ai.default_provider = Some("deepseek".into());
    for feature in [
        AiFeature::Completion,
        AiFeature::Doc,
        AiFeature::Commit,
        AiFeature::Merge,
    ] {
        assert_eq!(
            config.ai_provider(feature).unwrap().kind,
            AiProviderType::Deepseek
        );
    }
}

fn mixed() -> Config {
    toml::from_str(
        r#"
[ai]
default-provider = "cloud"
[ai.providers.cloud]
type = "deepseek"
endpoint = "https://api.deepseek.com/chat/completions"
model = "cloud-model"
[ai.providers.local]
type = "llama-cpp"
endpoint = "http://127.0.0.1:19239/v1/chat/completions"
model = "local-model"
model-path = 'C:\models\coder.gguf'
[ai.providers.other]
type = "openai-compatible"
endpoint = "https://example.com/custom/chat/completions"
model = "other-model"
[ai-completion]
enabled = true
provider = "local"
[ai-doc]
enabled = true
[ai-commit]
enabled = true
provider = "other"
"#,
    )
    .unwrap()
}

#[test]
fn three_features_resolve_independently_and_default_is_only_a_fallback() {
    let config = mixed();
    config.validate_ai().unwrap();
    assert_eq!(
        config.ai_provider(AiFeature::Completion).unwrap().model,
        "local-model"
    );
    assert_eq!(
        config.ai_provider(AiFeature::Doc).unwrap().model,
        "cloud-model"
    );
    assert_eq!(
        config.ai_provider(AiFeature::Commit).unwrap().model,
        "other-model"
    );
    assert_eq!(config.active_ai_providers().unwrap().len(), 3);
}

#[test]
fn legacy_connections_are_preserved_for_every_feature() {
    let config: Config = toml::from_str(
        r#"
[ai-completion]
endpoint = "http://127.0.0.1:19240/v1/chat/completions"
model = "legacy"
model-path = 'C:\models\legacy.gguf'
server-path = 'C:\tools\llama-server.exe'
context-size = 8192
gpu-layers = 20
api-key-env = "OLD_AI_KEY"
[ai-doc]
enabled = true
"#,
    )
    .unwrap();
    for feature in [
        AiFeature::Completion,
        AiFeature::Doc,
        AiFeature::Commit,
        AiFeature::Merge,
    ] {
        let model = config.ai_provider(feature).unwrap();
        assert_eq!(model, AiProviderConfig::from_legacy(&config.ai_completion));
        assert_eq!(model.model, "legacy");
        assert_eq!(model.context_size, 8192);
        assert_eq!(model.proxy, AiProxy::None);
        assert_eq!(
            model.server_path.to_str(),
            Some(r"C:\tools\llama-server.exe")
        );
    }
    assert_eq!(config.active_ai_providers().unwrap().len(), 1);
    assert!(Config::default().active_ai_providers().unwrap().is_empty());
}

#[test]
fn unknown_explicit_provider_never_falls_back_to_default_or_legacy() {
    let mut config = mixed();
    config.ai_doc.provider = Some("typo".into());
    assert!(config
        .ai_provider(AiFeature::Doc)
        .unwrap_err()
        .to_string()
        .contains("typo"));
    assert!(config.validate_ai().is_err());
    config.ai_doc.provider = None;
    config.ai.default_provider = Some("missing".into());
    assert!(config.validate_ai().is_err());
    config.ai.default_provider = None;
    assert_eq!(
        config.ai_provider(AiFeature::Doc).unwrap().model,
        config.ai_completion.model
    );
}

#[test]
fn disabled_features_do_not_activate_providers_and_shared_providers_are_deduplicated() {
    let mut config = mixed();
    config.ai_completion.enabled = false;
    config.ai_commit.enabled = false;
    assert!(config
        .active_ai_providers()
        .unwrap()
        .iter()
        .all(|p| p.model_path.is_none()));
    config.ai_completion.enabled = true;
    config.ai_completion.provider = Some("cloud".into());
    assert_eq!(config.active_ai_providers().unwrap().len(), 1);
}

#[test]
fn cloud_provider_cannot_start_a_local_model_and_local_endpoint_conflicts_are_rejected() {
    let mut config = mixed();
    config.ai.providers.get_mut("cloud").unwrap().model_path = Some("secret.gguf".into());
    assert!(config
        .validate_ai()
        .unwrap_err()
        .to_string()
        .contains("model-path"));
    config.ai.providers.get_mut("cloud").unwrap().model_path = None;
    let mut other_local = config.ai.providers["local"].clone();
    other_local.model = "another-local-model".into();
    config.ai.providers.insert("other".into(), other_local);
    assert!(config
        .validate_ai()
        .unwrap_err()
        .to_string()
        .contains("endpoint"));
    config.ai.providers.get_mut("other").unwrap().endpoint =
        "http://127.0.0.1:19240/v1/chat/completions".into();
    config.validate_ai().unwrap();
}

#[test]
fn local_provider_conflicts_use_effective_port_and_include_existing_servers() {
    for endpoint in [
        "http://127.0.0.1:019239/v1/chat/completions",
        "http://127.0.0.1:19239/v1/chat/completions",
    ] {
        let mut config = mixed();
        let mut other = config.ai.providers["local"].clone();
        other.endpoint = endpoint.into();
        other.model = "other-local-model".into();
        // A connection to an already-running local server must not silently
        // attach to a different model started for another feature.
        if endpoint.contains(":19239/") {
            other.model_path = None;
        }
        config.ai.providers.insert("other".into(), other);
        assert!(
            config.validate_ai().is_err(),
            "accepted conflicting local endpoint {endpoint}"
        );
    }
    let mut config = mixed();
    config.ai.providers.get_mut("local").unwrap().endpoint =
        "http://127.0.0.1/v1/chat/completions".into();
    let mut other = config.ai.providers["local"].clone();
    other.endpoint = "http://127.0.0.1:80/v1/chat/completions".into();
    other.model = "other".into();
    config.ai.providers.insert("other".into(), other);
    assert!(config.validate_ai().is_err());
}

#[test]
fn local_commit_thinking_follows_global_mode_and_preserves_explicit_overrides() {
    let source = r#"
[ai]
default-provider = "local"
[ai.providers.local]
type = "llama-cpp"
endpoint = "http://127.0.0.1:19240/v1/chat/completions"
model = "9B"
model-path = '9b.gguf'
[ai.providers.commit-27b]
type = "llama-cpp"
endpoint = "http://127.0.0.1:19241/v1/chat/completions"
model = "27B"
model-path = '27b.gguf'
thinking = true
thinking-budget = 2048
reasoning-effort = "medium"
context-size = 16384
[ai.providers.deepseek]
type = "deepseek"
endpoint = "https://api.deepseek.com/chat/completions"
model = "deepseek-flash"
[ai-completion]
enabled = true
[ai-doc]
enabled = true
local-provider = "commit-27b"
[ai-commit]
enabled = true
local-provider = "commit-27b"
"#;
    let parsed = toml::from_str::<Config>(source);
    assert!(
        parsed.is_ok(),
        "conditional local commit provider must parse: {parsed:?}"
    );
    let mut config = parsed.unwrap();
    config.validate_ai().unwrap();
    assert_eq!(
        config.ai_provider(AiFeature::Completion).unwrap().model,
        "9B"
    );
    assert_eq!(config.ai_provider(AiFeature::Doc).unwrap().model, "27B");
    let commit = config.ai_provider(AiFeature::Commit).unwrap();
    assert_eq!(commit.model, "27B");
    assert!(commit.on_demand);
    assert!(commit.thinking);
    config.ai.default_provider = Some("deepseek".into());
    assert_eq!(
        config.ai_provider(AiFeature::Commit).unwrap().model,
        "deepseek-flash"
    );
    config.ai.default_provider = Some("local".into());
    config.ai_commit.provider = Some("local".into());
    assert_eq!(config.ai_provider(AiFeature::Commit).unwrap().model, "9B");
    config.ai_commit.provider = None;
    let invalid = source.replace(
        "local-provider = \"commit-27b\"",
        "local-provider = \"typo\"",
    );
    let invalid: Config = toml::from_str(&invalid).unwrap();
    assert!(invalid.validate_ai().is_err());
    let invalid: Config = toml::from_str(
        &source
            .replace(
                "local-provider = \"commit-27b\"",
                "local-provider = \"typo\"",
            )
            .replace(
                "default-provider = \"local\"",
                "default-provider = \"deepseek\"",
            ),
    )
    .unwrap();
    assert!(
        invalid.validate_ai().is_err(),
        "invalid conditional references must fail even in online mode"
    );
}
#[test]
fn test_generation_configuration_is_supported() {
    let parsed = toml::from_str::<Config>(
        "[ai-test]\nenabled = true\nlocal-provider = 'local-27b'\nallow-fetch-content = true\n",
    );
    assert!(
        parsed.is_ok(),
        "AI test configuration must parse: {parsed:?}"
    );
}

#[test]
fn test_generation_follows_global_switch_and_uses_bounded_local_27b_on_demand() {
    let mut config = mixed();
    config.ai_test.enabled = true;
    let mut large = config.ai.providers["local"].clone();
    large.model = "27B".into();
    large.endpoint = "http://127.0.0.1:19241/v1/chat/completions".into();
    large.model_path = Some("27b.gguf".into());
    large.thinking = true;
    config.ai.providers.insert("local-27b".into(), large);
    config.ai_test.local_provider = Some("local-27b".into());
    assert_eq!(
        config.ai_provider(AiFeature::Test).unwrap().model,
        "cloud-model"
    );
    config.ai.default_provider = Some("local".into());
    let model = config.ai_provider(AiFeature::Test).unwrap();
    assert_eq!(model.model, "27B");
    assert!(model.on_demand && model.thinking);
    assert_eq!(model.thinking_budget, Some(2048));
    assert_eq!(model.reasoning_tokens(), 2048);
    config.validate_ai().unwrap();
    assert!(config
        .active_ai_providers()
        .unwrap()
        .iter()
        .any(|p| p.model == "27B" && p.on_demand));
    config.ai_test.provider = Some("cloud".into());
    assert_eq!(
        config.ai_provider(AiFeature::Test).unwrap().model,
        "cloud-model"
    );
    config.ai_test.local_provider = Some("typo".into());
    assert!(config.validate_ai().is_err());
}
