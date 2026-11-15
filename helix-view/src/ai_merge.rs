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

#[derive(Clone)]
pub struct Operation {
    pub id: u64,
    pub doc: crate::DocumentId,
    pub view: crate::ViewId,
    pub version: i32,
    pub session: crate::merge::MergeSessionId,
    pub source_version: helix_vcs::git_tools::ConflictVersion,
    pub model: crate::ai_completion::AiCompletionConfig,
    pub config: AiMergeConfig,
    pub target: Target,
    pub handle: helix_event::TaskHandle,
    pub suggestion: Option<Suggestion>,
    pub preview: bool,
}

impl Operation {
    pub fn is_valid(&self, editor: &crate::Editor) -> bool {
        if self.handle.is_canceled()
            || editor.active_merge() != Some(self.session)
            || editor.tree.focus != self.view
            || editor
                .tree
                .try_get(self.view)
                .is_none_or(|v| v.doc != self.doc)
            || editor.config().ai_model_config() != self.model
            || editor.config().ai_merge != self.config
        {
            return false;
        }
        let Some(doc) = editor.document(self.doc) else {
            return false;
        };
        let Some(state) = doc.merge() else {
            return false;
        };
        !doc.readonly
            && doc.version() == self.version
            && doc.language_name().unwrap_or("") == self.target.language
            && doc.active_snippet.is_none()
            && !state.busy
            && matches!(state.write_status, crate::merge::MergeWriteStatus::Draft)
            && state.data.version == self.source_version
            && state.selected_block == self.target.block
            && state.model.mode() == helix_vcs::merge::MergeMode::Blockwise
            && state
                .model
                .blocks()
                .get(self.target.block)
                .is_some_and(|b| b.result == self.target.range)
            && self.target.range.end <= doc.text().len_chars()
            && doc.text().slice(self.target.range.clone()) == self.target.result
    }
}

impl crate::Editor {
    pub fn cancel_ai_merge(&mut self) {
        if let Some(op) = self.ai_merge.take() {
            if let Some(doc) = self.document_mut(op.doc) {
                doc.ai_merge_controller.cancel();
            }
        }
    }
}
