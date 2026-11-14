use helix_view::editor::Config;

#[test]
fn ai_doc_is_independent_and_defaults_to_chinese_rest() {
    let parsed = toml::from_str::<Config>("[ai-doc]\nenabled = true\n");
    assert!(
        parsed.is_ok(),
        "ai-doc configuration must be supported: {parsed:?}"
    );
    let config = parsed.unwrap();
    assert!(!config.ai_completion.enabled);
    assert!(config.ai_model_config().enabled);
    assert!(!Config::default().ai_model_config().enabled);
    let value = toml::Value::try_from(&config).unwrap();
    assert_eq!(value["ai-doc"]["language"].as_str(), Some("zh-CN"));
    assert_eq!(value["ai-doc"]["style"].as_str(), Some("rest"));
    assert_eq!(value["ai-doc"]["timeout-ms"].as_integer(), Some(30000));
    assert_eq!(value["ai-doc"]["max-tokens"].as_integer(), Some(768));
    let defaults = toml::Value::try_from(Config::default()).unwrap();
    assert_eq!(defaults["ai-doc"]["enabled"].as_bool(), Some(false));
    assert!(toml::from_str::<Config>("[ai-doc]\nstyle = 'doxygen'").is_err());
}
