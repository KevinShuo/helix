use crate::keymap;
use crate::keymap::{merge_keys, KeyTrie};
use helix_loader::merge_toml_values;
use helix_view::{document::Mode, theme};
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt::Display;
use std::fs;
use std::io::Error as IOError;
use toml::de::Error as TomlError;

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub theme: Option<theme::Config>,
    pub keys: HashMap<Mode, KeyTrie>,
    pub editor: helix_view::editor::Config,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigRaw {
    pub theme: Option<theme::Config>,
    pub keys: Option<HashMap<Mode, KeyTrie>>,
    pub editor: Option<toml::Value>,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            theme: None,
            keys: keymap::default(),
            editor: helix_view::editor::Config::default(),
        }
    }
}

#[derive(Debug)]
pub enum ConfigLoadError {
    BadConfig(TomlError),
    InvalidAiConfig(String),
    Error(IOError),
}

impl Default for ConfigLoadError {
    fn default() -> Self {
        ConfigLoadError::Error(IOError::new(std::io::ErrorKind::NotFound, "place holder"))
    }
}

impl Display for ConfigLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigLoadError::BadConfig(err) => err.fmt(f),
            ConfigLoadError::InvalidAiConfig(err) => err.fmt(f),
            ConfigLoadError::Error(err) => err.fmt(f),
        }
    }
}

impl Config {
    pub fn load(
        global: Result<&String, ConfigLoadError>,
        local: Result<String, ConfigLoadError>,
    ) -> Result<Config, ConfigLoadError> {
        let config = Self::parse(global, local)?;
        config
            .editor
            .validate_ai()
            .map_err(|err| ConfigLoadError::InvalidAiConfig(format!("{err:#}")))?;
        Ok(config)
    }

    fn parse(
        global: Result<&String, ConfigLoadError>,
        local: Result<String, ConfigLoadError>,
    ) -> Result<Config, ConfigLoadError> {
        let global_config: Result<ConfigRaw, ConfigLoadError> =
            global.and_then(|file| toml::from_str(file).map_err(ConfigLoadError::BadConfig));
        let local_config: Result<ConfigRaw, ConfigLoadError> =
            local.and_then(|file| toml::from_str(&file).map_err(ConfigLoadError::BadConfig));
        let res = match (global_config, local_config) {
            (Ok(global), Ok(local)) => {
                let mut keys = keymap::default();
                if let Some(global_keys) = global.keys {
                    merge_keys(&mut keys, global_keys)
                }
                if let Some(local_keys) = local.keys {
                    merge_keys(&mut keys, local_keys)
                }

                let editor = match (global.editor, local.editor) {
                    (None, None) => helix_view::editor::Config::default(),
                    (None, Some(val)) | (Some(val), None) => {
                        val.try_into().map_err(ConfigLoadError::BadConfig)?
                    }
                    (Some(global), Some(local)) => {
                        // Named providers add one level below the usual editor settings.
                        let ai = global
                            .get("ai")
                            .zip(local.get("ai"))
                            .map(|(global, local)| {
                                merge_toml_values(global.clone(), local.clone(), 3)
                            });
                        let mut merged = merge_toml_values(global, local, 3);
                        if let Some(ai) = ai {
                            merged.as_table_mut().unwrap().insert("ai".into(), ai);
                        }
                        merged.try_into().map_err(ConfigLoadError::BadConfig)?
                    }
                };

                Config {
                    theme: local.theme.or(global.theme),
                    keys,
                    editor,
                }
            }
            // if any configs are invalid return that first
            (_, Err(ConfigLoadError::BadConfig(err)))
            | (Err(ConfigLoadError::BadConfig(err)), _) => {
                return Err(ConfigLoadError::BadConfig(err))
            }
            (Ok(config), Err(_)) | (Err(_), Ok(config)) => {
                let mut keys = keymap::default();
                if let Some(keymap) = config.keys {
                    merge_keys(&mut keys, keymap);
                }
                Config {
                    theme: config.theme,
                    keys,
                    editor: config.editor.map_or_else(
                        || Ok(helix_view::editor::Config::default()),
                        |val| val.try_into().map_err(ConfigLoadError::BadConfig),
                    )?,
                }
            }

            // these are just two io errors return the one for the global config
            (Err(err), Err(_)) => return Err(err),
        };

        Ok(res)
    }

    pub fn load_default() -> Result<Config, ConfigLoadError> {
        let global_config =
            fs::read_to_string(helix_loader::config_file()).map_err(ConfigLoadError::Error)?;
        let local_config = fs::read_to_string(helix_loader::workspace_config_file())
            .map_err(ConfigLoadError::Error);

        let phony_config = ConfigLoadError::Error(IOError::other("hacky placeholder"));
        let global_parsed = Config::parse(Ok(&global_config), Err(phony_config))?;

        // We need to build a transient `WorkspaceTrust` just to ask whether the workspace is
        // trusted enough to load its `.helix/config.toml`. The persisted-trust file on disk is the
        // source of truth either way; this transient instance has an empty cache and is dropped
        // after the check.
        let trust = helix_loader::workspace_trust::WorkspaceTrust::new(
            (&global_parsed.editor.workspace_trust).into(),
        );
        let mut config = if trust
            .query_current(helix_loader::workspace_trust::TrustQuery::LocalConfig)
            .is_trusted()
        {
            let mut merged = Config::parse(Ok(&global_config), local_config)?;
            // editor.workspace-trust is global/user-scope only. Without this override, a
            // workspace's `.helix/config.toml` could set `level = "insecure"`; once the user trusted
            // *that* workspace, refresh_config would re-load with the override merged in and from
            // then on every subsequent workspace in the session would be implicitly trusted. Pin
            // the gate's own configuration to the global file.
            merged.editor.workspace_trust = global_parsed.editor.workspace_trust;
            // A workspace may disable AI but cannot undo the user's global off switch.
            merged.editor.ai.enabled &= global_parsed.editor.ai.enabled;
            merged
        } else {
            global_parsed
        };
        crate::ai_hardware::configure(&mut config.editor)
            .map_err(|err| ConfigLoadError::InvalidAiConfig(format!("{err:#}")))?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_provider_project_override_preserves_connection_and_other_providers() {
        let global = r#"
[editor.ai]
default-provider = "cloud"
[editor.ai.providers.cloud]
type = "deepseek"
endpoint = "https://api.deepseek.com/chat/completions"
model = "default-model"
api-key-env = "DEEPSEEK_API_KEY"
[editor.ai.providers.local]
type = "llama-cpp"
endpoint = "http://127.0.0.1:19239/v1/chat/completions"
model = "local"
[editor.ai-doc]
enabled = true
"#
        .to_owned();
        let local = "[editor.ai.providers.cloud]\nmodel = 'project-model'\n[editor.ai-completion]\nprovider = 'local'\n".to_owned();
        let config =
            Config::load(Ok(&global), Ok(local)).expect("provider field overrides must merge");
        let provider = config
            .editor
            .ai_provider(helix_view::ai::AiFeature::Doc)
            .unwrap();
        assert_eq!(provider.model, "project-model");
        assert_eq!(
            provider.endpoint,
            "https://api.deepseek.com/chat/completions"
        );
        assert_eq!(provider.api_key_env.as_deref(), Some("DEEPSEEK_API_KEY"));
        assert_eq!(
            config
                .editor
                .ai_provider(helix_view::ai::AiFeature::Completion)
                .unwrap()
                .model,
            "local"
        );
    }

    #[test]
    fn ai_provider_bad_reference_is_a_config_error() {
        let config = "[editor.ai-doc]\nprovider = 'typo'\n".to_owned();
        let error = Config::load(Ok(&config), Err(ConfigLoadError::default())).unwrap_err();
        assert!(error.to_string().contains("typo"));
    }

    impl Config {
        fn load_test(config: &str) -> Config {
            Config::load(Ok(&config.to_owned()), Err(ConfigLoadError::default())).unwrap()
        }
    }

    #[test]
    fn parsing_keymaps_config_file() {
        use crate::keymap;
        use helix_core::hashmap;
        use helix_view::document::Mode;

        let sample_keymaps = r#"
            [keys.insert]
            y = "move_line_down"
            S-C-a = "delete_selection"

            [keys.normal]
            A-F12 = "move_next_word_end"
        "#;

        let mut keys = keymap::default();
        merge_keys(
            &mut keys,
            hashmap! {
                Mode::Insert => keymap!({ "Insert mode"
                    "y" => move_line_down,
                    "S-C-a" => delete_selection,
                }),
                Mode::Normal => keymap!({ "Normal mode"
                    "A-F12" => move_next_word_end,
                }),
            },
        );

        assert_eq!(
            Config::load_test(sample_keymaps),
            Config {
                keys,
                ..Default::default()
            }
        );
    }

    #[test]
    fn keys_resolve_to_correct_defaults() {
        // From serde default
        let default_keys = Config::load_test("").keys;
        assert_eq!(default_keys, keymap::default());

        // From the Default trait
        let default_keys = Config::default().keys;
        assert_eq!(default_keys, keymap::default());
    }

    #[test]
    fn minimap_config_merges_project_fields_and_rejects_invalid_values() {
        let global = "[editor.minimap]\nenabled=false\nwidth=20\ndiagnostics=false\n".to_owned();
        let local = "[editor.minimap]\nenabled=true\nrender='blocks'\n".to_owned();
        let merged = Config::load(Ok(&global), Ok(local)).unwrap();
        assert!(merged.editor.minimap.enabled);
        assert_eq!(merged.editor.minimap.width, 20);
        assert!(!merged.editor.minimap.diagnostics);
        assert_eq!(
            merged.editor.minimap.render,
            helix_view::minimap::MinimapRender::Blocks
        );
        let bad = "[editor.minimap]\nwidth=0\n".to_owned();
        assert!(Config::load(Ok(&bad), Err(ConfigLoadError::default())).is_err());
    }
}
