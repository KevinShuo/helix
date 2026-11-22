//! Configuration and view-scoped, non-editing AI completion state.

use helix_core::text_annotations::InlineAnnotation;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
    pub provider: Option<String>,
    pub enabled: bool,
    /// Run inline requests only through a managed llama.cpp model on loopback.
    pub local_only: bool,
    /// Computed from this machine's available models.
    #[serde(skip)]
    pub unavailable: Option<String>,
    pub conservative: bool,
    /// Minimum geometric mean token probability (percent) for local gap completions.
    /// Zero disables the filter.
    pub min_gap_confidence: u8,
    pub predict_next: bool,
    pub prediction_lines: u32,
    pub prediction_tokens: u32,
    pub endpoint: String,
    pub model: String,
    pub model_path: Option<PathBuf>,
    pub server_path: PathBuf,
    pub context_size: u32,
    pub gpu_layers: u32,
    pub startup_timeout_ms: u64,
    pub prompt_mode: AiCompletionPromptMode,
    pub api_key_env: Option<String>,
    pub debounce_ms: u64,
    /// Optional shorter delay for the first edit after a typing pause.
    pub idle_debounce_ms: Option<u64>,
    pub timeout_ms: u64,
    pub max_tokens: u32,
    pub prefix_chars: usize,
    pub suffix_chars: usize,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AiCompletionPromptMode {
    #[default]
    Insertion,
    Line,
    Gap,
}

impl Default for AiCompletionConfig {
    fn default() -> Self {
        Self {
            provider: None,
            enabled: false,
            local_only: false,
            unavailable: None,
            conservative: false,
            min_gap_confidence: 0,
            predict_next: false,
            prediction_lines: 6,
            prediction_tokens: 192,
            endpoint: "http://127.0.0.1:19239/v1/chat/completions".into(),
            model: "Qwen3.8-27B-UD-Q5_K_M".into(),
            model_path: None,
            server_path: "llama-server".into(),
            context_size: 4096,
            gpu_layers: 99,
            startup_timeout_ms: 120_000,
            prompt_mode: AiCompletionPromptMode::Insertion,
            api_key_env: None,
            debounce_ms: 350,
            idle_debounce_ms: None,
            timeout_ms: 5000,
            max_tokens: 128,
            prefix_chars: 8000,
            suffix_chars: 2000,
        }
    }
}

impl AiCompletionConfig {
    pub fn debounce_after(&self, since_edit: Option<std::time::Duration>) -> std::time::Duration {
        use std::time::Duration;
        let typing = self.debounce_ms.min(5000);
        let pause = Duration::from_millis((3 * typing).max(500));
        let delay = if since_edit.is_none_or(|gap| gap >= pause) {
            self.idle_debounce_ms.unwrap_or(typing).min(typing)
        } else {
            typing
        };
        Duration::from_millis(delay)
    }
}

#[derive(Debug, Clone)]
pub struct AiCompletion {
    pub view: ViewId,
    pub version: i32,
    pub annotation: InlineAnnotation,
    /// From triggering the request to publishing the validated suggestion.
    /// A remaining suffix of an already visible suggestion requires no request.
    pub request_latency: Option<std::time::Duration>,
}

/// The one edit an in-flight local request has already included in its prompt.
#[derive(Debug)]
pub struct AnticipatedEdit {
    pub view: ViewId,
    pub version: i32,
    pub cursor: usize,
    pub insertion: String,
}

impl AnticipatedEdit {
    pub fn consume(&mut self, changes: &helix_core::ChangeSet, view: ViewId, version: i32) -> bool {
        let mut edits = changes.changes_iter();
        let Some((from, to, Some(text))) = edits.next() else {
            return false;
        };
        if self.view != view
            || self.version.checked_add(1) != Some(version)
            || from != self.cursor
            || from != to
            || edits.next().is_some()
            || text.is_empty()
            || !self.insertion.starts_with(text.as_str())
        {
            return false;
        }
        self.cursor += text.chars().count();
        self.version = version;
        self.insertion.drain(..text.len());
        true
    }

    pub fn is_valid(&self, doc: &Document, view: ViewId) -> bool {
        self.view == view
            && self.version == doc.version()
            && completion_cursor(doc, view) == Some(self.cursor)
            && !doc.readonly
            && doc.active_snippet.is_none()
    }
}

/// A validated local result for revisiting the exact same document and caret.
pub struct CachedAiCompletion {
    pub source: helix_core::Rope,
    pub cursor: usize,
    pub path: Option<PathBuf>,
    pub language: Option<String>,
    pub indent: &'static str,
    pub text: String,
}

impl CachedAiCompletion {
    pub fn matches(&self, doc: &Document, cursor: usize) -> bool {
        self.cursor == cursor
            && self.path.as_deref() == doc.path()
            && self.language.as_deref() == doc.language_name()
            && self.indent == doc.indent_style.as_str()
            && self.source == *doc.text()
    }
}

#[cfg(test)]
#[test]
fn anticipated_edit_requires_contiguous_insertions_at_the_expected_cursor() {
    use helix_core::{Rope, Transaction};
    let view = ViewId::default();
    let mut pending = AnticipatedEdit {
        view,
        version: 4,
        cursor: 1,
        insertion: ":\r\n    ".into(),
    };
    let source = Rope::from("x");
    let typed = |at, text: &'static str| {
        Transaction::change(&source, [(at, at, Some(text.into()))].into_iter())
    };
    assert!(!pending.consume(typed(0, ":").changes(), view, 5));
    assert!(!pending.consume(typed(1, "z").changes(), view, 5));
    assert!(!pending.consume(typed(1, ":").changes(), view, 6));
    assert!(pending.consume(typed(1, ":").changes(), view, 5));
    let source = Rope::from("x:");
    let enter = Transaction::change(&source, [(2, 2, Some("\r\n    ".into()))].into_iter());
    assert!(pending.consume(enter.changes(), view, 6));
    assert!(pending.insertion.is_empty());
    assert_eq!(pending.cursor, 8);
}

#[cfg(test)]
#[test]
fn brief_first_delay_preserves_the_typing_debounce() {
    use std::time::Duration;
    let mut config = AiCompletionConfig {
        debounce_ms: 120,
        ..Default::default()
    };
    assert_eq!(config.debounce_after(None).as_millis(), 120);
    config.idle_debounce_ms = Some(40);
    assert_eq!(config.debounce_after(None).as_millis(), 40);
    assert_eq!(
        config
            .debounce_after(Some(Duration::from_millis(95)))
            .as_millis(),
        120
    );
    assert_eq!(
        config
            .debounce_after(Some(Duration::from_millis(500)))
            .as_millis(),
        40
    );
    config.idle_debounce_ms = Some(200);
    assert_eq!(config.debounce_after(None).as_millis(), 120);
}

impl AiCompletion {
    pub fn after_insertion(
        &self,
        changes: &helix_core::ChangeSet,
        view: ViewId,
        version: i32,
    ) -> Option<Self> {
        if self.view != view || self.version.checked_add(1) != Some(version) {
            return None;
        }
        let mut edits = changes.changes_iter();
        let (from, to, Some(inserted)) = edits.next()? else {
            return None;
        };
        if edits.next().is_some() || from != to || from != self.annotation.char_idx {
            return None;
        }
        let remaining = self.annotation.text.strip_prefix(inserted.as_str())?;
        if remaining.is_empty() {
            return None;
        }
        Some(Self {
            view,
            version,
            annotation: InlineAnnotation::new(from + inserted.chars().count(), remaining),
            request_latency: None,
        })
    }

    pub fn is_valid(&self, doc: &Document, view: ViewId) -> bool {
        if self.view != view || self.version != doc.version() || doc.readonly {
            return false;
        }
        completion_cursor(doc, view) == Some(self.annotation.char_idx)
            && doc.active_snippet.is_none()
    }
}
