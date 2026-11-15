//! Shared navigation state for live diagnostics and compiler problems.
use helix_core::diagnostic::Severity;
use helix_lsp::{lsp, OffsetEncoding};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProblemSource {
    Lsp,
    Build,
}

impl ProblemSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Lsp => "LSP",
            Self::Build => "BUILD",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub path: PathBuf,
    pub range: lsp::Range,
    pub encoding: OffsetEncoding,
    pub severity: Severity,
    pub source: ProblemSource,
    /// Provider identity, kept separate from the human-readable message.
    pub origin: String,
    pub message: String,
}

impl Problem {
    pub fn sort_key(&self) -> (&std::path::Path, u32, u32, ProblemSource, &str, &str) {
        (
            &self.path,
            self.range.start.line,
            self.range.start.character,
            self.source,
            &self.origin,
            &self.message,
        )
    }

    pub fn same_identity(&self, other: &Self) -> bool {
        self.path == other.path
            && self.source == other.source
            && self.origin == other.origin
            && self.message == other.message
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProblemFilter {
    pub source: Option<ProblemSource>,
    pub minimum_severity: Option<Severity>,
    pub document: bool,
}

impl Default for ProblemFilter {
    fn default() -> Self {
        Self {
            source: None,
            minimum_severity: Some(Severity::Warning),
            document: false,
        }
    }
}

#[derive(Default)]
pub struct ProblemState {
    pub filter: ProblemFilter,
    pub selected: Option<Problem>,
    /// Actual cursor after the jump, to distinguish navigation from manual movement.
    pub anchor: Option<(PathBuf, usize)>,
    pub compilation: CompilationState,
}

pub const BUILD_LOG_LIMIT: usize = 1_048_576;
pub const BUILD_LINE_LIMIT: usize = 65_536;
pub const BUILD_PROBLEM_LIMIT: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStamp {
    pub size: u64,
    pub modified: Option<std::time::SystemTime>,
}

impl FileStamp {
    pub fn read(path: &std::path::Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        Some(Self {
            size: meta.len(),
            modified: meta.modified().ok(),
        })
    }
}

pub struct BuildRun {
    pub id: u64,
    pub command: String,
    pub cwd: PathBuf,
    pub started: std::time::SystemTime,
    pub running: bool,
    pub status: String,
    pub log: String,
    pub truncated: bool,
    pub problems: Vec<Problem>,
    pub snapshots: std::collections::BTreeMap<PathBuf, Option<FileStamp>>,
    pub unsaved: Vec<PathBuf>,
}

#[derive(Default)]
pub struct CompilationState {
    pub serial: u64,
    pub current: Option<BuildRun>,
    pub controller: helix_event::TaskController,
    pub log_document: Option<crate::DocumentId>,
}
