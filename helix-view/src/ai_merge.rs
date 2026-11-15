//! Explicit, target-bound AI merge assistance. Suggestions never write Git state.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_merge_config_is_independent_and_enables_shared_provider() {
        let mut config = crate::editor::Config::default();
        assert!(!config.ai_merge.enabled);
        config.ai_merge = toml::from_str("enabled = true\ntimeout-ms = 45000").unwrap();
        assert!(!config.ai_completion.enabled);
        assert!(config.ai_model_config().enabled);
        assert_eq!(config.ai_merge.timeout_ms, 45000);
        assert_eq!(config.ai_completion.timeout_ms, 5000);
        assert_eq!(config.ai_merge.max_tokens, 1536);
        assert_eq!(config.ai_merge.max_input_bytes, 16384);
        assert!(toml::from_str::<AiMergeConfig>("max-token = 10").is_err());
    }
}
use serde::{Deserialize, Serialize};
use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct AiMergeConfig {
    pub enabled: bool,
    pub timeout_ms: u64,
    pub max_tokens: u32,
    pub max_input_bytes: usize,
}

impl Default for AiMergeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            timeout_ms: 60_000,
            max_tokens: 1536,
            max_input_bytes: 16_384,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Target {
    pub block: usize,
    pub path: String,
    pub language: String,
    pub base: String,
    pub current: String,
    pub incoming: String,
    pub result: String,
    pub context: Vec<String>,
    pub intent: String,
    pub range: Range<usize>,
}

#[derive(Clone, Debug)]
pub struct Suggestion {
    pub code: Option<String>,
    pub explanation: String,
}
