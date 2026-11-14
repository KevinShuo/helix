//! Configuration for explicit AI Git commit assistance.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct AiCommitConfig {
    pub enabled: bool,
    pub language: CommitLanguage,
    pub timeout_ms: u64,
    pub max_tokens: u32,
    pub max_input_bytes: usize,
    pub max_files: usize,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommitLanguage {
    #[default]
    #[serde(rename = "zh-CN")]
    Chinese,
    #[serde(rename = "en")]
    English,
}

impl Default for AiCommitConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            language: CommitLanguage::Chinese,
            timeout_ms: 30_000,
            max_tokens: 768,
            max_input_bytes: 4096,
            max_files: 64,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_commit_config_defaults_are_independent_and_serialize() {
        let config: AiCommitConfig = toml::from_str("").unwrap();
        assert!(!config.enabled);
        assert_eq!(config.language, CommitLanguage::Chinese);
        assert_eq!((config.timeout_ms, config.max_tokens), (30_000, 768));
        assert_eq!((config.max_input_bytes, config.max_files), (4096, 64));
        let english: AiCommitConfig = toml::from_str("enabled = true\nlanguage = 'en'").unwrap();
        assert!(english.enabled);
        assert_eq!(english.language, CommitLanguage::English);
        assert_eq!(
            toml::from_str::<AiCommitConfig>(&toml::to_string(&english).unwrap()).unwrap(),
            english
        );
        assert!(toml::from_str::<AiCommitConfig>("language = 'fr'").is_err());
        assert!(toml::from_str::<AiCommitConfig>("max-input-byte = 12").is_err());
    }
}
