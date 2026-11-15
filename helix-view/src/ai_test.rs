//! Explicit unit-test generation configuration and editor-owned operation state.
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct AiTestConfig {
    pub enabled: bool,
    pub provider: Option<String>,
    pub local_provider: Option<String>,
    pub timeout_ms: u64,
    pub max_tokens: u32,
    pub run_timeout_ms: u64,
    pub allow_fetch_content: bool,
    pub python: Option<PathBuf>,
    pub build_directory: Option<PathBuf>,
    pub cmake_preset: Option<String>,
    pub build_configuration: String,
    pub library_target: Option<String>,
    pub test_target: Option<String>,
    pub cmake_args: Vec<String>,
}

impl Default for AiTestConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: None,
            local_provider: None,
            timeout_ms: 120_000,
            max_tokens: 4096,
            run_timeout_ms: 180_000,
            allow_fetch_content: true,
            python: None,
            build_directory: None,
            cmake_preset: None,
            build_configuration: "Debug".into(),
            library_target: None,
            test_target: None,
            cmake_args: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub path: PathBuf,
    pub text: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FileChange {
    pub before: Snapshot,
    pub after: String,
}

#[derive(Debug, Clone)]
pub struct Project {
    pub root: PathBuf,
    pub source: PathBuf,
    pub test_file: PathBuf,
    pub language: String,
    pub target_name: String,
    pub test_filter: String,
    pub context: String,
    pub snapshots: Vec<Snapshot>,
    pub build_changes: Vec<FileChange>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStage {
    Configure,
    Build,
    Test,
    Environment,
}

#[derive(Debug, Clone)]
pub struct RunReport {
    pub stage: RunStage,
    pub success: bool,
    pub repairable: bool,
    pub log: String,
}

#[derive(Clone)]
pub struct Operation {
    pub id: u64,
    pub doc: crate::DocumentId,
    pub version: i32,
    pub model: crate::ai::AiProviderConfig,
    pub config: AiTestConfig,
    pub target: crate::ai_doc::Target,
    pub project: Project,
    pub handle: helix_event::TaskHandle,
    pub changes: Option<Vec<FileChange>>,
    pub assumptions: Vec<String>,
    pub preview: bool,
    pub applied: bool,
    pub running: bool,
    pub repairs: u8,
    pub report: Option<RunReport>,
}

impl Operation {
    pub fn source_valid(&self, editor: &crate::Editor) -> bool {
        !self.handle.is_canceled()
            && editor.config().ai_test == self.config
            && editor
                .config()
                .ai_provider(crate::ai::AiFeature::Test)
                .ok()
                .as_ref()
                == Some(&self.model)
            && editor.document(self.doc).is_some_and(|doc| {
                !doc.readonly
                    && doc.version() == self.version
                    && doc.path() == Some(self.project.source.as_path())
            })
    }
}
