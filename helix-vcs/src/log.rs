//! Native Git log, details and operations used by the terminal tool window.

use std::path::{Path, PathBuf};

#[path = "log/conflicts.rs"]
mod conflicts;
pub use conflicts::*;
#[cfg(feature = "git")]
#[path = "log/conflicts_backend.rs"]
mod conflicts_backend;

#[derive(Clone, Debug)]
pub struct GitRepository {
    root: PathBuf,
    #[cfg(feature = "git")]
    git_dir: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitRefKind {
    Local,
    Remote,
    Tag,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitReference {
    pub full_name: String,
    pub name: String,
    /// Peeled commit for annotated tags.
    pub id: String,
    pub kind: GitRefKind,
    pub current: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitCommit {
    pub id: String,
    pub parents: Vec<String>,
    pub author: String,
    pub date: String,
    pub subject: String,
    pub decoration: String,
    pub graph: String,
    /// Git's connection rows between this commit and the next commit.
    pub graph_after: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct GitLogFilter {
    pub revision: Option<String>,
    pub text: String,
    pub author: String,
    pub since: String,
    pub until: String,
    pub path: Option<PathBuf>,
    pub limit: usize,
}

impl Default for GitLogFilter {
    fn default() -> Self {
        Self {
            revision: None,
            text: String::new(),
            author: String::new(),
            since: String::new(),
            until: String::new(),
            path: None,
            limit: 200,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct GitSnapshot {
    pub head: Option<String>,
    pub head_name: String,
    pub references: Vec<GitReference>,
    pub commits: Vec<GitCommit>,
    pub status: Vec<GitFile>,
    pub has_more: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitTarget {
    Commit(String),
    WorkingTree,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitFile {
    /// Name-status for commits, two-column porcelain status for working changes.
    pub status: String,
    pub path: PathBuf,
    pub old_path: Option<PathBuf>,
}

#[derive(Clone, Debug, Default)]
pub struct GitDetails {
    pub message: String,
    pub files: Vec<GitFile>,
}

#[derive(Clone, Debug)]
pub enum GitOperation {
    Checkout { reference: GitReference },
    NewBranch { name: String, start: String },
    Merge { reference: String },
    CherryPick { commit: String },
    Fetch,
    Pull,
    Push,
    PushPlanned(GitPushPlan),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitPushPlan {
    pub local_branch: String,
    pub source_id: String,
    pub remote: String,
    pub destination: String,
}

impl GitOperation {
    pub fn changes_worktree(&self) -> bool {
        !matches!(self, Self::Fetch | Self::Push | Self::PushPlanned(_))
    }
}

impl GitRepository {
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn same_worktree(&self, other: &Self) -> bool {
        if self.root != other.root {
            return false;
        }
        #[cfg(feature = "git")]
        {
            self.git_dir == other.git_dir
        }
        #[cfg(not(feature = "git"))]
        {
            true
        }
    }
}

#[cfg(feature = "git")]
mod implementation {
    use super::*;
    use anyhow::{ensure, Context, Result};
    use gix::bstr::ByteSlice;
    use std::{
        collections::HashSet,
        ffi::OsString,
        path::Component,
        process::{Output, Stdio},
        sync::OnceLock,
        time::Duration,
    };
    use tokio::{io::AsyncReadExt, process::Command};

    const MAX_OUTPUT: usize = 16 * 1024 * 1024;
    pub(super) const QUERY_TIMEOUT: Duration = Duration::from_secs(20);
    pub(super) const OPERATION_TIMEOUT: Duration = Duration::from_secs(60);
    static OPERATIONS: OnceLock<parking_lot::Mutex<HashSet<PathBuf>>> = OnceLock::new();

    pub(super) struct OperationGuard(PathBuf);

    impl Drop for OperationGuard {
        fn drop(&mut self) {
            OPERATIONS
                .get_or_init(Default::default)
                .lock()
                .remove(&self.0);
        }
    }

    fn normalize_path(path: &Path) -> Result<PathBuf> {
        let mut ancestor = if path.is_absolute() {
            path.to_owned()
        } else {
            std::env::current_dir()?.join(path)
        };
        let mut missing = Vec::new();
        let mut resolved = loop {
            match std::fs::canonicalize(&ancestor) {
                Ok(path) => break path,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    missing.push(
                        ancestor
                            .file_name()
                            .context("Cannot resolve Git path")?
                            .to_owned(),
                    );
                    ensure!(ancestor.pop(), "Cannot resolve Git path");
                }
                Err(error) => return Err(error).context("Resolve Git path"),
            }
        };
        resolved.extend(missing.into_iter().rev());
        #[cfg(windows)]
        {
            use std::path::{Component, Prefix};
            let mut components = resolved.components();
            let mut ordinary = match components.next() {
                Some(Component::Prefix(prefix)) => match prefix.kind() {
                    Prefix::VerbatimDisk(drive) => PathBuf::from(format!("{}:\\", drive as char)),
                    Prefix::VerbatimUNC(server, share) => {
                        let mut path = PathBuf::from(r"\\");
                        path.push(server);
                        path.push(share);
                        path
                    }
                    _ => return Ok(resolved),
                },
                _ => return Ok(resolved),
            };
            for component in components {
                if !matches!(component, Component::RootDir) {
                    ordinary.push(component.as_os_str());
                }
            }
            return Ok(ordinary);
        }
        #[cfg(not(windows))]
        Ok(resolved)
    }

    pub(super) fn valid_id(id: &str) -> bool {
        matches!(id.len(), 40 | 64) && id.bytes().all(|b| b.is_ascii_hexdigit())
    }

    fn valid_revision(revision: &str) -> Result<()> {
        ensure!(
            revision == "HEAD"
                || valid_id(revision)
                || ["refs/heads/", "refs/remotes/", "refs/tags/"]
                    .iter()
                    .any(|prefix| revision.starts_with(prefix)),
            "Expected a full Git reference or commit id"
        );
        ensure!(
            !revision.contains(['\0', '\n', '\r']),
            "Invalid Git reference"
        );
        Ok(())
    }

    async fn read_output(reader: impl tokio::io::AsyncRead + Unpin) -> Result<Vec<u8>> {
        let mut result = Vec::new();
        reader
            .take((MAX_OUTPUT + 1) as u64)
            .read_to_end(&mut result)
            .await?;
        ensure!(
            result.len() <= MAX_OUTPUT,
            "Git output exceeds the 16 MiB limit; narrow the filter"
        );
        Ok(result)
    }

    pub(super) fn path_from_bytes(bytes: &[u8]) -> Result<PathBuf> {
        Ok(bytes.as_bstr().to_path()?.to_path_buf())
    }

    impl GitRepository {
        pub async fn open(path: PathBuf, trust_full: bool) -> Result<Self> {
            ensure!(trust_full, "Git tools require a trusted workspace (:trust)");
            tokio::task::spawn_blocking(move || {
                let path = normalize_path(&path).context("Resolve Git workspace path")?;
                let directory = if path.is_file() {
                    path.parent().context("File has no parent directory")?
                } else {
                    path.as_path()
                };
                let (repository, _) = gix::discover::upwards_opts(
                    directory,
                    gix::discover::upwards::Options {
                        dot_git_only: true,
                        ..Default::default()
                    },
                )
                .context("No Git repository found")?;
                let (git_dir, work_dir) = repository.into_repository_and_work_tree_directories();
                Ok(Self {
                    root: normalize_path(&work_dir.context("Git tools require a working tree")?)?,
                    git_dir: normalize_path(&git_dir)?,
                })
            })
            .await?
        }

        pub(super) async fn run(&self, args: &[OsString], timeout: Duration) -> Result<Output> {
            let mut command = Command::new("git");
            command
                .args(["--no-pager", "--no-replace-objects", "--literal-pathspecs"])
                .arg("--git-dir")
                .arg(&self.git_dir)
                .arg("--work-tree")
                .arg(&self.root)
                .args([
                    "-c",
                    "core.fsmonitor=false",
                    "-c",
                    "color.ui=false",
                    "-c",
                    "core.quotepath=false",
                    "-c",
                    "i18n.logOutputEncoding=UTF-8",
                    "-c",
                    "log.showSignature=false",
                ])
                .args(args)
                .current_dir(&self.root)
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .env_remove("GIT_COMMON_DIR")
                .env_remove("GIT_INDEX_FILE")
                .env_remove("GIT_OBJECT_DIRECTORY")
                .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
                .env_remove("GIT_NAMESPACE")
                .env("GIT_TERMINAL_PROMPT", "0")
                .env("GCM_INTERACTIVE", "never")
                .env("GIT_NO_LAZY_FETCH", "1")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true);
            #[cfg(windows)]
            command.creation_flags(0x08000000);
            let mut child = command
                .spawn()
                .context("Run Git (is git installed on PATH?)")?;
            let stdout = child.stdout.take().context("Git stdout unavailable")?;
            let stderr = child.stderr.take().context("Git stderr unavailable")?;
            let (stdout, stderr, status) = tokio::time::timeout(timeout, async {
                tokio::try_join!(read_output(stdout), read_output(stderr), async {
                    Ok::<_, anyhow::Error>(child.wait().await?)
                })
            })
            .await
            .context("Git command timed out")??;
            Ok(Output {
                status,
                stdout,
                stderr,
            })
        }

        pub(super) async fn checked(&self, args: &[OsString]) -> Result<Vec<u8>> {
            let output = self.run(args, QUERY_TIMEOUT).await?;
            ensure!(
                output.status.success(),
                "Git: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
            Ok(output.stdout)
        }

        pub(super) fn relative(&self, path: &Path) -> Result<PathBuf> {
            let resolved;
            let path = if path.is_absolute() {
                resolved = normalize_path(path)?;
                resolved
                    .strip_prefix(&self.root)
                    .context("Path is outside Git worktree")?
            } else {
                path
            };
            ensure!(
                path.components()
                    .all(|c| matches!(c, Component::Normal(_) | Component::CurDir)),
                "Path must stay inside the Git worktree"
            );
            Ok(path.to_owned())
        }

        pub fn contains_path(&self, path: &Path) -> Result<bool> {
            Ok(normalize_path(path)?.starts_with(&self.root))
        }

        pub fn operation_in_progress(&self) -> bool {
            OPERATIONS
                .get_or_init(Default::default)
                .lock()
                .contains(&self.git_dir)
        }

        pub(super) fn begin_operation(&self) -> Result<OperationGuard> {
            let mut active = OPERATIONS.get_or_init(Default::default).lock();
            ensure!(
                active.insert(self.git_dir.clone()),
                "A Git operation is already running for this repository"
            );
            Ok(OperationGuard(self.git_dir.clone()))
        }

        pub(super) async fn head(&self) -> Result<(Option<String>, String)> {
            let head = self
                .run(
                    &args(&["rev-parse", "--verify", "--quiet", "HEAD^{commit}"]),
                    QUERY_TIMEOUT,
                )
                .await?;
            let id = if head.status.success() {
                Some(String::from_utf8(head.stdout)?.trim().to_owned())
            } else {
                ensure!(
                    head.status.code() == Some(1),
                    "Cannot resolve Git HEAD: {}",
                    String::from_utf8_lossy(&head.stderr)
                );
                None
            };
            let symbolic = self
                .run(
                    &args(&["symbolic-ref", "--quiet", "--short", "HEAD"]),
                    QUERY_TIMEOUT,
                )
                .await?;
            let name = if symbolic.status.success() {
                String::from_utf8_lossy(&symbolic.stdout).trim().to_owned()
            } else {
                ensure!(
                    symbolic.status.code() == Some(1),
                    "Cannot resolve Git branch: {}",
                    String::from_utf8_lossy(&symbolic.stderr)
                );
                id.as_deref()
                    .unwrap_or("unborn HEAD")
                    .chars()
                    .take(8)
                    .collect()
            };
            Ok((id, name))
        }

        async fn references(&self) -> Result<Vec<GitReference>> {
            let output = self
                .checked(&args(&[
                    "for-each-ref",
                    "--format=%(refname)%00%(objectname)%00%(*objectname)%00%(symref)%00%(HEAD)%00",
                    "refs/heads",
                    "refs/remotes",
                    "refs/tags",
                ]))
                .await?;
            let mut references = Vec::new();
            for row in output.split(|b| *b == b'\n').filter(|r| !r.is_empty()) {
                let fields: Vec<_> = row.split(|b| *b == 0).collect();
                ensure!(fields.len() >= 6, "Invalid Git reference record");
                if !fields[3].is_empty() {
                    continue;
                } // origin/HEAD is an alias.
                let full_name = String::from_utf8_lossy(fields[0]).into_owned();
                let (prefix, kind) = if full_name.starts_with("refs/heads/") {
                    ("refs/heads/", GitRefKind::Local)
                } else if full_name.starts_with("refs/remotes/") {
                    ("refs/remotes/", GitRefKind::Remote)
                } else {
                    ("refs/tags/", GitRefKind::Tag)
                };
                let name = full_name[prefix.len()..].to_owned();
                let id = if fields[2].is_empty() {
                    fields[1]
                } else {
                    fields[2]
                };
                references.push(GitReference {
                    full_name,
                    name,
                    id: String::from_utf8_lossy(id).into_owned(),
                    kind,
                    current: fields[4] == b"*",
                });
            }
            Ok(references)
        }

        async fn status(&self) -> Result<Vec<GitFile>> {
            let output = self
                .checked(&args(&[
                    "status",
                    "--porcelain=v1",
                    "-z",
                    "--untracked-files=all",
                ]))
                .await?;
            let mut fields = output.split(|b| *b == 0).filter(|s| !s.is_empty());
            let mut files = Vec::new();
            while let Some(field) = fields.next() {
                ensure!(
                    field.len() >= 4 && field[2] == b' ',
                    "Invalid Git status record"
                );
                let status = String::from_utf8_lossy(&field[..2]).into_owned();
                let path = path_from_bytes(&field[3..])?;
                let old_path = if status.contains(['R', 'C']) {
                    Some(path_from_bytes(
                        fields.next().context("Missing Git rename source")?,
                    )?)
                } else {
                    None
                };
                files.push(GitFile {
                    status,
                    path,
                    old_path,
                });
            }
            Ok(files)
        }

        pub async fn snapshot(&self, filter: &GitLogFilter) -> Result<GitSnapshot> {
            if let Some(revision) = &filter.revision {
                valid_revision(revision)?;
            }
            let ((head, head_name), references, status) =
                tokio::try_join!(self.head(), self.references(), self.status())?;
            let mut snapshot = GitSnapshot {
                head,
                head_name,
                references,
                status,
                ..Default::default()
            };
            if snapshot.head.is_none() && snapshot.references.is_empty() {
                return Ok(snapshot);
            }
            let limit = filter.limit.clamp(1, 5000);
            let mut revision = filter.revision.clone();
            let mut search_text = filter.text.as_str();
            // A short/full object id is a history jump, not a message grep.
            if (4..=64).contains(&filter.text.len())
                && filter.text.bytes().all(|b| b.is_ascii_hexdigit())
            {
                let object = format!("{}^{{commit}}", filter.text);
                let resolved = self
                    .run(
                        &args(&[
                            "rev-parse",
                            "--verify",
                            "--quiet",
                            "--end-of-options",
                            &object,
                        ]),
                        QUERY_TIMEOUT,
                    )
                    .await?;
                if resolved.status.success() {
                    let id = String::from_utf8_lossy(&resolved.stdout).trim().to_owned();
                    ensure!(valid_id(&id), "Invalid Git commit id");
                    revision = Some(id);
                    search_text = "";
                }
            }
            let mut command = args(&[
                "log",
                "--graph",
                "--topo-order",
                "--no-color",
                "--no-ext-diff",
                "--no-textconv",
                "--no-show-signature",
                "--no-notes",
                "--decorate=short",
                "--format=%x00%H%x00%P%x00%an%x00%aI%x00%s%x00%D%x00",
                "--regexp-ignore-case",
                "--fixed-strings",
            ]);
            command.push(format!("--max-count={}", limit + 1).into());
            if !search_text.is_empty() {
                command.push(format!("--grep={search_text}").into());
            }
            if !filter.author.is_empty() {
                command.push(format!("--author={}", filter.author).into());
            }
            if !filter.since.is_empty() {
                command.push(format!("--since={}", filter.since).into());
            }
            if !filter.until.is_empty() {
                command.push(format!("--until={}", filter.until).into());
            }
            if let Some(revision) = &revision {
                command.push(revision.into());
            } else {
                command.push("--all".into());
                if snapshot.head.is_some() {
                    command.push("HEAD".into());
                }
            }
            command.push("--".into());
            if let Some(path) = &filter.path {
                command.push(self.relative(path)?.into_os_string());
            }
            let output = self.checked(&command).await?;
            snapshot.commits = parse_log(&output)?;
            snapshot.has_more = snapshot.commits.len() > limit;
            snapshot.commits.truncate(limit);
            Ok(snapshot)
        }

        pub async fn details(&self, target: &GitTarget) -> Result<GitDetails> {
            let GitTarget::Commit(id) = target else {
                return Ok(GitDetails {
                    message: "Working tree vs HEAD (staged and unstaged changes)".into(),
                    files: self.status().await?,
                });
            };
            ensure!(valid_id(id), "Expected a full Git commit id");
            let message_args = args(&[
                "show",
                "-s",
                "--no-notes",
                "--no-show-signature",
                "--format=%B",
                id,
                "--",
            ]);
            let files_args = args(&[
                "show",
                "--format=",
                "--name-status",
                "-z",
                "--root",
                "--diff-merges=first-parent",
                "--find-renames",
                "--no-ext-diff",
                "--no-textconv",
                id,
                "--",
            ]);
            let (message, files) =
                tokio::try_join!(self.checked(&message_args), self.checked(&files_args))?;
            Ok(GitDetails {
                message: String::from_utf8_lossy(&message).trim_end().to_owned(),
                files: parse_files(&files)?,
            })
        }

        pub async fn patch(&self, target: &GitTarget, file: &GitFile) -> Result<String> {
            let path = self.relative(&file.path)?;
            let unborn = matches!(target, GitTarget::WorkingTree) && self.head().await?.0.is_none();
            if matches!(target, GitTarget::WorkingTree) && (file.status == "??" || unborn) {
                let path = self.root.join(path);
                let root = self.root.clone();
                return tokio::task::spawn_blocking(move || {
                    let metadata = match std::fs::symlink_metadata(&path) {
                        Ok(metadata) => metadata,
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                            return Ok(String::new())
                        }
                        Err(error) => return Err(error.into()),
                    };
                    if metadata.file_type().is_symlink() {
                        return Ok(format!(
                            "New symbolic link: {}",
                            std::fs::read_link(&path)?.display()
                        ));
                    }
                    ensure!(metadata.is_file(), "Not a regular file");
                    ensure!(
                        metadata.len() <= MAX_OUTPUT as u64,
                        "File exceeds the 16 MiB preview limit"
                    );
                    let resolved = gix::path::realpath(&path)?;
                    ensure!(resolved.starts_with(root), "File is outside Git worktree");
                    let bytes = std::fs::read(resolved)?;
                    if bytes.contains(&0) {
                        return Ok("Binary file (untracked)".into());
                    }
                    Ok(String::from_utf8_lossy(&bytes)
                        .lines()
                        .map(|l| format!("+{l}\n"))
                        .collect())
                })
                .await?;
            }
            let mut command = match target {
                GitTarget::Commit(id) => {
                    ensure!(valid_id(id), "Expected a full Git commit id");
                    args(&[
                        "show",
                        "--format=",
                        "--root",
                        "--diff-merges=first-parent",
                        "--find-renames",
                        "--no-ext-diff",
                        "--no-textconv",
                        "--no-color",
                        id,
                    ])
                }
                GitTarget::WorkingTree => {
                    let (head, _) = self.head().await?;
                    let mut command = args(&[
                        "diff",
                        "--find-renames",
                        "--no-ext-diff",
                        "--no-textconv",
                        "--no-color",
                    ]);
                    if head.is_some() {
                        command.push("HEAD".into());
                    } else {
                        command.push("--cached".into());
                    }
                    command
                }
            };
            command.push("--".into());
            if let Some(old_path) = &file.old_path {
                command.push(self.relative(old_path)?.into_os_string());
            }
            command.push(path.into_os_string());
            let output = self.checked(&command).await?;
            Ok(String::from_utf8_lossy(&output).into_owned())
        }

        pub async fn file_at(&self, commit: &str, path: &Path) -> Result<Vec<u8>> {
            ensure!(valid_id(commit), "Expected a full Git commit id");
            let path = self.relative(path)?;
            let mut object = OsString::from(format!("{commit}:"));
            // Git object paths always use '/' even on Windows.
            let mut first = true;
            for component in path.components() {
                if let Component::Normal(part) = component {
                    if !first {
                        object.push("/");
                    }
                    object.push(part);
                    first = false;
                }
            }
            self.checked(&[OsString::from("cat-file"), OsString::from("blob"), object])
                .await
        }

        pub async fn operate(&self, operation: &GitOperation) -> Result<String> {
            let _operation = self.begin_operation()?;
            let command = match operation {
                GitOperation::Fetch => args(&["fetch", "--all"]),
                GitOperation::Pull => args(&["pull", "--ff-only", "--no-autostash"]),
                GitOperation::Push => self.push_arguments(&self.push_plan().await?).await?,
                GitOperation::PushPlanned(plan) => self.push_arguments(plan).await?,
                GitOperation::Checkout { reference } => {
                    valid_revision(&reference.full_name)?;
                    match reference.kind {
                        GitRefKind::Local => args(&["switch", "--", &reference.name]),
                        GitRefKind::Tag => args(&["switch", "--detach", &reference.full_name]),
                        GitRefKind::Remote => {
                            let (_, local) = reference
                                .name
                                .split_once('/')
                                .context("Invalid remote branch")?;
                            args(&["switch", "--track", "-c", local, &reference.full_name])
                        }
                    }
                }
                GitOperation::NewBranch { name, start } => {
                    ensure!(
                        !name.is_empty() && !name.starts_with('-'),
                        "Invalid Git branch name"
                    );
                    self.checked(&args(&["check-ref-format", "--branch", name]))
                        .await?;
                    valid_revision(start)?;
                    args(&["switch", "-c", name, start])
                }
                GitOperation::Merge { reference } => {
                    valid_revision(reference)?;
                    args(&["merge", "--no-edit", "--no-autostash", reference])
                }
                GitOperation::CherryPick { commit } => {
                    ensure!(valid_id(commit), "Expected a full Git commit id");
                    args(&["cherry-pick", commit])
                }
            };
            let output = self.run(&command, OPERATION_TIMEOUT).await?;
            let message = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            ensure!(output.status.success(), "Git: {}", message.trim());
            Ok(message.trim().to_owned())
        }

        async fn config_value(&self, key: &str) -> Result<Option<String>> {
            let output = self
                .run(&args(&["config", "--get", key]), QUERY_TIMEOUT)
                .await?;
            if output.status.code() == Some(1) {
                return Ok(None);
            }
            ensure!(
                output.status.success(),
                "Read Git configuration: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            Ok(Some(String::from_utf8(output.stdout)?.trim().to_owned()))
        }

        pub async fn push_plan(&self) -> Result<GitPushPlan> {
            let branch = self
                .checked(&args(&["symbolic-ref", "--quiet", "--short", "HEAD"]))
                .await?;
            let branch = String::from_utf8(branch)?.trim().to_owned();
            let source_id = self
                .head()
                .await?
                .0
                .context("Cannot push an unborn branch")?;
            let tracking_remote = self
                .config_value(&format!("branch.{branch}.remote"))
                .await?;
            let remote = match self
                .config_value(&format!("branch.{branch}.pushRemote"))
                .await?
                .or(self.config_value("remote.pushDefault").await?)
                .or_else(|| tracking_remote.clone())
            {
                Some(remote) => remote,
                None => {
                    let remotes = self.checked(&args(&["remote"])).await?;
                    let remotes = String::from_utf8(remotes)?;
                    let names: Vec<_> = remotes.lines().collect();
                    if names.contains(&"origin") {
                        "origin".into()
                    } else {
                        ensure!(
                            names.len() == 1,
                            "Configure a push remote for the current branch"
                        );
                        names[0].to_owned()
                    }
                }
            };
            let destination = if tracking_remote.as_deref() == Some(remote.as_str()) {
                self.config_value(&format!("branch.{branch}.merge")).await?
            } else {
                None
            }
            .unwrap_or_else(|| format!("refs/heads/{branch}"));
            ensure!(
                destination.starts_with("refs/heads/") && !destination.contains(['\0', '\n', '\r']),
                "Invalid push destination"
            );
            Ok(GitPushPlan {
                local_branch: branch,
                source_id,
                remote,
                destination,
            })
        }

        async fn push_arguments(&self, plan: &GitPushPlan) -> Result<Vec<OsString>> {
            let (head, branch) = self.head().await?;
            ensure!(
                head.as_deref() == Some(plan.source_id.as_str()) && branch == plan.local_branch,
                "HEAD changed since push confirmation; review the push again"
            );
            ensure!(
                plan.destination.starts_with("refs/heads/")
                    && !plan.destination.contains(['\0', '\n', '\r']),
                "Invalid push destination"
            );
            // The confirmed destination is immutable, even if tracking
            // configuration changes while the confirmation is displayed.
            Ok(args(&[
                "push",
                "--no-force",
                "--no-follow-tags",
                "--set-upstream",
                "--",
                &plan.remote,
                &format!("refs/heads/{}:{}", plan.local_branch, plan.destination),
            ]))
        }
    }

    pub(super) fn args(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    fn parse_log(output: &[u8]) -> Result<Vec<GitCommit>> {
        let mut commits: Vec<GitCommit> = Vec::new();
        for row in output.split(|b| *b == b'\n').filter(|r| !r.is_empty()) {
            let fields: Vec<_> = row.split(|b| *b == 0).collect();
            if fields.len() == 1 {
                if let Some(last) = commits.last_mut() {
                    last.graph_after
                        .push(String::from_utf8_lossy(row).into_owned());
                }
                continue;
            }
            ensure!(fields.len() == 8, "Invalid Git log record");
            let value = |i: usize| String::from_utf8_lossy(fields[i]).into_owned();
            let id = value(1);
            ensure!(valid_id(&id), "Invalid commit id in Git log");
            commits.push(GitCommit {
                id,
                parents: value(2).split_whitespace().map(str::to_owned).collect(),
                author: value(3),
                date: value(4),
                subject: value(5),
                decoration: value(6),
                graph: value(0),
                graph_after: Vec::new(),
            });
        }
        Ok(commits)
    }

    fn parse_files(output: &[u8]) -> Result<Vec<GitFile>> {
        let mut fields = output.split(|b| *b == 0).filter(|s| !s.is_empty());
        let mut files = Vec::new();
        while let Some(status) = fields.next() {
            let status = String::from_utf8_lossy(status)
                .trim_start_matches('\n')
                .to_owned();
            let first = path_from_bytes(fields.next().context("Missing Git file path")?)?;
            let (path, old_path) = if status.starts_with(['R', 'C']) {
                (
                    path_from_bytes(fields.next().context("Missing Git rename destination")?)?,
                    Some(first),
                )
            } else {
                (first, None)
            };
            files.push(GitFile {
                status,
                path,
                old_path,
            });
        }
        Ok(files)
    }
}

#[cfg(not(feature = "git"))]
impl GitRepository {
    pub fn contains_path(&self, _path: &Path) -> anyhow::Result<bool> {
        anyhow::bail!("Git support was not compiled in")
    }
    pub fn operation_in_progress(&self) -> bool {
        false
    }
    pub async fn push_plan(&self) -> anyhow::Result<GitPushPlan> {
        anyhow::bail!("Git support was not compiled in")
    }
    pub async fn open(_path: PathBuf, _trust_full: bool) -> anyhow::Result<Self> {
        anyhow::bail!("Git support was not compiled in")
    }
    pub async fn snapshot(&self, _filter: &GitLogFilter) -> anyhow::Result<GitSnapshot> {
        anyhow::bail!("Git support was not compiled in")
    }
    pub async fn details(&self, _target: &GitTarget) -> anyhow::Result<GitDetails> {
        anyhow::bail!("Git support was not compiled in")
    }
    pub async fn patch(&self, _target: &GitTarget, _file: &GitFile) -> anyhow::Result<String> {
        anyhow::bail!("Git support was not compiled in")
    }
    pub async fn file_at(&self, _commit: &str, _path: &Path) -> anyhow::Result<Vec<u8>> {
        anyhow::bail!("Git support was not compiled in")
    }
    pub async fn operate(&self, _operation: &GitOperation) -> anyhow::Result<String> {
        anyhow::bail!("Git support was not compiled in")
    }
}

#[cfg(all(test, feature = "git"))]
#[path = "log/test.rs"]
mod test;
