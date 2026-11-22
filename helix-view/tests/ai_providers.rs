use helix_view::{
    ai::{AiBudgetMode, AiFeature, AiProviderConfig, AiProviderType, AiProxy},
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
fn provider_budget_mode_configuration_is_accepted() {
    for (mode, expected) in [
        ("auto", AiBudgetMode::Auto),
        ("configured", AiBudgetMode::Configured),
    ] {
        let parsed = toml::from_str::<AiProviderConfig>(&format!(
            "type = 'deepseek'\nmodel = 'deepseek-flash'\nbudget-mode = '{mode}'\n"
        ));
        assert!(parsed.is_ok(), "budget-mode {mode} must parse: {parsed:?}");
        let provider = parsed.unwrap();
        assert_eq!(provider.budget_mode, expected);
        let serialized = toml::to_string(&provider).unwrap();
        assert!(serialized.contains(&format!("budget-mode = \"{mode}\"")));
    }
    assert_eq!(AiProviderConfig::default().budget_mode, AiBudgetMode::Auto);
    assert_eq!(
        AiProviderConfig::from_legacy(&Config::default().ai_completion).budget_mode,
        AiBudgetMode::Auto
    );
    let defaulted: AiProviderConfig =
        toml::from_str("type = 'deepseek'\nmodel = 'deepseek-flash'\n").unwrap();
    assert!(defaulted.automatic_budget());
    assert!(toml::from_str::<AiProviderConfig>("budget-mode = 'unbounded'").is_err());
}

fn deepseek_budget(model: &str) -> AiProviderConfig {
    AiProviderConfig {
        kind: AiProviderType::Deepseek,
        model: model.into(),
        ..Default::default()
    }
}

#[test]
fn recognized_deepseek_models_use_official_automatic_budgets() {
    for id in [
        "deepseek-flash",
        "deepseek-v4-pro",
        "deepseek-v4-flash",
        "deepseek-v4-flash-vision-exp",
    ] {
        let mut model = deepseek_budget(id);
        assert!(model.automatic_budget(), "{id}");
        assert_eq!(model.budget_context(), 1_048_576);
        assert_eq!(model.budget_input_bytes(1), 16_777_216);
        assert_eq!(model.budget_output_tokens(1), 393_216);
        assert_eq!(model.budget_timeout_ms(1), 7_200_000);
        assert_eq!(model.budget_response_bytes(), 16_777_216);
        assert_eq!(model.budget_artifact_bytes(1), 16_777_216);
        assert_eq!(model.budget_file_limit(1), 4096);
        assert_eq!(model.budget_batch_files(), 128);
        assert_eq!(model.budget_request_limit(), 256);
        // Bytes are not a tokenizer: a complete large UTF-8 request can be
        // admitted without treating every byte as a token.
        assert!(model.budget_fits(2_000_000, 393_216, 1024));
        assert!(model.budget_fits(16_777_200, 393_216, 16));
        assert!(!model.budget_fits(16_777_200, 393_216, 17));
        assert!(!model.budget_fits(1, 393_217, 1));
        assert!(!model.budget_fits(usize::MAX, 1, 1));
        assert!(!model.budget_fits(1, 1, usize::MAX));
        assert_eq!(model.budget_input_bytes(usize::MAX), 16_777_216);
        assert_eq!(model.budget_output_tokens(u32::MAX), 393_216);
        assert_eq!(model.budget_timeout_ms(u64::MAX), 7_200_000);
        assert_eq!(model.budget_artifact_bytes(usize::MAX), 16_777_216);
        assert_eq!(model.budget_file_limit(usize::MAX), 4096);
        model.context_size = 0;
        assert_eq!(model.budget_context(), 1_048_576);
        model.context_size = u32::MAX;
        assert_eq!(model.budget_context(), 1_048_576);
        assert_eq!(model.budget_input_bytes(0), 16_777_216);
        assert_eq!(model.budget_output_tokens(0), 393_216);
        assert_eq!(model.budget_timeout_ms(0), 7_200_000);
        assert_eq!(model.budget_artifact_bytes(0), 16_777_216);
        assert_eq!(model.budget_file_limit(0), 4096);
    }
}

#[test]
fn configured_deepseek_honors_feature_limits_with_service_ceiling() {
    let mut model = deepseek_budget("deepseek-flash");
    model.budget_mode = AiBudgetMode::Configured;
    model.context_size = 8192;
    assert!(!model.automatic_budget());
    assert_eq!(model.budget_context(), 8192);
    assert_eq!(model.budget_input_bytes(4096), 4096);
    assert_eq!(model.budget_output_tokens(20_000), 20_000);
    assert_eq!(model.budget_timeout_ms(900_000), 900_000);
    assert_eq!(model.budget_response_bytes(), 16_777_216);
    assert_eq!(model.budget_artifact_bytes(8192), 16_777_216);
    assert_eq!(model.budget_file_limit(128), 128);
    assert_eq!(model.budget_file_limit(usize::MAX), 1024);
    assert_eq!(model.budget_batch_files(), 4);
    assert_eq!(model.budget_request_limit(), 64);
    assert!(model.budget_fits(6000, 2000, 192));
    assert!(!model.budget_fits(6000, 2000, 193));
    assert!(!model.budget_fits(usize::MAX, u32::MAX, usize::MAX));
    model.context_size = u32::MAX;
    assert_eq!(model.budget_context(), 1_048_576);
    assert_eq!(model.budget_output_tokens(u32::MAX), 393_216);
    assert_eq!(model.budget_timeout_ms(u64::MAX), 7_200_000);
    model.context_size = 0;
    assert_eq!(model.budget_context(), 512);
    assert_eq!(model.budget_output_tokens(0), 0);
    assert_eq!(model.budget_input_bytes(0), 0);
    assert_eq!(model.budget_timeout_ms(0), 100);
    assert_eq!(model.budget_file_limit(0), 0);
}

#[test]
fn unknown_models_and_other_provider_types_keep_configured_budgets() {
    for (kind, id) in [
        (AiProviderType::Deepseek, "future-model"),
        (AiProviderType::Deepseek, "deepseek-flash-custom"),
        (AiProviderType::Deepseek, "DEEPSEEK-FLASH"),
        (AiProviderType::LlamaCpp, "deepseek-flash"),
        (AiProviderType::OpenaiCompatible, "deepseek-flash"),
    ] {
        for mode in [AiBudgetMode::Auto, AiBudgetMode::Configured] {
            let mut model = deepseek_budget(id);
            model.kind = kind;
            model.budget_mode = mode;
            model.context_size = 16384;
            assert!(!model.automatic_budget(), "{kind:?} {id} {mode:?}");
            assert_eq!(model.budget_context(), 16384);
            assert_eq!(model.budget_input_bytes(12_000), 12_000);
            assert_eq!(model.budget_output_tokens(2048), 2048);
            assert_eq!(model.budget_timeout_ms(180_000), 180_000);
            assert_eq!(model.budget_response_bytes(), 262_144);
            assert_eq!(model.budget_artifact_bytes(12_000), 12_000);
            assert_eq!(model.budget_file_limit(128), 128);
            assert_eq!(model.budget_batch_files(), 4);
            assert_eq!(model.budget_request_limit(), 64);
            assert!(model.budget_fits(12_000, 4096, 288));
            assert!(!model.budget_fits(12_000, 4096, 289));
            assert!(!model.budget_fits(usize::MAX, u32::MAX, usize::MAX));
            model.context_size = u32::MAX;
            assert_eq!(model.budget_context(), 131_072);
            assert_eq!(model.budget_output_tokens(u32::MAX), 16_384);
            assert_eq!(model.budget_timeout_ms(u64::MAX), 600_000);
            assert_eq!(model.budget_file_limit(usize::MAX), 1024);
            assert_eq!(model.budget_input_bytes(usize::MAX), usize::MAX);
            assert_eq!(model.budget_artifact_bytes(usize::MAX), usize::MAX);
            model.context_size = 0;
            assert_eq!(model.budget_context(), 512);
            assert_eq!(model.budget_input_bytes(0), 0);
            assert_eq!(model.budget_output_tokens(0), 0);
            assert_eq!(model.budget_timeout_ms(0), 100);
            assert_eq!(model.budget_artifact_bytes(0), 0);
            assert_eq!(model.budget_file_limit(0), 0);
        }
    }
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

fn installed_example() -> (Config, tempfile::NamedTempFile) {
    let example: toml::Value =
        toml::from_str(include_str!("../../contrib/config-ai-providers.toml")).unwrap();
    let mut config: Config = example["editor"].clone().try_into().unwrap();
    let weights = tempfile::NamedTempFile::new().unwrap();
    for p in config
        .ai
        .providers
        .values_mut()
        .filter(|p| p.model_path.is_some())
    {
        p.model_path = Some(weights.path().into());
        p.server_path = std::env::current_exe().unwrap();
    }
    (config, weights)
}

#[test]
fn shipped_settings_use_deepseek_on_a_laptop_and_27b_on_a_capable_desktop() {
    let (mut config, _weights) = installed_example();
    config.validate_ai().unwrap();
    config.apply_ai_host(|| (24 << 30, true)).unwrap();
    assert!(config.laptop());
    let inline = config.ai_provider(AiFeature::Completion).unwrap();
    assert_eq!(inline.kind, AiProviderType::LlamaCpp);
    assert!(!inline.on_demand);
    for (feature, _) in config.ai_features() {
        if feature == AiFeature::Completion {
            continue;
        }
        // Even a saved local override must not start a manual model on a laptop.
        *config.ai_feature_settings(feature).1 = Some("local-27b".into());
        assert_eq!(
            config.ai_provider(feature).unwrap().kind,
            AiProviderType::Deepseek
        );
        *config.ai_feature_settings(feature).1 = None;
    }
    assert!(config
        .active_ai_providers()
        .unwrap()
        .iter()
        .all(|p| p == &inline || p.kind == AiProviderType::Deepseek));
    config.apply_ai_host(|| (24 << 30, false)).unwrap();
    assert!(!config.laptop());
    assert_eq!(
        config.ai_provider(AiFeature::Completion).unwrap().kind,
        AiProviderType::LlamaCpp
    );
    for feature in [
        AiFeature::Doc,
        AiFeature::Commit,
        AiFeature::Test,
        AiFeature::Explain,
        AiFeature::Generate,
    ] {
        let model = config.ai_provider(feature).unwrap();
        assert_eq!(model.kind, AiProviderType::LlamaCpp);
        assert_eq!(model.model, "Qwen3.8-27B-UD-Q5_K_M");
        assert!(model.on_demand);
        assert!(model.thinking);
        assert_eq!(model.thinking_budget, Some(2048));
    }
    let merge = config.ai_provider(AiFeature::Merge).unwrap();
    assert_eq!(merge.model, "Qwen3.8-27B-UD-Q5_K_M");
    assert!(merge.on_demand && !merge.thinking);
    config.apply_ai_host(|| (1 << 30, false)).unwrap();
    for feature in [
        AiFeature::Doc,
        AiFeature::Commit,
        AiFeature::Merge,
        AiFeature::Explain,
        AiFeature::Generate,
    ] {
        assert_eq!(
            config.ai_provider(feature).unwrap().kind,
            AiProviderType::Deepseek
        );
    }
}

const MANUAL_FEATURES: [AiFeature; 6] = [
    AiFeature::Doc,
    AiFeature::Commit,
    AiFeature::Test,
    AiFeature::Merge,
    AiFeature::Explain,
    AiFeature::Generate,
];

#[test]
fn shared_local_generation_switches_six_routes_and_restores_automatic_models() {
    let (mut config, _weights) = installed_example();
    let mut alternative = config.ai.providers["local-27b"].clone();
    alternative.model = "alternate-coder".into();
    alternative.endpoint = "http://127.0.0.1:19250/v1/chat/completions".into();
    config.ai.providers.insert("alternate".into(), alternative);
    config.apply_ai_host(|| (24 << 30, false)).unwrap();
    let inline = config.ai_provider(AiFeature::Completion).unwrap();
    let defaults = MANUAL_FEATURES.map(|f| config.ai_provider(f).unwrap());

    config.ai.local_generation_provider = Some("alternate".into());
    config.apply_ai_host(|| (24 << 30, false)).unwrap();
    for feature in MANUAL_FEATURES {
        let selected = config.ai_provider(feature).unwrap();
        assert_eq!(selected.model, "alternate-coder", "{feature:?}");
        assert!(selected.on_demand, "{feature:?}");
    }
    assert_eq!(config.ai_provider(AiFeature::Completion).unwrap(), inline);
    let registered = config.active_ai_providers().unwrap();
    assert_eq!(registered.len(), 2, "{registered:?}");
    assert!(registered.contains(&inline));
    assert!(registered
        .iter()
        .any(|p| p.model == "alternate-coder" && p.on_demand));

    config.ai_doc.provider = Some("deepseek".into());
    assert_eq!(
        config.ai_provider(AiFeature::Doc).unwrap().kind,
        AiProviderType::Deepseek
    );
    assert_eq!(
        config.ai_provider(AiFeature::Generate).unwrap().model,
        "alternate-coder"
    );
    config.ai_doc.provider = None;

    config.apply_ai_host(|| (24 << 30, true)).unwrap();
    for feature in MANUAL_FEATURES {
        assert_eq!(
            config.ai_provider(feature).unwrap().kind,
            AiProviderType::Deepseek,
            "{feature:?}"
        );
    }
    assert_eq!(config.ai_provider(AiFeature::Completion).unwrap(), inline);
    assert!(config
        .active_ai_providers()
        .unwrap()
        .iter()
        .all(|p| p == &inline || p.kind == AiProviderType::Deepseek));
    config.apply_ai_host(|| (24 << 30, false)).unwrap();
    assert_eq!(
        config.ai_provider(AiFeature::Generate).unwrap().model,
        "alternate-coder"
    );

    config.ai.local_generation_provider = None;
    config.apply_ai_host(|| (24 << 30, false)).unwrap();
    for (feature, original) in MANUAL_FEATURES.into_iter().zip(defaults) {
        assert_eq!(
            config.ai_provider(feature).unwrap(),
            original,
            "{feature:?}"
        );
    }
    assert_eq!(config.ai_provider(AiFeature::Completion).unwrap(), inline);
}

#[test]
fn unavailable_shared_generation_falls_back_without_displacing_resident_inline() {
    let (mut config, _weights) = installed_example();
    config.ai.min_local_vram_gib = 0;
    let bigger = tempfile::NamedTempFile::new().unwrap();
    bigger.as_file().set_len(256).unwrap();
    let mut chosen = config.ai.providers["local-27b"].clone();
    chosen.model_path = Some(bigger.path().into());
    chosen.model = "too-large".into();
    config.ai.providers.insert("chosen".into(), chosen);
    config.ai.local_generation_provider = Some("chosen".into());
    // Inline fits exactly with the required 1 GiB headroom. The chosen file does not.
    let verify = |config: &mut Config| {
        config.apply_ai_host(|| ((1 << 30) + 128, false)).unwrap();
        let inline = config.ai_provider(AiFeature::Completion).unwrap();
        assert_eq!(inline.model, "local-coder");
        assert!(!inline.on_demand);
        for feature in MANUAL_FEATURES {
            assert_eq!(
                config.ai_provider(feature).unwrap().kind,
                AiProviderType::Deepseek,
                "{feature:?}"
            );
        }
        assert!(config
            .active_ai_providers()
            .unwrap()
            .iter()
            .all(|p| p == &inline || p.kind == AiProviderType::Deepseek));
    };
    verify(&mut config);
    drop(bigger);
    verify(&mut config);
}

#[test]
fn explicit_feature_and_shared_generation_reuse_one_model_lease() {
    let (mut config, _weights) = installed_example();
    config.ai.local_generation_provider = Some("local-27b".into());
    config.ai_doc.provider = Some("local-27b".into());
    config.apply_ai_host(|| (24 << 30, false)).unwrap();
    let doc = config.ai_provider(AiFeature::Doc).unwrap();
    let generated = config.ai_provider(AiFeature::Generate).unwrap();
    assert_eq!(doc.endpoint, generated.endpoint);
    assert!(doc.on_demand && generated.on_demand);
    assert_eq!(config.active_ai_providers().unwrap().len(), 2);
}

#[test]
fn independent_model_directories_select_the_largest_fit_and_allow_a_smaller_override() {
    let root = tempfile::tempdir().unwrap();
    let first = root.path().join("first");
    let second = root.path().join("second");
    std::fs::create_dir_all(&first).unwrap();
    std::fs::create_dir_all(&second).unwrap();
    std::fs::write(first.join("small.gguf"), [0; 32]).unwrap();
    std::fs::write(second.join("medium.gguf"), [0; 64]).unwrap();
    std::fs::write(second.join("large.gguf"), [0; 256]).unwrap();
    std::fs::write(second.join("tiny-3B.Q8_0.gguf"), [0; 512]).unwrap();
    let mut config = Config::default();
    config.ai.min_local_vram_gib = 0;
    config.ai.local_model_directories = vec![first, second];
    for feature in MANUAL_FEATURES {
        *config.ai_feature_settings(feature).0 = true;
    }
    config.load_local_model_catalog();
    assert_eq!(config.ai.providers.len(), 4);
    // The catalog above has no provider template. Replace its executable in this
    // fixture so checking availability never depends on llama-server being on PATH.
    let runner = std::env::current_exe().unwrap();
    for provider in config.ai.providers.values_mut() {
        provider.server_path = runner.clone();
    }
    let small = config
        .ai
        .providers
        .iter()
        .find(|(_, p)| p.model == "small")
        .unwrap()
        .0
        .clone();
    let tiny = config
        .ai
        .providers
        .iter()
        .find(|(_, p)| p.model == "tiny-3B.Q8_0")
        .unwrap()
        .0
        .clone();
    for (choice, capacity, expected) in [
        (None, 128, "medium"),
        (Some(small), 128, "small"),
        (None, 128, "medium"),
        (None, 256, "large"),
        (None, 1024, "large"),
        (Some(tiny), 1024, "tiny-3B.Q8_0"),
    ] {
        config.ai.local_generation_provider = choice;
        config
            .apply_ai_host(|| ((1 << 30) + capacity, false))
            .unwrap();
        let active = config.active_ai_providers().unwrap();
        assert_eq!(active.len(), 1, "{active:?}");
        assert!(active[0].on_demand);
        for feature in MANUAL_FEATURES {
            assert_eq!(
                config.ai_provider(feature).unwrap().model,
                expected,
                "{feature:?}"
            );
        }
    }
}

#[test]
fn a_normal_editor_option_change_preserves_automatic_model_discovery() {
    let (mut config, _weights) = installed_example();
    let library = tempfile::tempdir().unwrap();
    std::fs::write(library.path().join("library-coder.gguf"), b"model").unwrap();
    config.ai.local_model_directories = vec![library.path().into()];
    config.ai_doc.local_provider = None;
    config.ai_explain.local_provider = None;
    config.ai_test.local_provider = None;
    config.ai_commit.local_provider = None;
    config.load_local_model_catalog();
    config.apply_ai_host(|| (16 << 30, false)).unwrap();
    let before = config.ai_provider(AiFeature::Explain).unwrap();
    assert_eq!(before.model, "library-coder");

    // :set-option and :toggle update a JSON copy of the active editor config.
    let mut settings = serde_json::to_value(&config).unwrap();
    settings["scrolloff"] = 3.into();
    let mut updated: Config = serde_json::from_value(settings).unwrap();
    updated.load_local_model_catalog();
    updated.apply_ai_host(|| (16 << 30, false)).unwrap();
    assert_eq!(updated.ai_provider(AiFeature::Explain).unwrap(), before);
}

#[test]
fn automatic_routing_replaces_oversized_generation_profile_without_displacing_inline() {
    let (mut config, _weights) = installed_example();
    config.ai.min_local_vram_gib = 0;
    config.ai.default_provider = Some("local".into());
    let big = tempfile::NamedTempFile::new().unwrap();
    big.as_file().set_len(512).unwrap();
    for name in ["local-27b", "local-27b-direct"] {
        config.ai.providers.get_mut(name).unwrap().model_path = Some(big.path().into());
    }
    let library = tempfile::tempdir().unwrap();
    std::fs::write(library.path().join("medium.gguf"), [0; 64]).unwrap();
    config.ai.local_model_directories = vec![library.path().into()];
    config.load_local_model_catalog();
    config.apply_ai_host(|| ((1 << 30) + 128, false)).unwrap();
    assert_eq!(
        config.ai_provider(AiFeature::Completion).unwrap().model,
        "local-coder"
    );
    for feature in MANUAL_FEATURES {
        assert_eq!(
            config.ai_provider(feature).unwrap().model,
            "medium",
            "{feature:?}"
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

#[test]
fn master_switch_preserves_feature_settings_and_skips_hardware_detection() {
    let mut config = mixed();
    config.ai.enabled = false;
    config.apply_ai_hardware(|| panic!("AI is off")).unwrap();
    assert!(config.active_ai_providers().unwrap().is_empty());
    for feature in [
        AiFeature::Completion,
        AiFeature::Doc,
        AiFeature::Commit,
        AiFeature::Merge,
        AiFeature::Test,
    ] {
        assert!(config.ai_provider(feature).is_err());
    }
    assert!(config.ai_completion.enabled);
    assert!(!config.ai_test.enabled);
    config.ai.enabled = true;
    config.apply_ai_hardware(|| 32 << 30).unwrap();
    assert_eq!(
        config.ai_provider(AiFeature::Completion).unwrap().model,
        "local-model"
    );
    Config::default()
        .apply_ai_hardware(|| panic!("no enabled features"))
        .unwrap();
}

#[test]
fn low_or_unknown_dedicated_memory_routes_all_features_to_deepseek() {
    let mut config = mixed();
    config.ai.default_provider = Some("local".into());
    config.ai_test.enabled = true;
    config.ai_explain.enabled = true;
    config.ai_generate.enabled = true;
    config.ai_test.local_provider = Some("local".into());
    config.ai_merge.enabled = true;
    for vram in [0, (8 << 30) - 1] {
        config.apply_ai_hardware(|| vram).unwrap();
        assert_eq!(
            config.active_ai_providers().unwrap(),
            vec![config.ai.providers["cloud"].clone()]
        );
        for feature in [
            AiFeature::Completion,
            AiFeature::Doc,
            AiFeature::Commit,
            AiFeature::Merge,
            AiFeature::Test,
            AiFeature::Explain,
            AiFeature::Generate,
        ] {
            let provider = config.ai_provider(feature).unwrap();
            assert_eq!(provider.model, "cloud-model");
            assert!(!provider.on_demand && provider.model_path.is_none());
        }
    }
    config.apply_ai_hardware(|| 8 << 30).unwrap();
    assert_eq!(
        config.ai_provider(AiFeature::Completion).unwrap().model,
        "local-model"
    );
    assert_eq!(
        config.ai_provider(AiFeature::Commit).unwrap().model,
        "other-model"
    );
}

#[test]
fn managed_model_file_and_runtime_headroom_can_exceed_the_configured_floor() {
    let mut config = mixed();
    let model = tempfile::NamedTempFile::new().unwrap();
    model.as_file().set_len(4096).unwrap();
    config.ai.providers.get_mut("local").unwrap().model_path = Some(model.path().into());
    config.ai.min_local_vram_gib = 1;
    config.apply_ai_hardware(|| (1 << 30) + 4095).unwrap();
    assert_eq!(
        config.ai_provider(AiFeature::Completion).unwrap().kind,
        AiProviderType::Deepseek
    );
    config.apply_ai_hardware(|| (1 << 30) + 4096).unwrap();
    assert_eq!(
        config.ai_provider(AiFeature::Completion).unwrap().kind,
        AiProviderType::LlamaCpp
    );
}

#[test]
fn legacy_local_only_config_uses_authenticated_deepseek_on_small_gpus() {
    let mut config = Config::default();
    config.ai_doc.enabled = true;
    config.apply_ai_hardware(|| 128 << 20).unwrap();
    let provider = config.ai_provider(AiFeature::Doc).unwrap();
    assert_eq!(
        provider.endpoint,
        "https://api.deepseek.com/chat/completions"
    );
    assert_eq!(provider.api_key_env.as_deref(), Some("DEEPSEEK_API_KEY"));
    assert!(provider.model_path.is_none());
}

#[test]
fn feature_thinking_overrides_survive_gpu_routing_without_changing_model_registration() {
    use helix_view::ai::AiThinking;
    let mut config = mixed();
    config
        .ai
        .thinking
        .insert(AiFeature::Completion, AiThinking::High);
    config.ai.thinking.insert(AiFeature::Doc, AiThinking::Off);
    config.ai.providers.get_mut("cloud").unwrap().thinking = true;
    config.apply_ai_hardware(|| 128 << 20).unwrap();
    let completion = config.ai_provider(AiFeature::Completion).unwrap();
    assert_eq!(completion.kind, AiProviderType::Deepseek);
    assert_eq!(completion.reasoning_effort.as_deref(), Some("high"));
    assert!(completion.thinking);
    assert!(!config.ai_provider(AiFeature::Doc).unwrap().thinking);
    config.ai_doc.provider = Some("local".into());
    config.apply_ai_hardware(|| 32 << 30).unwrap();
    let locals = config
        .active_ai_providers()
        .unwrap()
        .into_iter()
        .filter(|p| p.model_path.is_some())
        .count();
    assert_eq!(locals, 1);
    assert_eq!(
        config
            .ai_provider(AiFeature::Completion)
            .unwrap()
            .thinking_budget,
        Some(2048)
    );
}

#[test]
fn local_only_inline_completion_keeps_cloud_features_and_never_falls_back_online() {
    let mut config = mixed();
    config.ai_completion.local_only = true;
    config.apply_ai_hardware(|| 128 << 20).unwrap();
    assert_eq!(
        config.ai.statusline(),
        format!("AI 开 · 补全 本地 / {}", config.ai.providers["local"].model)
    );
    assert_eq!(
        config.ai_provider(AiFeature::Completion).unwrap().kind,
        AiProviderType::LlamaCpp
    );
    for feature in [
        AiFeature::Doc,
        AiFeature::Commit,
        AiFeature::Merge,
        AiFeature::Test,
    ] {
        assert_eq!(
            config.ai_provider(feature).unwrap().kind,
            AiProviderType::Deepseek
        );
    }
    config.ai_completion.provider = Some("cloud".into());
    assert!(config.ai_provider(AiFeature::Completion).is_err());
    config.ai.enabled = false;
    assert!(config.active_ai_providers().unwrap().is_empty());
}

#[test]
fn interactive_generation_uses_large_local_or_deepseek_even_when_inline_fits() {
    let mut config = mixed();
    config.ai.auto_local_provider = Some("local".into());
    config.ai_completion.provider = None;
    let inline_weights = tempfile::NamedTempFile::new().unwrap();
    let large_weights = tempfile::NamedTempFile::new().unwrap();
    let inline = config.ai.providers.get_mut("local").unwrap();
    inline.model = "inline-3b".into();
    inline.model_path = Some(inline_weights.path().into());
    inline.server_path = std::env::current_exe().unwrap();
    let mut large = inline.clone();
    large.model = "generation-27b".into();
    large.model_path = Some(large_weights.path().into());
    large.endpoint = "http://127.0.0.1:19300/v1/chat/completions".into();
    config.ai.providers.insert("large".into(), large);
    config.ai_doc.local_provider = Some("large".into());
    config.ai_explain.enabled = true;
    config.ai_generate.enabled = true;
    config.apply_ai_hardware(|| 24 << 30).unwrap();
    for feature in [AiFeature::Explain, AiFeature::Generate] {
        let model = config.ai_provider(feature).unwrap();
        assert_eq!(model.model, "generation-27b");
        assert!(model.on_demand);
    }
    assert_eq!(
        config.ai_provider(AiFeature::Completion).unwrap().model,
        "inline-3b"
    );
    drop(large_weights);
    for feature in [AiFeature::Explain, AiFeature::Generate] {
        assert_eq!(config.ai_provider(feature).unwrap().model, "cloud-model");
    }
    config.ai.auto_local_provider = None;
    config.ai.default_provider = Some("local".into());
    assert_eq!(
        config.ai_provider(AiFeature::Generate).unwrap().model,
        "cloud-model"
    );
    config.ai_doc.local_provider = None;
    for feature in [AiFeature::Explain, AiFeature::Generate] {
        assert_eq!(config.ai_provider(feature).unwrap().model, "cloud-model");
    }
    for feature in [AiFeature::Explain, AiFeature::Generate] {
        assert_eq!(
            Config::default().ai_provider(feature).unwrap().kind,
            AiProviderType::Deepseek
        );
    }
    config.ai.providers.get_mut("other").unwrap().endpoint =
        "http://localhost:8080/v1/chat/completions".into();
    config.ai.default_provider = Some("other".into());
    assert_eq!(
        config.ai_provider(AiFeature::Explain).unwrap().model,
        "cloud-model"
    );
    config.ai_explain.provider = Some("other".into());
    assert_eq!(
        config.ai_provider(AiFeature::Explain).unwrap().model,
        "other-model"
    );
}

#[test]
fn automatic_routing_uses_installed_models_that_fit_the_current_gpu() {
    let mut config = mixed();
    config.ai_completion.provider = None;
    config.ai.auto_local_provider = Some("local".into());
    let weights = tempfile::NamedTempFile::new().unwrap();
    let local = config.ai.providers.get_mut("local").unwrap();
    local.server_path = std::env::current_exe().unwrap();
    local.model_path = Some(weights.path().into());
    config.apply_ai_hardware(|| 24 << 30).unwrap();
    assert_eq!(
        config.ai_provider(AiFeature::Completion).unwrap().model,
        "local-model"
    );
    assert_eq!(
        config.ai_provider(AiFeature::Doc).unwrap().model,
        "local-model"
    );
    config.apply_ai_hardware(|| 128 << 20).unwrap();
    assert!(config.ai_provider(AiFeature::Completion).is_err());
    assert_eq!(
        config.ai_provider(AiFeature::Doc).unwrap().model,
        "cloud-model"
    );
    config.apply_ai_hardware(|| 24 << 30).unwrap();
    drop(weights);
    assert!(config.ai_provider(AiFeature::Completion).is_err());
    config.apply_ai_hardware(|| 24 << 30).unwrap();
    assert!(config.ai_provider(AiFeature::Completion).is_err());
    assert_eq!(
        config.ai_provider(AiFeature::Doc).unwrap().model,
        "cloud-model"
    );
}

#[test]
fn deepseek_feature_effort_follows_the_route_and_yields_to_the_panel() {
    use helix_view::ai::AiThinking;
    let mut config = mixed();
    config.ai_commit.provider = Some("local".into());
    config
        .ai
        .deepseek_thinking
        .insert(AiFeature::Commit, AiThinking::Low);
    config.apply_ai_hardware(|| 128 << 20).unwrap();
    let cloud = config.ai_provider(AiFeature::Commit).unwrap();
    assert_eq!(cloud.kind, AiProviderType::Deepseek);
    assert!(cloud.thinking);
    assert_eq!(cloud.reasoning_effort.as_deref(), Some("low"));
    assert!(!config.ai_provider(AiFeature::Doc).unwrap().thinking);
    config.apply_ai_hardware(|| 24 << 30).unwrap();
    let local = config.ai_provider(AiFeature::Commit).unwrap();
    assert_eq!(local.kind, AiProviderType::LlamaCpp);
    assert!(!local.thinking);
    config.apply_ai_hardware(|| 128 << 20).unwrap();
    config
        .ai
        .thinking
        .insert(AiFeature::Commit, AiThinking::Off);
    assert!(!config.ai_provider(AiFeature::Commit).unwrap().thinking);
}
