//! A conversation retains its original buffer and scope across follow-ups.
use std::{ops::Range, path::PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Auto,
    Selection,
    Function,
    File,
    Project,
}

impl Scope {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "auto" => Self::Auto,
            "selection" => Self::Selection,
            "function" => Self::Function,
            "file" => Self::File,
            "project" => Self::Project,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Suggestion {
    pub title: String,
    pub reason: String,
    pub line: usize,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Debug)]
pub struct Session {
    pub optimize: bool,
    pub scope: Scope,
    pub doc: crate::DocumentId,
    pub version: i32,
    pub range: Option<Range<usize>>,
    pub first_line: usize,
    pub path: Option<PathBuf>,
    pub project_root: Option<PathBuf>,
    pub origin_dir: PathBuf,
    pub explore_root: Option<PathBuf>,
    pub agent_progress: String,
    pub agent_trace: Vec<String>,
    /// Endpoint and model which explicitly rejected tool requests in this conversation.
    pub plain_model: Option<(String, String)>,
    pub context: String,
    pub suggestions: Vec<Suggestion>,
    pub locations: Vec<(PathBuf, usize)>,
    pub label: String,
    pub source: String,
    pub language: String,
    pub turns: Vec<(String, String)>,
    pub pending: Option<String>,
    pub draft: String,
    pub notice: String,
    pub revision: u64,
    pub controller: helix_event::TaskController,
}
