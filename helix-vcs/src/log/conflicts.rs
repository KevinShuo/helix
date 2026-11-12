//! Structured conflicts. Tokens are issued by reads, never assembled by callers.
#[cfg(not(feature = "git"))]
use super::GitRepository;
#[cfg(not(feature = "git"))]
use std::path::Path;
use std::path::PathBuf;

pub const MAX_MERGE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_MERGE_LINES: usize = 50_000;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ConflictOperation {
    #[default]
    None,
    Merge {
        head: String,
        parents: Vec<String>,
    },
    Rebase,
    CherryPick,
    Other,
}

impl ConflictOperation {
    pub fn is_merge(&self) -> bool {
        matches!(self, Self::Merge { parents, .. } if parents.len() == 1)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictStage {
    pub oid: String,
    pub mode: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitConflictFile {
    pub path: PathBuf,
    pub status: String,
    /// Base, current, incoming (Git stages 1, 2, 3).
    pub stages: [Option<ConflictStage>; 3],
}

#[derive(Clone, Debug, Default)]
pub struct GitConflictSnapshot {
    pub operation: ConflictOperation,
    pub files: Vec<GitConflictFile>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConflictSupport {
    Text,
    InspectOnly { reason: String },
}

impl ConflictSupport {
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Text => None,
            Self::InspectOnly { reason } => Some(reason),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MergeTextFormat {
    pub bom: bool,
    pub crlf: bool,
}

impl MergeTextFormat {
    pub fn detect(bytes: &[u8]) -> anyhow::Result<Self> {
        anyhow::ensure!(
            bytes.len() <= MAX_MERGE_BYTES,
            "File exceeds the 2 MiB merge limit"
        );
        anyhow::ensure!(
            !bytes.contains(&0),
            "Binary files require an external merge tool"
        );
        let bom = bytes.starts_with(b"\xef\xbb\xbf");
        let text = std::str::from_utf8(if bom { &bytes[3..] } else { bytes })
            .map_err(|_| anyhow::anyhow!("Only UTF-8 merge files are supported"))?;
        anyhow::ensure!(
            text.lines().count() <= MAX_MERGE_LINES,
            "File exceeds the 50,000 line merge limit"
        );
        let crlf = text.contains("\r\n");
        let normalized = text.replace("\r\n", "");
        anyhow::ensure!(
            !normalized.contains('\r') && !(crlf && normalized.contains('\n')),
            "Mixed or unsupported line endings require external resolution"
        );
        Ok(Self { bom, crlf })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(not(feature = "git"), allow(dead_code))]
pub struct ConflictVersion {
    pub(super) root: PathBuf,
    pub(super) git_dir: PathBuf,
    pub(super) file: GitConflictFile,
    pub(super) operation: ConflictOperation,
    pub(super) working: Option<Vec<u8>>,
    pub(super) attributes: Vec<u8>,
    #[cfg(feature = "git")]
    pub(super) path_identity: Option<super::conflicts_write::PathIdentity>,
}

impl ConflictVersion {
    pub fn root(&self) -> &std::path::Path {
        &self.root
    }
}

#[derive(Clone, Debug)]
pub struct GitConflictData {
    pub file: GitConflictFile,
    pub operation: ConflictOperation,
    pub sources: [Option<Vec<u8>>; 3],
    pub working: Option<Vec<u8>>,
    pub format: Option<MergeTextFormat>,
    pub support: ConflictSupport,
    pub version: ConflictVersion,
}

#[derive(Clone, Debug)]
pub struct ResolutionRequest {
    pub version: ConflictVersion,
    pub contents: Vec<u8>,
}

#[derive(Clone, Debug)]
#[cfg_attr(not(feature = "git"), allow(dead_code))]
pub struct StageRetry {
    pub(super) version: ConflictVersion,
    pub(super) contents: Vec<u8>,
}

#[derive(Clone, Debug)]
pub enum ApplyOutcome {
    Staged,
    WrittenNotStaged { retry: StageRetry, message: String },
    NeedsRefresh { message: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeAction {
    Finish,
    Abort,
}

/// A preview and its validation token. Callers cannot change the action or
/// preview independently of the repository state that authorized it.
#[derive(Clone, Debug)]
#[cfg_attr(not(feature = "git"), allow(dead_code))]
pub struct MergeActionPlan {
    pub(super) action: MergeAction,
    pub(super) staged_paths: Vec<PathBuf>,
    pub(super) head: String,
    pub(super) parents: Vec<String>,
    pub(super) default_message: String,
    #[cfg(feature = "git")]
    pub(super) token: super::conflicts_write::MergeActionToken,
}

impl MergeActionPlan {
    pub fn action(&self) -> MergeAction {
        self.action
    }
    pub fn staged_paths(&self) -> &[PathBuf] {
        &self.staged_paths
    }
    pub fn head(&self) -> &str {
        &self.head
    }
    pub fn parents(&self) -> &[String] {
        &self.parents
    }
    pub fn default_message(&self) -> &str {
        &self.default_message
    }
}

#[cfg(not(feature = "git"))]
impl GitRepository {
    pub async fn apply_resolution(
        &self,
        _request: &ResolutionRequest,
    ) -> anyhow::Result<ApplyOutcome> {
        anyhow::bail!("Git support was not compiled in")
    }
    pub async fn retry_stage(&self, _retry: &StageRetry) -> anyhow::Result<ApplyOutcome> {
        anyhow::bail!("Git support was not compiled in")
    }
    pub async fn prepare_merge_action(
        &self,
        _action: MergeAction,
    ) -> anyhow::Result<MergeActionPlan> {
        anyhow::bail!("Git support was not compiled in")
    }
    pub async fn execute_merge_action(
        &self,
        _plan: &MergeActionPlan,
    ) -> anyhow::Result<GitConflictSnapshot> {
        anyhow::bail!("Git support was not compiled in")
    }
    pub async fn conflicts(&self) -> anyhow::Result<GitConflictSnapshot> {
        anyhow::bail!("Git support was not compiled in")
    }
    pub async fn conflict_file(&self, _path: &Path) -> anyhow::Result<GitConflictData> {
        anyhow::bail!("Git support was not compiled in")
    }
}
