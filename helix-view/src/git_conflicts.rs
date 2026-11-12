//! Lifetime and notification state for a current-worktree conflict query.

use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use helix_event::{TaskController, TaskHandle};
use helix_vcs::git_tools::{ConflictOperation, GitConflictFile, GitConflictSnapshot};

static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(1);

/// Kept on the Editor, so callbacks from a closed/replaced editor cannot be
/// accepted by a new one, even if it happens to visit the same repository.
pub struct ConflictScanState {
    instance: u64,
    revision: u64,
    path: Option<PathBuf>,
    controller: TaskController,
    notified: Option<(PathBuf, ConflictOperation, Vec<GitConflictFile>)>,
}

impl Default for ConflictScanState {
    fn default() -> Self {
        Self {
            instance: NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed),
            revision: 0,
            path: None,
            controller: TaskController::new(),
            notified: None,
        }
    }
}

pub struct ConflictScanTicket {
    instance: u64,
    revision: u64,
    path: PathBuf,
    task: TaskHandle,
}

impl ConflictScanTicket {
    pub async fn canceled(&self) {
        self.task.canceled().await;
    }
}

impl ConflictScanState {
    pub fn start(&mut self, path: PathBuf) -> ConflictScanTicket {
        self.revision = self.revision.wrapping_add(1);
        self.path = Some(path.clone());
        ConflictScanTicket {
            instance: self.instance,
            revision: self.revision,
            path,
            task: self.controller.restart(),
        }
    }

    pub fn cancel(&mut self) {
        self.controller.cancel();
        self.path = None;
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn is_running(&self) -> bool {
        self.controller.is_running()
    }

    pub fn accepts(&self, ticket: &ConflictScanTicket, current_path: &Path) -> bool {
        self.instance == ticket.instance
            && self.revision == ticket.revision
            && self.path.as_deref() == Some(current_path)
            && ticket.path == current_path
            && !ticket.task.is_canceled()
    }

    /// Suppress repeated focus messages, but notice changed stages even when the
    /// path/count is unchanged. An empty state resets the notification history.
    pub fn should_notify(&mut self, root: &Path, snapshot: &GitConflictSnapshot) -> bool {
        if snapshot.files.is_empty() {
            self.notified = None;
            return false;
        }
        let next = (
            root.to_owned(),
            snapshot.operation.clone(),
            snapshot.files.clone(),
        );
        let changed = self.notified.as_ref() != Some(&next);
        self.notified = Some(next);
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use helix_vcs::git_tools::{
        ConflictOperation, ConflictStage, GitConflictFile, GitConflictSnapshot,
    };

    #[tokio::test]
    async fn git_conflicts_restarting_scan_cancels_old_instance_and_context() {
        let mut scans = ConflictScanState::default();
        let first = scans.start("first".into());
        assert!(scans.accepts(&first, std::path::Path::new("first")));
        let second = scans.start("second".into());
        first.canceled().await;
        assert!(!scans.accepts(&first, std::path::Path::new("first")));
        assert!(!scans.accepts(&second, std::path::Path::new("first")));
        assert!(scans.accepts(&second, std::path::Path::new("second")));
        let other = ConflictScanState::default();
        assert!(!other.accepts(&second, std::path::Path::new("second")));
        scans.cancel();
        second.canceled().await;
        assert!(!scans.accepts(&second, std::path::Path::new("second")));
    }

    #[test]
    fn git_conflicts_focus_notifications_follow_meaningful_state_changes() {
        let mut scans = ConflictScanState::default();
        let root = std::path::Path::new("repo");
        let mut snapshot = GitConflictSnapshot {
            operation: ConflictOperation::Merge {
                head: "a".repeat(40),
                parents: vec!["b".repeat(40)],
            },
            files: vec![GitConflictFile {
                path: "file.txt".into(),
                status: "UU".into(),
                stages: [
                    None,
                    Some(ConflictStage {
                        oid: "c".repeat(40),
                        mode: 0o100644,
                    }),
                    None,
                ],
            }],
        };
        assert!(scans.should_notify(root, &snapshot));
        assert!(!scans.should_notify(root, &snapshot));
        snapshot.files[0].stages[1].as_mut().unwrap().oid = "d".repeat(40);
        assert!(scans.should_notify(root, &snapshot));
        assert!(!scans.should_notify(root, &GitConflictSnapshot::default()));
        assert!(scans.should_notify(root, &snapshot));
        assert!(scans.should_notify(std::path::Path::new("another"), &snapshot));
    }
}
