//! Configuration for explicit AI function documentation.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct AiDocConfig {
    pub enabled: bool,
    pub language: DocLanguage,
    pub style: DocStyle,
    pub timeout_ms: u64,
    pub max_tokens: u32,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocLanguage {
    #[default]
    #[serde(rename = "zh-CN")]
    Chinese,
    #[serde(rename = "en")]
    English,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DocStyle {
    #[default]
    Rest,
}

impl Default for AiDocConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            language: DocLanguage::Chinese,
            style: DocStyle::Rest,
            timeout_ms: 30_000,
            max_tokens: 768,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Parameter {
    pub name: String,
    pub ty: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Target {
    pub language: String,
    pub name: String,
    pub source: String,
    pub supporting: String,
    pub range: std::ops::Range<usize>,
    pub insert: usize,
    pub indent: String,
    pub newline: String,
    pub parameters: Vec<Parameter>,
    pub templates: Vec<String>,
    pub return_type: Option<String>,
    pub returns: bool,
    pub exceptions: Vec<String>,
    pub line: usize,
}

#[derive(Clone)]
pub struct Operation {
    pub id: u64,
    pub doc: crate::DocumentId,
    pub view: crate::ViewId,
    pub version: i32,
    pub model: crate::ai_completion::AiCompletionConfig,
    pub config: AiDocConfig,
    pub target: Target,
    pub handle: helix_event::TaskHandle,
    pub text: Option<String>,
    pub preview: bool,
}

impl Operation {
    pub fn is_valid(&self, editor: &crate::Editor) -> bool {
        if self.handle.is_canceled()
            || editor.config().ai_model_config() != self.model
            || editor.config().ai_doc != self.config
            || editor.tree.focus != self.view
        {
            return false;
        }
        let Some(doc) = editor.document(self.doc) else {
            return false;
        };
        !doc.readonly
            && doc.version() == self.version
            && doc.language_name() == Some(self.target.language.as_str())
            && doc.active_snippet.is_none()
            && doc.selection(self.view).len() == 1
            && editor.tree.get(self.view).doc == self.doc
            && self.target.range.end <= doc.text().len_chars()
            && doc.text().slice(self.target.range.clone()).to_string() == self.target.source
    }
}
