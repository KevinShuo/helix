//! Test workbench state, independent of AI generation and model providers.
use std::{collections::HashMap, path::PathBuf};

use serde::{Deserialize, Serialize};

pub mod discovery;
pub use discovery::{discover, Declaration};

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct TestConfig {
    pub python: Option<PathBuf>,
    pub build_directory: Option<PathBuf>,
    pub cmake_preset: Option<String>,
    pub build_configuration: Option<String>,
    pub target: Option<String>,
    pub timeout_ms: Option<u64>,
    pub cmake_args: Vec<String>,
}

impl TestConfig {
    pub fn effective(
        &self,
        fallback: &crate::ai_test::AiTestConfig,
    ) -> crate::ai_test::AiTestConfig {
        let mut config = fallback.clone();
        if self.python.is_some() {
            config.python = self.python.clone();
        }
        if self.build_directory.is_some() {
            config.build_directory = self.build_directory.clone();
        }
        if self.cmake_preset.is_some() {
            config.cmake_preset = self.cmake_preset.clone();
        }
        if let Some(value) = &self.build_configuration {
            config.build_configuration = value.clone();
        }
        // The AI generator's last target must not restrict ordinary project runs.
        config.test_target = self.target.clone();
        if let Some(value) = self.timeout_ms {
            config.run_timeout_ms = value;
        }
        if !self.cmake_args.is_empty() {
            config.cmake_args = self.cmake_args.clone();
        }
        config
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    #[default]
    Queued,
    Running,
    Passed,
    Failed,
    Skipped,
    Error,
    Cancelled,
}
impl Outcome {
    pub fn failed(self) -> bool {
        matches!(self, Self::Failed | Self::Error)
    }
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Queued => "▶",
            Self::Running => "◷",
            Self::Passed => "✓",
            Self::Failed => "✗",
            Self::Skipped => "−",
            Self::Error => "!",
            Self::Cancelled => "■",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestCase {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub selector: Option<String>,
    #[serde(default)]
    pub path: PathBuf,
    #[serde(default)]
    pub line: usize,
    #[serde(default)]
    pub outcome: Outcome,
    #[serde(default)]
    pub duration: f64,
    #[serde(default)]
    pub details: String,
    #[serde(default)]
    pub failure_path: Option<PathBuf>,
    #[serde(default)]
    pub failure_line: Option<usize>,
}

#[derive(Debug, Clone)]
pub enum Scope {
    Declaration {
        path: PathBuf,
        declaration: Declaration,
    },
    File(PathBuf),
    Project,
    Cases(Vec<String>),
}

#[derive(Debug, Clone)]
pub struct RunRequest {
    pub root: PathBuf,
    pub language: String,
    pub config: crate::ai_test::AiTestConfig,
    pub scope: Scope,
}

#[derive(Default)]
pub struct TestState {
    pub declarations: HashMap<PathBuf, DeclarationCache>,
    pub cases: Vec<TestCase>,
    pub request: Option<RunRequest>,
    pub id: u64,
    pub running: bool,
    pub stale: bool,
    pub phase: String,
    pub log: String,
    pub panel_visible: bool,
    pub focus_requested: bool,
    pub task: Option<tokio::task::AbortHandle>,
    pub versions: HashMap<PathBuf, (crate::DocumentId, i32)>,
}
pub struct DeclarationCache {
    pub document: crate::DocumentId,
    pub language: String,
    pub version: i32,
    pub declarations: Vec<Declaration>,
}
impl Drop for TestState {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
impl TestState {
    pub fn failed_ids(&self) -> Vec<String> {
        self.cases
            .iter()
            .filter(|c| c.outcome.failed())
            .map(|c| c.id.clone())
            .collect()
    }
    pub fn aggregate<'a>(cases: impl Iterator<Item = &'a TestCase>) -> Outcome {
        let values: Vec<_> = cases.map(|c| c.outcome).collect();
        if values.contains(&Outcome::Running) {
            Outcome::Running
        } else if values.contains(&Outcome::Error) {
            Outcome::Error
        } else if values.contains(&Outcome::Failed) {
            Outcome::Failed
        } else if values.contains(&Outcome::Queued) || values.is_empty() {
            Outcome::Queued
        } else if values.contains(&Outcome::Cancelled) {
            Outcome::Cancelled
        } else if values.iter().all(|o| *o == Outcome::Skipped) {
            Outcome::Skipped
        } else {
            Outcome::Passed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn declaration_runtime_ids_do_not_expand_to_neighbouring_tests() {
        let d = Declaration {
            start_line: 1,
            line: 2,
            end_line: 5,
            name: "test_x".into(),
            suite: Some("TestA".into()),
            class: false,
        };
        assert!(d.matches_python("test_a.py::TestA::test_x[value::one]"));
        assert!(!d.matches_python("test_a.py::TestA::test_xy"));
        assert!(!d.matches_python("test_a.py::TestB::test_x"));
        let d = Declaration {
            name: "Works".into(),
            suite: Some("Suite".into()),
            ..d
        };
        assert!(d.matches_gtest("Instantiation/Suite.Works/0"));
        assert!(d.matches_gtest("Suite/0.Works"));
        assert!(!d.matches_gtest("OtherSuite.Works"));
    }
    #[test]
    fn no_tests_is_not_a_pass_and_error_overrides_pass() {
        assert_eq!(TestState::aggregate([].iter()), Outcome::Queued);
        let mut c: TestCase =
            serde_json::from_str(r#"{"id":"a","name":"a","outcome":"passed"}"#).unwrap();
        let pass = c.clone();
        c.outcome = Outcome::Error;
        assert_eq!(TestState::aggregate([pass, c].iter()), Outcome::Error);
    }
}
