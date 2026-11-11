//! Configuration and view-scoped, non-editing AI completion state.

use helix_core::text_annotations::InlineAnnotation;
use serde::{Deserialize, Serialize};

use crate::{Document, ViewId};

/// Insert-mode carets in Helix may select the adjacent grapheme internally.
/// Treat that native representation like a point, but not a larger selection.
pub fn completion_cursor(doc: &Document, view: ViewId) -> Option<usize> {
    let selection = doc.selection(view);
    if selection.len() != 1 {
        return None;
    }
    let range = selection.primary();
    let text = doc.text().slice(..);
    let cursor = range.cursor(text);
    (range.is_empty()
        || (range.from() == cursor
            && range.to() == helix_core::graphemes::next_grapheme_boundary(text, cursor)))
    .then_some(cursor)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct AiCompletionConfig {
    pub enabled: bool,
    pub endpoint: String,
    pub model: String,
    pub api_key_env: Option<String>,
    pub debounce_ms: u64,
    pub timeout_ms: u64,
    pub max_tokens: u32,
    pub prefix_chars: usize,
    pub suffix_chars: usize,
}

impl Default for AiCompletionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: "http://127.0.0.1:19239/v1/chat/completions".into(),
            model: "Qwen3.8-27B-UD-Q5_K_M".into(),
            api_key_env: None,
            debounce_ms: 350,
            timeout_ms: 5000,
            max_tokens: 128,
            prefix_chars: 8000,
            suffix_chars: 2000,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AiCompletion {
    pub view: ViewId,
    pub version: i32,
    pub annotation: InlineAnnotation,
}

impl AiCompletion {
    pub fn is_valid(&self, doc: &Document, view: ViewId) -> bool {
        if self.view != view || self.version != doc.version() || doc.readonly {
            return false;
        }
        completion_cursor(doc, view) == Some(self.annotation.char_idx)
            && doc.active_snippet.is_none()
    }
}
