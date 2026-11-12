//! Optimistic, path-scoped merge writes. The application lock serializes our
//! operations; external Git and filesystem writers are checked, not locked out.
use super::{implementation::*, *};
use anyhow::{ensure, Context, Result};
use std::{collections::BTreeSet, fs, io::Write, path::Component, time::SystemTime};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PathIdentity(Vec<PathNode>);

#[derive(Clone, Debug, PartialEq, Eq)]
struct PathNode {
    canonical: PathBuf,
    created: Option<SystemTime>,
    readonly: bool,
    #[cfg(unix)]
    inode: (u64, u64, u32),
}

fn safe_node(path: &Path, directory: bool) -> Result<PathNode> {
    let metadata = fs::symlink_metadata(path).context("Read merge path metadata")?;
    ensure!(
        !metadata.is_symlink(),
        "Merge paths cannot contain symbolic links"
    );
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        ensure!(
            metadata.file_attributes() & 0x400 == 0,
            "Merge paths cannot contain reparse points or junctions"
        );
    }
    ensure!(
        if directory {
            metadata.is_dir()
        } else {
            metadata.is_file()
        },
        "Merge path has an unsupported file type"
    );
    Ok(PathNode {
        canonical: fs::canonicalize(path)?,
        created: metadata.created().ok(),
        readonly: metadata.permissions().readonly(),
        #[cfg(unix)]
        inode: {
            use std::os::unix::fs::MetadataExt;
            (metadata.dev(), metadata.ino(), metadata.mode())
        },
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct MergeActionToken {
    root: PathBuf,
    git_dir: PathBuf,
    repository: PathIdentity,
    operation: ConflictOperation,
    index: Vec<u8>,
    metadata: Vec<Option<Vec<u8>>>,
    // Abort may overwrite tracked paths absent from the current index too.
    working: Vec<(PathBuf, PathIdentity, Option<Vec<u8>>)>,
}

#[derive(Debug)]
struct IndexEntry {
    path: PathBuf,
    oid: String,
    mode: u32,
    stage: u8,
    record: Vec<u8>,
}

fn index_entries(bytes: &[u8]) -> Result<Vec<IndexEntry>> {
    bytes
        .split(|b| *b == 0)
        .filter(|r| !r.is_empty())
        .map(|r| {
            let tab = r
                .iter()
                .position(|b| *b == b'\t')
                .context("Invalid index record")?;
            let mut header = std::str::from_utf8(&r[..tab])?.split(' ');
            let mode = u32::from_str_radix(header.next().context("Missing index mode")?, 8)?;
            let oid = header.next().context("Missing index object")?.to_owned();
            ensure!(valid_id(&oid), "Invalid index object");
            let stage: u8 = header.next().context("Missing index stage")?.parse()?;
            ensure!(stage <= 3 && header.next().is_none(), "Invalid index stage");
            Ok(IndexEntry {
                path: path_from_bytes(&r[tab + 1..])?,
                oid,
                mode,
                stage,
                record: r.to_vec(),
            })
        })
        .collect()
}

impl GitRepository {
    fn repository_identity(&self) -> Result<PathIdentity> {
        Ok(PathIdentity(vec![
            safe_node(&self.root, true)?,
            safe_node(&self.git_dir, true)?,
        ]))
    }

    /// Validate every component without following a symlink/junction parent.
    pub(super) fn safe_conflict_path(&self, relative: &Path) -> Result<PathIdentity> {
        self.safe_path(relative, false)
    }

    fn safe_path(&self, relative: &Path, allow_missing: bool) -> Result<PathIdentity> {
        ensure!(!relative.as_os_str().is_empty(), "Missing merge path");
        ensure!(
            relative
                .components()
                .all(|c| matches!(c, Component::Normal(_))),
            "Merge path must stay inside the worktree"
        );
        let mut identity = self.repository_identity()?;
        let root = identity.0[0].canonical.clone();
        let mut full = self.root.clone();
        let components: Vec<_> = relative.components().collect();
        for (i, component) in components.iter().enumerate() {
            full.push(component.as_os_str());
            let node = match safe_node(&full, i + 1 < components.len()) {
                Ok(node) => node,
                Err(error)
                    if allow_missing
                        && error
                            .downcast_ref::<std::io::Error>()
                            .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
                {
                    // Even an absent leaf retains its validated parent identity.
                    // Recreating a directory after preview changes this token.
                    PathNode {
                        canonical: root
                            .join(relative.components().take(i + 1).collect::<PathBuf>()),
                        created: None,
                        readonly: false,
                        #[cfg(unix)]
                        inode: (0, 0, 0),
                    }
                }
                Err(error) => return Err(error),
            };
            ensure!(
                node.canonical.starts_with(&root),
                "Merge path resolves outside the worktree"
            );
            identity.0.push(node);
        }
        Ok(identity)
    }

    async fn check_repository_binding(&self) -> Result<()> {
        let current = Self::open(self.root.clone(), true).await?;
        ensure!(
            self.same_worktree(&current),
            "Repository identity changed; refresh conflicts"
        );
        Ok(())
    }

    async fn read_index_entries(&self) -> Result<Vec<u8>> {
        self.checked(&args(&["ls-files", "--stage", "-z"])).await
    }

    async fn validate_resolution(&self, version: &ConflictVersion, working: &[u8]) -> Result<()> {
        ensure!(
            self.root == version.root && self.git_dir == version.git_dir,
            "Resolution belongs to another worktree"
        );
        self.check_repository_binding().await?;
        let mut current = self.conflict_file(&version.file.path).await?;
        ensure!(
            current.support == ConflictSupport::Text,
            "This conflict requires external resolution"
        );
        ensure!(
            current.working.as_deref() == Some(working),
            "Working file changed; refresh conflicts"
        );
        // A retry has a new file identity after atomic replacement, but still
        // carries the original stages, operation and attributes.
        current.version.working = version.working.clone();
        ensure!(
            &current.version == version,
            "Conflict version changed; refresh conflicts"
        );
        Ok(())
    }

    pub async fn apply_resolution(&self, request: &ResolutionRequest) -> Result<ApplyOutcome> {
        let _operation = self.begin_operation()?;
        MergeTextFormat::detect(&request.contents)?;
        let version = &request.version;
        self.validate_resolution(
            version,
            version
                .working
                .as_deref()
                .context("Missing working snapshot")?,
        )
        .await?;
        let full = self.root.join(&version.file.path);
        let permissions = fs::metadata(&full)?.permissions();
        ensure!(
            !permissions.readonly(),
            "Merge result cannot replace a read-only file"
        );
        let before = self.read_index_entries().await?;
        let mut temporary =
            tempfile::NamedTempFile::new_in(full.parent().context("Missing merge parent")?)?;
        temporary.write_all(&request.contents)?;
        temporary.as_file().set_permissions(permissions)?;
        temporary.as_file().sync_all()?;
        let expected_oid = self
            .normalized_oid(&version.file.path, temporary.path())
            .await?;
        // Git/attribute queries and temporary writes created an opportunity for
        // another writer; do the final optimistic check immediately before save.
        self.validate_resolution(version, version.working.as_deref().unwrap())
            .await?;
        ensure!(
            self.read_index_entries().await? == before,
            "Index changed before saving; refresh conflicts"
        );
        ensure!(
            self.safe_conflict_path(&version.file.path)?
                == version
                    .path_identity
                    .clone()
                    .context("Missing path identity")?,
            "Merge path changed before save"
        );
        ensure!(
            super::conflicts_backend::read_bounded(&full)?.as_deref() == version.working.as_deref(),
            "Working file changed before save"
        );
        temporary
            .persist(&full)
            .map_err(|e| e.error)
            .context("Atomically save merge result")?;
        // Every error beyond this point must describe the already-written file.
        let mut retry = StageRetry {
            version: version.clone(),
            contents: request.contents.clone(),
        };
        match self.safe_conflict_path(&version.file.path) {
            Ok(identity) => retry.version.path_identity = Some(identity),
            Err(error) => {
                return Ok(ApplyOutcome::NeedsRefresh {
                    message: format!("Result was written, but path validation failed: {error:#}"),
                })
            }
        }
        self.stage_written(retry, before, Some(expected_oid)).await
    }

    /// Compute Git's expected stage-0 blob with built-in text/eol conversion.
    /// Custom clean filters/encodings are rejected by validate_resolution.
    async fn normalized_oid(&self, relative: &Path, input: &Path) -> Result<String> {
        let mut command = args(&["hash-object", "--path"]);
        command.push(relative.as_os_str().to_owned());
        command.push("--".into());
        command.push(input.as_os_str().to_owned());
        let bytes = self.checked(&command).await?;
        let oid = std::str::from_utf8(&bytes)?.trim().to_owned();
        ensure!(valid_id(&oid), "Invalid normalized merge object");
        Ok(oid)
    }

    pub async fn retry_stage(&self, retry: &StageRetry) -> Result<ApplyOutcome> {
        let _operation = self.begin_operation()?;
        self.validate_resolution(&retry.version, &retry.contents)
            .await?;
        let before = self.read_index_entries().await?;
        self.stage_written(retry.clone(), before, None).await
    }

    async fn stage_written(
        &self,
        retry: StageRetry,
        before: Vec<u8>,
        expected_oid: Option<String>,
    ) -> Result<ApplyOutcome> {
        // This function never writes the worktree; a retry only attempts add.
        let result = self
            .stage_written_inner(&retry, &before, expected_oid)
            .await;
        Ok(match result {
            Ok(()) => ApplyOutcome::Staged,
            Err(error) => {
                // A successful add may already have removed the unmerged
                // entries. In that case this token cannot authorize another
                // add; show the real state through a refresh instead.
                let still_unmerged = self.conflicts().await.ok().is_some_and(|s| {
                    s.operation == retry.version.operation
                        && s.files.iter().any(|f| f == &retry.version.file)
                });
                if still_unmerged {
                    ApplyOutcome::WrittenNotStaged {
                        retry,
                        message: format!("Result was written; staging was not verified: {error:#}"),
                    }
                } else {
                    ApplyOutcome::NeedsRefresh { message: format!("Result was written, but repository state changed during staging: {error:#}") }
                }
            }
        })
    }

    async fn stage_written_inner(
        &self,
        retry: &StageRetry,
        before: &[u8],
        expected_oid: Option<String>,
    ) -> Result<()> {
        self.validate_resolution(&retry.version, &retry.contents)
            .await?;
        let path = &retry.version.file.path;
        let full = self.root.join(path);
        let expected_oid = match expected_oid {
            Some(oid) => oid,
            None => self.normalized_oid(path, &full).await?,
        };
        ensure!(
            self.read_index_entries().await? == before,
            "Index changed before staging"
        );
        self.validate_resolution(&retry.version, &retry.contents)
            .await?;
        let mut command = args(&["add", "--"]);
        command.push(path.as_os_str().to_owned());
        ensure!(
            self.safe_conflict_path(path)?
                == retry
                    .version
                    .path_identity
                    .clone()
                    .context("Missing path identity")?
                && super::conflicts_backend::read_bounded(&full)?.as_deref()
                    == Some(retry.contents.as_slice()),
            "Working file changed before staging"
        );
        let output = self.run(&command, OPERATION_TIMEOUT).await?;
        ensure!(
            output.status.success(),
            "Git add: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        ensure!(
            self.safe_conflict_path(path)?
                == retry
                    .version
                    .path_identity
                    .clone()
                    .context("Missing path identity")?,
            "Path changed during staging"
        );
        ensure!(
            super::conflicts_backend::read_bounded(&full)?.as_deref()
                == Some(retry.contents.as_slice()),
            "Working file changed during staging"
        );
        self.check_repository_binding().await?;
        ensure!(
            self.conflicts().await?.operation == retry.version.operation,
            "Merge operation changed during staging"
        );
        ensure!(
            self.merge_attributes(path).await? == retry.version.attributes,
            "Git attributes changed during staging"
        );
        let after = index_entries(&self.read_index_entries().await?)?;
        let target: Vec<_> = after.iter().filter(|e| e.path == *path).collect();
        ensure!(
            target.len() == 1
                && target[0].stage == 0
                && target[0].oid == expected_oid
                && matches!(target[0].mode, 0o100644 | 0o100755),
            "Staged result does not match the saved content"
        );
        let others = |entries: Vec<IndexEntry>| {
            entries
                .into_iter()
                .filter(|e| e.path != *path)
                .map(|e| e.record)
                .collect::<Vec<_>>()
        };
        ensure!(
            others(index_entries(before)?) == others(after),
            "Other index entries changed during staging; refresh conflicts"
        );
        Ok(())
    }

    pub async fn prepare_merge_action(&self, action: MergeAction) -> Result<MergeActionPlan> {
        let _operation = self.begin_operation()?;
        self.prepare_merge_action_inner(action).await
    }

    async fn prepare_merge_action_inner(&self, action: MergeAction) -> Result<MergeActionPlan> {
        self.check_repository_binding().await?;
        let snapshot = self.conflicts().await?;
        ensure!(
            snapshot.operation.is_merge(),
            "Only an ordinary merge can be finished or aborted"
        );
        if action == MergeAction::Finish {
            ensure!(
                snapshot.files.is_empty(),
                "Resolve all unmerged paths before finishing the merge"
            );
        }
        let ConflictOperation::Merge { head, parents } = &snapshot.operation else {
            unreachable!()
        };
        for parent in parents {
            let commit = self
                .checked(&args(&[
                    "rev-parse",
                    "--verify",
                    &format!("{parent}^{{commit}}"),
                ]))
                .await?;
            ensure!(
                std::str::from_utf8(&commit)?.trim() == parent,
                "MERGE_HEAD must name a commit"
            );
        }
        let repository = self.repository_identity()?;
        let index = self.read_index_entries().await?;
        let mut metadata = Vec::new();
        for name in [
            "MERGE_HEAD",
            "MERGE_MSG",
            "MERGE_MODE",
            "ORIG_HEAD",
            "MERGE_AUTOSTASH",
        ] {
            metadata.push(super::conflicts_backend::read_bounded(
                &self.git_dir.join(name),
            )?);
        }
        ensure!(
            metadata[4].is_none(),
            "This merge has an autostash; finish or abort it externally so Git can restore the stash"
        );
        let message = metadata[1]
            .as_deref()
            .context("Missing default merge message")?;
        let default_message = std::str::from_utf8(message)
            .context("Default merge message is not UTF-8")?
            .to_owned();
        let paths = self
            .checked(&args(&[
                "diff",
                "--no-renames",
                "--no-ext-diff",
                "--no-textconv",
                "--cached",
                "--name-only",
                "-z",
                "HEAD",
                "--",
            ]))
            .await?;
        let staged_paths = paths
            .split(|b| *b == 0)
            .filter(|r| !r.is_empty())
            .map(path_from_bytes)
            .collect::<Result<Vec<_>>>()?;
        let mut working = Vec::new();
        if action == MergeAction::Abort {
            let mut tracked: BTreeSet<PathBuf> =
                snapshot.files.iter().map(|f| f.path.clone()).collect();
            // Abort resets towards ORIG_HEAD. Include both staged and unstaged
            // differences: comparing only worktree vs ORIG_HEAD can hide an
            // index change whose worktree was subsequently restored.
            for command in [
                args(&[
                    "diff",
                    "--no-renames",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--name-only",
                    "-z",
                    "ORIG_HEAD",
                    "--",
                ]),
                args(&[
                    "diff",
                    "--no-renames",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--cached",
                    "--name-only",
                    "-z",
                    "ORIG_HEAD",
                    "--",
                ]),
                args(&[
                    "diff",
                    "--no-renames",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--name-only",
                    "-z",
                    "--",
                ]),
            ] {
                let paths = self.checked(&command).await?;
                for bytes in paths.split(|b| *b == 0).filter(|r| !r.is_empty()) {
                    tracked.insert(path_from_bytes(bytes)?);
                }
            }
            for path in tracked {
                let full = self.root.join(&path);
                let identity = self.safe_path(&path, true)?;
                let state = match fs::symlink_metadata(&full) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                    Err(error) => return Err(error.into()),
                    Ok(_) => {
                        let mut command = args(&["hash-object", "--no-filters", "--"]);
                        command.push(full.as_os_str().to_owned());
                        let digest = self.checked(&command).await?;
                        ensure!(
                            identity == self.safe_conflict_path(&path)?,
                            "Tracked path changed during preview"
                        );
                        Some(digest)
                    }
                };
                working.push((path, identity, state));
            }
        }
        ensure!(
            index == self.read_index_entries().await?
                && snapshot.operation == self.conflicts().await?.operation,
            "Merge state changed during preview; refresh"
        );
        Ok(MergeActionPlan {
            action,
            staged_paths,
            head: head.clone(),
            parents: parents.clone(),
            default_message,
            token: MergeActionToken {
                root: self.root.clone(),
                git_dir: self.git_dir.clone(),
                repository,
                operation: snapshot.operation,
                index,
                metadata,
                working,
            },
        })
    }

    pub async fn execute_merge_action(
        &self,
        plan: &MergeActionPlan,
    ) -> Result<GitConflictSnapshot> {
        let _operation = self.begin_operation()?;
        let current = self.prepare_merge_action_inner(plan.action).await?;
        ensure!(
            current.token == plan.token,
            "Merge state changed after preview; review a fresh preview"
        );
        let command = match plan.action {
            MergeAction::Finish => args(&["commit", "--no-edit"]),
            MergeAction::Abort => args(&["merge", "--abort"]),
        };
        let output = self.run(&command, OPERATION_TIMEOUT).await?;
        ensure!(
            output.status.success(),
            "Git merge action failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        self.conflicts().await
    }
}
