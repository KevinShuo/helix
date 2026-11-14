//! Snapshot-bound staging and commits. Git's own locks protect each write;
//! optimistic validation detects external changes before we start that write.
use super::GitRepository;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitSnapshot {
    pub head: Option<String>,
    pub head_name: String,
    pub files: Vec<CommitFile>,
    stamp: CommitStamp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitFile {
    pub id: usize,
    pub path: PathBuf,
    pub old_path: Option<PathBuf>,
    pub staged: bool,
    pub unstaged: bool,
    pub untracked: bool,
    pub partial: bool,
    pub staged_diff: String,
    pub worktree_diff: String,
    pub ai_eligible: bool,
    pub exclusion: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitOutcome {
    pub oid: String,
    pub message: String,
    pub changed_from_preview: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CommitStamp {
    root: PathBuf,
    git_dir: PathBuf,
    head_ref: Vec<u8>,
    index: Vec<u8>,
    status: Vec<u8>,
    working: Vec<(PathBuf, String)>,
}

impl CommitSnapshot {
    pub fn is_detached(&self) -> bool {
        self.head.is_some() && self.stamp.head_ref.is_empty()
    }

    /// Compare the complete commit input, independent of unstaged edits.
    pub fn same_index(&self, other: &Self) -> bool {
        self.head == other.head
            && self.stamp.root == other.stamp.root
            && self.stamp.git_dir == other.stamp.git_dir
            && self.stamp.head_ref == other.stamp.head_ref
            && self.stamp.index == other.stamp.index
    }
}

#[cfg(feature = "git")]
mod backend {
    use super::*;
    use crate::git_tools::implementation::*;
    use anyhow::{ensure, Context, Result};
    use std::{collections::BTreeSet, fs, io::Write, path::Path};

    const MAX_TEXT: u64 = 256 * 1024;
    const MAX_FILES: usize = 4096;

    fn sensitive(path: &Path) -> bool {
        path.components().any(|part| {
            let name = part.as_os_str().to_string_lossy().to_ascii_lowercase();
            name == ".env"
                || name.starts_with(".env.")
                || name == ".ssh"
                || name == ".aws"
                || name == ".gnupg"
                || name == ".netrc"
                || name == ".npmrc"
                || name == ".pypirc"
                || name == "credentials"
                || name == "credentials.json"
                || name == "secrets.json"
                || name == "secrets.yaml"
                || name == "secrets.yml"
                || name.starts_with("id_rsa")
                || name.starts_with("id_ed25519")
                || [".pem", ".key", ".p12", ".pfx", ".keystore"]
                    .iter()
                    .any(|suffix| name.ends_with(suffix))
        })
    }

    fn disk_fingerprint(root: &Path, path: &Path) -> Result<String> {
        // Inspect every ancestor before opening a candidate. Never follow an
        // untracked symlink/junction into content outside the repository.
        let mut full = root.to_owned();
        for component in path.components() {
            full.push(component);
            let meta = match fs::symlink_metadata(&full) {
                Ok(meta) => meta,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok("missing".into()),
                Err(e) => return Err(e).context("Read commit candidate metadata"),
            };
            #[cfg(windows)]
            let link = {
                use std::os::windows::fs::MetadataExt;
                meta.is_symlink() || meta.file_attributes() & 0x400 != 0
            };
            #[cfg(not(windows))]
            let link = meta.is_symlink();
            if link {
                return Ok(format!("link:{:?}", fs::read_link(&full)?));
            }
        }
        let meta = fs::symlink_metadata(&full)?;
        if meta.is_dir() {
            return Ok("directory".into());
        }
        ensure!(meta.is_file(), "Unsupported commit candidate file type");
        let hash = gix::objs::compute_stream_hash(
            gix::hash::Kind::Sha1,
            gix::objs::Kind::Blob,
            &mut fs::File::open(&full)?,
            meta.len(),
            &mut gix::progress::Discard,
            &std::sync::atomic::AtomicBool::new(false),
        )?;
        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::MetadataExt;
            meta.mode()
        };
        #[cfg(not(unix))]
        let mode = u32::from(meta.permissions().readonly());
        Ok(format!("{hash}:{}:{mode}", meta.len()))
    }

    fn records(bytes: &[u8]) -> Result<Vec<(&[u8], PathBuf)>> {
        bytes
            .split(|b| *b == 0)
            .filter(|b| !b.is_empty())
            .map(|row| {
                let tab = row
                    .iter()
                    .position(|b| *b == b'\t')
                    .context("Invalid Git index record")?;
                Ok((&row[..tab], path_from_bytes(&row[tab + 1..])?))
            })
            .collect()
    }

    impl GitRepository {
        async fn commit_state(&self) -> Result<(Option<String>, String, CommitStamp)> {
            for marker in [
                "MERGE_HEAD",
                "CHERRY_PICK_HEAD",
                "REVERT_HEAD",
                "rebase-merge",
                "rebase-apply",
                "sequencer",
            ] {
                ensure!(
                    !self.git_dir.join(marker).exists(),
                    "Git {marker} operation is active; finish or abort it before committing"
                );
            }
            let (head, name) = self.head().await?;
            let symbolic = self
                .run(&args(&["symbolic-ref", "--quiet", "HEAD"]), QUERY_TIMEOUT)
                .await?;
            ensure!(
                symbolic.status.success() || symbolic.status.code() == Some(1),
                "Cannot resolve symbolic HEAD"
            );
            let index = self.checked(&args(&["ls-files", "--stage", "-z"])).await?;
            for (header, _) in records(&index)? {
                ensure!(
                    header.ends_with(b" 0"),
                    "Unmerged index; resolve conflicts before committing"
                );
            }
            // --no-optional-locks prevents status's index-stat refresh writes.
            let status = self
                .checked(&args(&[
                    "--no-optional-locks",
                    "status",
                    "--porcelain=v1",
                    "-z",
                    "--untracked-files=all",
                    "--ignore-submodules=none",
                ]))
                .await?;
            let paths = parse_status(&status)?;
            ensure!(
                paths.len() <= MAX_FILES,
                "Too many changed files; narrow the repository changes before using AI commit"
            );
            let root = self.root.clone();
            let names: BTreeSet<_> = paths
                .iter()
                .flat_map(|(_, path, old)| std::iter::once(path.clone()).chain(old.clone()))
                .collect();
            let mut working = tokio::task::spawn_blocking(move || {
                names
                    .into_iter()
                    .map(|path| {
                        let hash = disk_fingerprint(&root, &path)?;
                        Ok((path, hash))
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .await??;
            for (path, fingerprint) in &mut working {
                if fingerprint == "directory" {
                    // Gitlinks identify the nested HEAD, whose object normally
                    // does not exist in this repository's object database.
                    if let Ok(nested) = GitRepository::open(self.root.join(&*path), true).await {
                        if !self.same_worktree(&nested) {
                            let nested_head = nested.head().await?;
                            fingerprint
                                .push_str(&format!(":{:?}:{:?}", nested_head.0, nested_head.1));
                        }
                    }
                }
            }
            Ok((
                head,
                name,
                CommitStamp {
                    root: self.root.clone(),
                    git_dir: self.git_dir.clone(),
                    head_ref: symbolic.stdout,
                    index,
                    status,
                    working,
                },
            ))
        }

        pub async fn commit_snapshot(&self) -> Result<CommitSnapshot> {
            // Compare pre/post content digests as well as HEAD/index. A mixed
            // snapshot is discarded, never supplied to the model.
            for _ in 0..2 {
                let (head, head_name, stamp) = self.commit_state().await?;
                let mut files = Vec::new();
                let status_records = parse_status(&stamp.status)?;
                // Porcelain emits staged deletion and recreated untracked
                // content as two records for the same path. Keep their IDs and
                // diffs separate, but mark replacement as partial so it needs
                // explicit selection just like any other staged replacement.
                let staged_paths: BTreeSet<_> = status_records
                    .iter()
                    .filter(|(status, _, _)| status[0] != b' ' && status[0] != b'?')
                    .map(|(_, path, _)| path.clone())
                    .collect();
                for (id, (status, path, old_path)) in status_records.into_iter().enumerate() {
                    self.relative(&path)?;
                    let staged = status[0] != b' ' && status[0] != b'?';
                    let untracked = status == *b"??";
                    let unstaged = status[1] != b' ' || untracked;
                    let partial = unstaged && (staged || staged_paths.contains(&path));
                    let mut exclusion =
                        if sensitive(&path) || old_path.as_ref().is_some_and(|p| sensitive(p)) {
                            Some("Sensitive path: content withheld".to_owned())
                        } else {
                            None
                        };
                    let fingerprint = stamp
                        .working
                        .iter()
                        .find(|(p, _)| *p == path)
                        .map(|(_, h)| h.as_str())
                        .unwrap_or("");
                    if fingerprint.starts_with("link:") {
                        exclusion = Some("Symbolic link: manual review required".into());
                    }
                    if fingerprint.starts_with("directory") {
                        exclusion =
                            Some("Submodule or nested repository: manual review required".into());
                    }
                    for (header, indexed) in records(&stamp.index)? {
                        if indexed == path || old_path.as_ref() == Some(&indexed) {
                            if header.starts_with(b"120000 ") {
                                exclusion = Some("Symbolic link: manual review required".into());
                            }
                            if header.starts_with(b"160000 ") {
                                exclusion = Some("Submodule: manual review required".into());
                            }
                            if !header.starts_with(b"160000 ") {
                                let oid = std::str::from_utf8(header)?
                                    .split(' ')
                                    .nth(1)
                                    .context("Missing index object")?;
                                let size = self.checked(&args(&["cat-file", "-s", oid])).await?;
                                if std::str::from_utf8(&size)?.trim().parse::<u64>()? > MAX_TEXT {
                                    exclusion =
                                        Some("Oversized file: manual review required".into());
                                }
                            }
                        }
                    }
                    if fs::symlink_metadata(self.root.join(&path))
                        .ok()
                        .is_some_and(|m| m.len() > MAX_TEXT)
                    {
                        exclusion = Some("Oversized file: manual review required".into());
                    }
                    // Check the base blob too: a large removed file must not
                    // generate an unbounded patch just because the disk is small.
                    if head.is_some() {
                        let mut command = args(&["ls-tree", "-l", "-z", "HEAD", "--"]);
                        command.push(old_path.as_ref().unwrap_or(&path).as_os_str().to_owned());
                        let base = self.checked(&command).await?;
                        if base.starts_with(b"120000 ") {
                            exclusion = Some("Symbolic link: manual review required".into());
                        }
                        if base.starts_with(b"160000 ") {
                            exclusion = Some("Submodule: manual review required".into());
                        }
                        if let Some(tab) = base.iter().position(|b| *b == b'\t') {
                            if String::from_utf8_lossy(&base[..tab])
                                .split_whitespace()
                                .nth(3)
                                .and_then(|s| s.parse::<u64>().ok())
                                .is_some_and(|n| n > MAX_TEXT)
                            {
                                exclusion =
                                    Some("Oversized base file: manual review required".into());
                            }
                        }
                    }
                    let mut staged_diff = String::new();
                    let mut worktree_diff = String::new();
                    if exclusion.is_none() {
                        if staged {
                            staged_diff =
                                self.commit_patch(true, &path, old_path.as_deref()).await?;
                        }
                        if untracked {
                            let full = self.root.join(&path);
                            let bytes = fs::read(full)?;
                            if bytes.contains(&0) || std::str::from_utf8(&bytes).is_err() {
                                exclusion = Some("Binary file: manual review required".into());
                            } else {
                                worktree_diff = format!(
                                    "new file {}\n{}",
                                    path.to_string_lossy(),
                                    std::str::from_utf8(&bytes)?
                                );
                            }
                        } else if unstaged {
                            worktree_diff =
                                self.commit_patch(false, &path, old_path.as_deref()).await?;
                        }
                        if staged_diff.contains("Binary files ")
                            || worktree_diff.contains("Binary files ")
                            || staged_diff.contains('\0')
                            || worktree_diff.contains('\0')
                        {
                            exclusion = Some("Binary file: manual review required".into());
                        }
                        if staged_diff.len() + worktree_diff.len() > MAX_TEXT as usize {
                            exclusion = Some("Oversized diff: manual review required".into());
                        }
                    }
                    if exclusion.is_some() {
                        staged_diff.clear();
                        worktree_diff.clear();
                    }
                    files.push(CommitFile {
                        id,
                        path,
                        old_path,
                        staged,
                        unstaged,
                        untracked,
                        partial,
                        staged_diff,
                        worktree_diff,
                        ai_eligible: exclusion.is_none(),
                        exclusion,
                    });
                }
                let after = self.commit_state().await?;
                if head == after.0 && head_name == after.1 && stamp == after.2 {
                    return Ok(CommitSnapshot {
                        head,
                        head_name,
                        files,
                        stamp,
                    });
                }
            }
            anyhow::bail!("Repository changed while reading commit snapshot; refresh and retry")
        }

        async fn commit_patch(
            &self,
            cached: bool,
            path: &Path,
            old: Option<&Path>,
        ) -> Result<String> {
            let mut command = args(&[
                "diff",
                "--no-ext-diff",
                "--no-textconv",
                "--no-color",
                "--no-renames",
                "--submodule=short",
            ]);
            if cached {
                command.push("--cached".into());
            }
            command.push("--".into());
            command.push(path.as_os_str().to_owned());
            if let Some(old) = old {
                command.push(old.as_os_str().to_owned());
            }
            Ok(String::from_utf8_lossy(&self.checked(&command).await?).into_owned())
        }

        fn assigned_commit_file<'a>(
            &self,
            snapshot: &'a CommitSnapshot,
            id: usize,
        ) -> Result<&'a CommitFile> {
            let file = snapshot
                .files
                .get(id)
                .filter(|f| f.id == id)
                .context("Unknown commit file ID")?;
            let assigned = parse_status(&snapshot.stamp.status)?;
            let (_, path, old) = assigned.get(id).context("Unknown commit file ID")?;
            ensure!(
                &file.path == path && &file.old_path == old,
                "Commit file metadata changed; refresh the snapshot"
            );
            Ok(file)
        }

        /// Explicit, local-only inspection. Excluded content is never added to
        /// the AI-facing snapshot. The existing Git output cap bounds patches.
        pub async fn commit_file_patch(
            &self,
            snapshot: &CommitSnapshot,
            id: usize,
            staged: bool,
        ) -> Result<String> {
            self.validate_commit_snapshot(snapshot, !staged).await?;
            let file = self.assigned_commit_file(snapshot, id)?;
            let patch = if !staged && file.untracked {
                let full = self.root.join(&file.path);
                let metadata = fs::symlink_metadata(&full)?;
                let fingerprint = disk_fingerprint(&self.root, &file.path)?;
                if fingerprint.starts_with("link:") || metadata.is_dir() {
                    format!(
                        "{}: {}",
                        file.path.to_string_lossy(),
                        file.exclusion.as_deref().unwrap_or("Metadata only")
                    )
                } else {
                    ensure!(metadata.len() <= 16 * 1024 * 1024, "Local file exceeds the 16 MiB preview limit; inspect it in a separate editor buffer");
                    let bytes = fs::read(full)?;
                    if bytes.contains(&0) {
                        format!(
                            "{}: binary file ({} bytes)",
                            file.path.to_string_lossy(),
                            bytes.len()
                        )
                    } else {
                        String::from_utf8(bytes)
                            .context("Local file is not UTF-8 text; open it directly")?
                    }
                }
            } else {
                self.commit_patch(staged, &file.path, file.old_path.as_deref())
                    .await?
            };
            self.validate_commit_snapshot(snapshot, !staged).await?;
            Ok(patch)
        }

        async fn validate_commit_snapshot(
            &self,
            snapshot: &CommitSnapshot,
            working: bool,
        ) -> Result<()> {
            let (head, name, stamp) = self.commit_state().await?;
            let current = CommitSnapshot {
                head,
                head_name: name,
                files: Vec::new(),
                stamp,
            };
            ensure!(
                snapshot.same_index(&current) && (!working || snapshot.stamp == current.stamp),
                "Commit preview is stale; refresh and review the actual repository state"
            );
            Ok(())
        }

        pub async fn stage_files(
            &self,
            snapshot: &CommitSnapshot,
            ids: &[usize],
        ) -> Result<CommitSnapshot> {
            self.change_commit_index(snapshot, ids, true).await
        }

        pub async fn unstage_files(
            &self,
            snapshot: &CommitSnapshot,
            ids: &[usize],
        ) -> Result<CommitSnapshot> {
            self.change_commit_index(snapshot, ids, false).await
        }

        async fn change_commit_index(
            &self,
            snapshot: &CommitSnapshot,
            ids: &[usize],
            stage: bool,
        ) -> Result<CommitSnapshot> {
            let _guard = self.begin_operation()?;
            self.validate_commit_snapshot(snapshot, stage).await?;
            ensure!(!ids.is_empty(), "No files selected");
            let mut selected = BTreeSet::new();
            let mut unique = BTreeSet::new();
            for id in ids {
                ensure!(unique.insert(id), "Duplicate commit file ID");
                let file = self.assigned_commit_file(snapshot, *id)?;
                ensure!(stage || file.staged, "Selected file has no staged changes");
                selected.insert(self.relative(&file.path)?);
                if let Some(old) = &file.old_path {
                    // Only an old entry still tracked in the original index
                    // belongs to this staging action. A recreated untracked
                    // old name is a separate candidate requiring selection.
                    // Reset still needs both names to undo a staged rename.
                    if !stage
                        || records(&snapshot.stamp.index)?
                            .iter()
                            .any(|(_, p)| p == old)
                    {
                        selected.insert(self.relative(old)?);
                    }
                }
            }
            let mut command = if stage {
                args(&["add", "--all", "--"])
            } else if snapshot.head.is_some() {
                args(&["reset", "--quiet", "HEAD", "--"])
            } else {
                args(&[
                    "rm",
                    "--cached",
                    "--force",
                    "--quiet",
                    "--ignore-unmatch",
                    "--",
                ])
            };
            command.extend(selected.iter().map(|p| p.as_os_str().to_owned()));
            let result = self.run(&command, OPERATION_TIMEOUT).await;
            match result {
                Ok(out) if out.status.success() => self.commit_snapshot().await,
                result => {
                    let actual = self.commit_state().await;
                    let detail = match result {
                        Ok(out) => String::from_utf8_lossy(&out.stderr).into_owned(),
                        Err(e) => format!("{e:#}"),
                    };
                    anyhow::bail!("Git index operation failed: {detail}. Actual HEAD: {}; refresh to inspect the actual index", actual.ok().and_then(|s| s.0).as_deref().unwrap_or("unborn or unavailable"))
                }
            }
        }

        pub async fn commit_staged(
            &self,
            snapshot: &CommitSnapshot,
            message: &str,
        ) -> Result<CommitOutcome> {
            let _guard = self.begin_operation()?;
            ensure!(
                !message.trim().is_empty() && !message.contains('\0'),
                "Commit message must be nonempty UTF-8 text without NUL"
            );
            self.validate_commit_snapshot(snapshot, false).await?;
            let diff = self
                .run(
                    &args(&[
                        "diff",
                        "--no-ext-diff",
                        "--no-textconv",
                        "--cached",
                        "--quiet",
                        "--exit-code",
                    ]),
                    QUERY_TIMEOUT,
                )
                .await?;
            ensure!(diff.status.code() == Some(1), "Nothing staged to commit");
            let expected_tree = self.checked(&args(&["write-tree"])).await?;
            self.validate_commit_snapshot(snapshot, false).await?;
            let mut file =
                tempfile::NamedTempFile::new().context("Create UTF-8 commit message file")?;
            file.write_all(message.as_bytes())?;
            file.flush()?;
            let mut command = args(&["commit", "--cleanup=verbatim", "--file"]);
            command.push(file.path().as_os_str().to_owned());
            let result = self.run(&command, OPERATION_TIMEOUT).await;
            let head = self.head().await;
            match result {
                Ok(out) if out.status.success() => {
                    let oid = head?
                        .0
                        .context("Git reported success without a commit HEAD")?;
                    ensure!(Some(&oid) != snapshot.head.as_ref(), "Git reported success but HEAD is unchanged; inspect actual repository state");
                    let actual_message = self
                        .checked(&args(&["show", "--no-patch", "--format=%B", &oid]))
                        .await?;
                    let tree = self
                        .checked(&args(&["rev-parse", &format!("{oid}^{{tree}}")]))
                        .await?;
                    let message_actual = String::from_utf8(actual_message)?
                        .trim_end_matches('\n')
                        .to_owned();
                    let changed_from_preview =
                        expected_tree != tree || message_actual != message.trim_end_matches('\n');
                    Ok(CommitOutcome {
                        oid,
                        message: message_actual,
                        changed_from_preview,
                    })
                }
                result => {
                    let actual = head.ok().and_then(|s| s.0);
                    let detail = match result {
                        Ok(out) => format!(
                            "{}{}",
                            String::from_utf8_lossy(&out.stdout),
                            String::from_utf8_lossy(&out.stderr)
                        ),
                        Err(e) => format!("{e:#}"),
                    };
                    anyhow::bail!("Git commit failed or timed out: {}. Actual HEAD: {} (preview HEAD: {}). Refresh before retrying; inspect the actual index and hooks", detail.trim(), actual.as_deref().unwrap_or("unborn or unavailable"), snapshot.head.as_deref().unwrap_or("unborn"))
                }
            }
        }
    }

    type StatusRecord = ([u8; 2], PathBuf, Option<PathBuf>);
    fn parse_status(bytes: &[u8]) -> Result<Vec<StatusRecord>> {
        let mut fields = bytes.split(|b| *b == 0).filter(|b| !b.is_empty());
        let mut files = Vec::new();
        while let Some(row) = fields.next() {
            ensure!(
                row.len() >= 4 && row[2] == b' ',
                "Invalid Git status record"
            );
            let status = [row[0], row[1]];
            let path = path_from_bytes(&row[3..])?;
            let old = if status.contains(&b'R') || status.contains(&b'C') {
                Some(path_from_bytes(
                    fields.next().context("Missing rename source")?,
                )?)
            } else {
                None
            };
            files.push((status, path, old));
        }
        Ok(files)
    }
}

#[cfg(not(feature = "git"))]
impl GitRepository {
    pub async fn commit_snapshot(&self) -> anyhow::Result<CommitSnapshot> {
        anyhow::bail!("Git support was not compiled in")
    }
    pub async fn stage_files(
        &self,
        _: &CommitSnapshot,
        _: &[usize],
    ) -> anyhow::Result<CommitSnapshot> {
        anyhow::bail!("Git support was not compiled in")
    }
    pub async fn unstage_files(
        &self,
        _: &CommitSnapshot,
        _: &[usize],
    ) -> anyhow::Result<CommitSnapshot> {
        anyhow::bail!("Git support was not compiled in")
    }
    pub async fn commit_staged(
        &self,
        _: &CommitSnapshot,
        _: &str,
    ) -> anyhow::Result<CommitOutcome> {
        anyhow::bail!("Git support was not compiled in")
    }
    pub async fn commit_file_patch(
        &self,
        _: &CommitSnapshot,
        _: usize,
        _: bool,
    ) -> anyhow::Result<String> {
        anyhow::bail!("Git support was not compiled in")
    }
}
