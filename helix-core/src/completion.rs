use std::borrow::Cow;

use crate::{diagnostic::LanguageServerId, Transaction};

#[derive(Debug, PartialEq, Clone)]
pub struct CompletionItem {
    pub transaction: Transaction,
    pub label: Cow<'static, str>,
    pub kind: Cow<'static, str>,
    /// Containing Markdown
    pub documentation: Option<String>,
    pub provider: CompletionProvider,
    pub ignore_rule: Option<IgnoreRuleCompletion>,
}

#[derive(Debug, PartialEq, Clone)]
pub struct IgnoreRuleCompletion {
    pub text: String,
    /// Lower values prefer candidates with a known effect.
    pub rank: u8,
    /// Standalone rule fragment, retaining directory syntax for dot-repeat.
    pub repeat: String,
}

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub enum CompletionProvider {
    Lsp(LanguageServerId),
    Path,
    Word,
    Gitignore,
}

impl From<LanguageServerId> for CompletionProvider {
    fn from(id: LanguageServerId) -> Self {
        CompletionProvider::Lsp(id)
    }
}
