use std::{ops::Range, path::PathBuf};

/// The author of the latest committed change to a function.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FunctionAuthor {
    pub author: String,
    pub commit: String,
    /// Whether the function also contains changes absent from HEAD.
    pub modified: bool,
}

/// Find the latest authors for the requested current-buffer line ranges.
///
/// Ranges are zero-based, with an exclusive end. New functions, untracked files,
/// and ranges that cannot be mapped unambiguously to HEAD return `None`. This
/// operation requires a trusted workspace, because it invokes Git in the repository.
pub async fn function_authors(
    file: PathBuf,
    contents: String,
    ranges: Vec<Range<usize>>,
    trust_full: bool,
) -> anyhow::Result<Vec<Option<FunctionAuthor>>> {
    if !trust_full {
        anyhow::bail!("Function author lookup requires a trusted workspace")
    }
    #[cfg(feature = "git")]
    {
        use anyhow::Context;
        tokio::time::timeout(
            std::time::Duration::from_secs(15),
            git::function_authors(file, contents, ranges),
        )
        .await
        .context("Git function author lookup timed out")?
    }
    #[cfg(not(feature = "git"))]
    {
        let _ = (file, contents, ranges);
        anyhow::bail!("Git support was not compiled in")
    }
}

#[cfg(feature = "git")]
mod git {
    use std::{
        ffi::{OsStr, OsString},
        path::Path,
        process::{Output, Stdio},
        time::Duration,
    };

    use anyhow::{ensure, Context, Result};
    use imara_diff::{Algorithm, Diff, Hunk, InternedInput};
    use tokio::process::Command;

    use super::*;

    const MAX_FILE_BYTES: usize = 16 * 1024 * 1024;

    struct Repository {
        git_dir: PathBuf,
        work_dir: PathBuf,
        relative_file: OsString,
    }

    impl Repository {
        fn discover(file: &Path) -> Result<Self> {
            ensure!(
                file.is_absolute(),
                "Function author lookup requires an absolute path"
            );
            // Resolve symlinks before discovering the repository; never follow a
            // lexical `..` or a link into a different worktree with this repo's HEAD.
            let file = gix::path::realpath(file).context("resolve function source path")?;
            let directory = file.parent().context("file has no parent directory")?;
            let (repository, _) = gix::discover::upwards_opts(
                directory,
                gix::discover::upwards::Options {
                    dot_git_only: true,
                    ..Default::default()
                },
            )
            .context("discover function source repository")?;
            let (git_dir, work_dir) = repository.into_repository_and_work_tree_directories();
            let work_dir = gix::path::realpath(work_dir.context("repository has no worktree")?)?;
            let relative = file
                .strip_prefix(&work_dir)
                .context("file is outside worktree")?;
            let mut relative_file = OsString::new();
            for component in relative.components() {
                ensure!(
                    matches!(component, std::path::Component::Normal(_)),
                    "invalid relative file path"
                );
                if !relative_file.is_empty() {
                    relative_file.push("/");
                }
                relative_file.push(component.as_os_str());
            }
            ensure!(!relative_file.is_empty(), "file is a worktree directory");
            Ok(Self {
                git_dir,
                work_dir,
                relative_file,
            })
        }

        async fn run(&self, args: &[&OsStr]) -> Result<Output> {
            let mut command = Command::new("git");
            command
                .args(["--no-pager", "--no-replace-objects", "--literal-pathspecs"])
                .arg("--git-dir")
                .arg(&self.git_dir)
                .arg("--work-tree")
                .arg(&self.work_dir)
                .args([
                    "-c",
                    "core.fsmonitor=false",
                    "-c",
                    "color.ui=false",
                    "-c",
                    "i18n.logOutputEncoding=UTF-8",
                ])
                .args(args)
                .current_dir(&self.work_dir)
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .env_remove("GIT_COMMON_DIR")
                .env_remove("GIT_INDEX_FILE")
                .env_remove("GIT_OBJECT_DIRECTORY")
                .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
                .env_remove("GIT_NAMESPACE")
                .env("GIT_TERMINAL_PROMPT", "0")
                .env("GIT_NO_LAZY_FETCH", "1")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true);
            #[cfg(windows)]
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW
            tokio::time::timeout(Duration::from_secs(5), command.output())
                .await
                .context("Git command timed out")?
                .context("run Git for function author lookup")
        }

        async fn checked(&self, args: &[&OsStr]) -> Result<Vec<u8>> {
            let output = self.run(args).await?;
            ensure!(
                output.status.success(),
                "Git function author lookup failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
            Ok(output.stdout)
        }
    }

    pub(super) async fn function_authors(
        file: PathBuf,
        contents: String,
        ranges: Vec<Range<usize>>,
    ) -> Result<Vec<Option<FunctionAuthor>>> {
        if ranges.is_empty() {
            return Ok(Vec::new());
        }
        ensure!(
            contents.len() <= MAX_FILE_BYTES,
            "file is too large for function author lookup"
        );
        let repository = tokio::task::spawn_blocking(move || Repository::discover(&file)).await??;
        let head = repository
            .run(&[
                "rev-parse".as_ref(),
                "--verify".as_ref(),
                "--quiet".as_ref(),
                "HEAD^{commit}".as_ref(),
            ])
            .await?;
        if head.status.code() == Some(1) {
            return Ok(vec![None; ranges.len()]); // Unborn branch.
        }
        ensure!(
            head.status.success(),
            "resolve HEAD: {}",
            String::from_utf8_lossy(&head.stderr).trim()
        );
        let head = std::str::from_utf8(&head.stdout)?.trim();
        ensure!(
            !head.is_empty() && head.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid Git commit id"
        );
        let entry = repository
            .checked(&[
                "ls-tree".as_ref(),
                "--full-tree".as_ref(),
                "-lz".as_ref(),
                head.as_ref(),
                "--".as_ref(),
                &repository.relative_file,
            ])
            .await?;
        if entry.is_empty() {
            return Ok(vec![None; ranges.len()]);
        }
        let metadata = entry
            .split(|byte| *byte == b'\t')
            .next()
            .context("missing Git tree metadata")?;
        let metadata = std::str::from_utf8(metadata)?
            .split_ascii_whitespace()
            .collect::<Vec<_>>();
        ensure!(
            metadata.len() == 4
                && matches!(metadata[0], "100644" | "100755")
                && metadata[1] == "blob",
            "function source is not a regular tracked file"
        );
        ensure!(
            metadata[3].parse::<usize>()? <= MAX_FILE_BYTES,
            "HEAD file is too large for function author lookup"
        );
        let base = repository
            .checked(&["cat-file".as_ref(), "blob".as_ref(), metadata[2].as_ref()])
            .await?;
        let base = String::from_utf8(base).context("HEAD file is not UTF-8")?;
        let mapped =
            tokio::task::spawn_blocking(move || map_ranges(&base, &contents, ranges)).await?;

        let mut authors = Vec::with_capacity(mapped.len());
        for mapped_range in mapped {
            let Some((range, modified)) = mapped_range else {
                authors.push(None);
                continue;
            };
            // `-L` follows changes to the whole function, including deletions.
            // Choosing the newest surviving blame line would miss those changes.
            let mut lines = OsString::from(format!("-L{},{}:", range.start + 1, range.end));
            lines.push(&repository.relative_file);
            let result = repository
                .checked(&[
                    "log".as_ref(),
                    "-1".as_ref(),
                    "--format=%H%x00%an%x00".as_ref(),
                    "--no-patch".as_ref(),
                    "--no-ext-diff".as_ref(),
                    "--no-textconv".as_ref(),
                    "--no-show-signature".as_ref(),
                    "--no-notes".as_ref(),
                    "--no-decorate".as_ref(),
                    &lines,
                    head.as_ref(),
                ])
                .await?;
            if result.is_empty() {
                authors.push(None);
                continue;
            }
            let mut fields = result.split(|byte| *byte == 0);
            let commit = std::str::from_utf8(fields.next().context("missing commit id")?)?;
            let author = std::str::from_utf8(fields.next().context("missing commit author")?)?;
            if author.is_empty() {
                authors.push(None);
            } else {
                authors.push(Some(FunctionAuthor {
                    author: author.to_owned(),
                    commit: commit.to_owned(),
                    modified,
                }));
            }
        }
        Ok(authors)
    }

    /// Map only the requested ranges, using unchanged spans as line anchors.
    /// Replaced spans retain their committed extent; purely inserted functions
    /// have no extent in HEAD and therefore have no committed author.
    fn map_ranges(
        base: &str,
        contents: &str,
        ranges: Vec<Range<usize>>,
    ) -> Vec<Option<(Range<usize>, bool)>> {
        // `lines()` normalizes CRLF without executing clean/smudge filters.
        let mut input = InternedInput::default();
        input.update_before(base.lines());
        input.update_after(contents.lines());
        let mut diff = Diff::compute(Algorithm::Histogram, &input);
        diff.postprocess_lines(&input);
        let hunks = diff.hunks().collect::<Vec<_>>();
        ranges
            .into_iter()
            .map(|range| {
                let range = range.start..range.end.min(input.after.len());
                if range.is_empty() {
                    return None;
                }
                let mut start = usize::MAX;
                let mut end = 0;
                let mut modified = false;
                let mut partial_replacement = false;
                let mut before = 0;
                let mut after = 0;
                // The sentinel includes the unchanged tail in the same loop.
                let sentinel = Hunk {
                    before: input.before.len() as u32..input.before.len() as u32,
                    after: input.after.len() as u32..input.after.len() as u32,
                };
                for hunk in hunks.iter().chain(std::iter::once(&sentinel)) {
                    let unchanged_start = range.start.max(after);
                    let unchanged_end = range.end.min(hunk.after.start as usize);
                    if unchanged_start < unchanged_end {
                        start = start.min(before + unchanged_start - after);
                        end = end.max(before + unchanged_end - after);
                    }
                    let changed = range.start < hunk.after.end as usize
                        && (hunk.after.start as usize) < range.end;
                    let deleted_inside = hunk.after.is_empty()
                        && range.start < hunk.after.start as usize
                        && (hunk.after.start as usize) < range.end;
                    if changed || deleted_inside {
                        modified = true;
                        if !hunk.before.is_empty() {
                            partial_replacement |= range.start > hunk.after.start as usize
                                || range.end < hunk.after.end as usize;
                            start = start.min(hunk.before.start as usize);
                            end = end.max(hunk.before.end as usize);
                        }
                    }
                    before = hunk.before.end as usize;
                    after = hunk.after.end as usize;
                }
                // When a replacement spans a function boundary, its old extent
                // cannot be assigned to this function reliably. Do not borrow
                // an adjacent function's author for newly inserted code.
                (start < end && !partial_replacement).then_some((start..end, modified))
            })
            .collect()
    }
}

#[cfg(all(test, feature = "git"))]
mod tests {
    use std::{fs, path::Path, process::Command};

    use tempfile::TempDir;

    use super::*;

    const ORIGINAL: &str =
        "fn first() {\n    keep();\n    remove();\n}\n\nfn second() {\n    other();\n}\n";

    struct Repo(TempDir);

    impl Repo {
        fn new() -> Self {
            let repo = Self(tempfile::tempdir().unwrap());
            repo.git(&["init", "--initial-branch=main"], "Alice");
            repo.git(&["config", "core.autocrlf", "false"], "Alice");
            repo
        }

        fn path(&self) -> &Path {
            self.0.path()
        }

        fn git(&self, args: &[&str], author: &str) -> String {
            let mut command = Command::new("git");
            command
                .args(["-c", "commit.gpgsign=false", "-c", "core.hooksPath="])
                .args(args)
                .current_dir(self.path())
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .env_remove("GIT_INDEX_FILE")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env(
                    "GIT_CONFIG_GLOBAL",
                    if cfg!(windows) { "NUL" } else { "/dev/null" },
                )
                .env("GIT_AUTHOR_NAME", author)
                .env("GIT_AUTHOR_EMAIL", "author@example.com")
                .env("GIT_COMMITTER_NAME", author)
                .env("GIT_COMMITTER_EMAIL", "author@example.com")
                .env("GIT_AUTHOR_DATE", "2000-01-01T00:00:00+0000")
                .env("GIT_COMMITTER_DATE", "2000-01-01T00:00:00+0000");
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x08000000);
            }
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout).unwrap().trim().to_owned()
        }

        fn commit(&self, name: &str, contents: &str, author: &str) -> String {
            fs::write(self.path().join(name), contents).unwrap();
            self.git(&["add", "--", name], author);
            self.git(&["commit", "-m", "change function"], author);
            self.git(&["rev-parse", "HEAD"], author)
        }

        async fn authors(
            &self,
            name: &str,
            contents: &str,
            ranges: Vec<Range<usize>>,
        ) -> Vec<Option<FunctionAuthor>> {
            function_authors(self.path().join(name), contents.to_owned(), ranges, true)
                .await
                .unwrap()
        }
    }

    #[tokio::test]
    async fn latest_body_change_wins_over_majority_author() {
        let repo = Repo::new();
        let first = repo.commit("code.rs", ORIGINAL, "Alice");
        let changed = ORIGINAL.replace("remove();", "replacement();");
        let latest = repo.commit("code.rs", &changed, "Bob");
        let authors = repo.authors("code.rs", &changed, vec![0..4, 5..8]).await;
        assert_eq!(
            authors,
            vec![
                Some(FunctionAuthor {
                    author: "Bob".into(),
                    commit: latest,
                    modified: false
                }),
                Some(FunctionAuthor {
                    author: "Alice".into(),
                    commit: first,
                    modified: false
                }),
            ]
        );
    }

    #[tokio::test]
    async fn deletion_only_commit_is_the_latest_change() {
        let repo = Repo::new();
        repo.commit("code.rs", ORIGINAL, "Alice");
        let changed = ORIGINAL.replace("    remove();\n", "");
        let latest = repo.commit("code.rs", &changed, "Bob");
        let authors = repo.authors("code.rs", &changed, vec![0..3]).await;
        assert_eq!(
            authors,
            vec![Some(FunctionAuthor {
                author: "Bob".into(),
                commit: latest,
                modified: false
            })]
        );
    }

    #[tokio::test]
    async fn inserted_lines_before_function_do_not_mark_it_modified() {
        let repo = Repo::new();
        let commit = repo.commit("code.rs", ORIGINAL, "Alice");
        let current = format!("// unsaved header\n\n{ORIGINAL}");
        let authors = repo.authors("code.rs", &current, vec![2..6, 7..10]).await;
        assert_eq!(
            authors,
            vec![
                Some(FunctionAuthor {
                    author: "Alice".into(),
                    commit,
                    modified: false
                });
                2
            ]
        );
    }

    #[tokio::test]
    async fn dirty_function_retains_committed_author_with_modified_flag() {
        let repo = Repo::new();
        let commit = repo.commit("code.rs", ORIGINAL, "Alice");
        let current = ORIGINAL.replace("remove();", "unsaved();");
        let authors = repo.authors("code.rs", &current, vec![0..4, 5..8]).await;
        assert_eq!(
            authors[0],
            Some(FunctionAuthor {
                author: "Alice".into(),
                commit: commit.clone(),
                modified: true
            })
        );
        assert_eq!(
            authors[1],
            Some(FunctionAuthor {
                author: "Alice".into(),
                commit,
                modified: false
            })
        );
    }

    #[tokio::test]
    async fn dirty_deletion_marks_function_modified() {
        let repo = Repo::new();
        repo.commit("code.rs", ORIGINAL, "Alice");
        let current = ORIGINAL.replace("    remove();\n", "");
        let authors = repo.authors("code.rs", &current, vec![0..3]).await;
        assert!(authors[0].as_ref().unwrap().modified);
    }

    #[tokio::test]
    async fn wholly_inserted_function_has_no_committed_author() {
        let repo = Repo::new();
        repo.commit("code.rs", ORIGINAL, "Alice");
        let current = format!("{ORIGINAL}\nfn added() {{\n    new_body();\n}}\n");
        let authors = repo.authors("code.rs", &current, vec![9..12]).await;
        assert_eq!(authors, vec![None]);
    }

    #[tokio::test]
    async fn inserted_function_before_existing_function_has_no_author() {
        let repo = Repo::new();
        let commit = repo.commit("code.rs", ORIGINAL, "Alice");
        let current = format!("fn added() {{\n    new_body();\n}}\n\n{ORIGINAL}");
        let authors = repo.authors("code.rs", &current, vec![0..3, 4..8]).await;
        assert_eq!(
            authors,
            vec![
                None,
                Some(FunctionAuthor {
                    author: "Alice".into(),
                    commit,
                    modified: false,
                })
            ]
        );
    }

    #[tokio::test]
    async fn new_function_inside_a_larger_replacement_has_no_author() {
        let repo = Repo::new();
        repo.commit("code.rs", "fn old() {\n    old_body();\n}\n", "Alice");
        for (current, added_range) in [
            (
                "fn added() {\n    new_body();\n}\n\nfn renamed() {\n    edited_body();\n}\n",
                0..3,
            ),
            (
                "fn renamed() {\n    edited_body();\n}\n\nfn added() {\n    new_body();\n}\n",
                4..7,
            ),
        ] {
            let authors = repo.authors("code.rs", current, vec![added_range]).await;
            assert_eq!(authors, vec![None]);
        }
    }

    #[tokio::test]
    async fn crlf_checkout_does_not_mark_functions_modified() {
        let repo = Repo::new();
        repo.commit("code.rs", ORIGINAL, "Alice");
        let current = ORIGINAL.replace('\n', "\r\n");
        let authors = repo.authors("code.rs", &current, vec![0..4]).await;
        assert!(!authors[0].as_ref().unwrap().modified);
    }

    #[tokio::test]
    async fn unborn_repository_has_no_committed_author() {
        let repo = Repo::new();
        assert_eq!(
            repo.authors("new.rs", ORIGINAL, vec![0..4]).await,
            vec![None]
        );
    }

    #[tokio::test]
    async fn untracked_file_has_no_committed_author() {
        let repo = Repo::new();
        repo.commit("tracked.rs", ORIGINAL, "Alice");
        fs::write(repo.path().join("untracked.rs"), ORIGINAL).unwrap();
        assert_eq!(
            repo.authors("untracked.rs", ORIGINAL, vec![0..4]).await,
            vec![None]
        );
    }

    #[tokio::test]
    async fn unsaved_new_file_has_no_committed_author() {
        let repo = Repo::new();
        repo.commit("tracked.rs", ORIGINAL, "Alice");
        assert_eq!(
            repo.authors("new.rs", ORIGINAL, vec![0..4]).await,
            vec![None]
        );
    }

    #[tokio::test]
    async fn outside_repository_returns_error() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("code.rs");
        fs::write(&file, ORIGINAL).unwrap();
        assert!(function_authors(file, ORIGINAL.into(), vec![0..4], true)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn untrusted_workspace_is_refused_before_reading_git_config() {
        let repo = Repo::new();
        fs::write(
            repo.path().join(".git/config"),
            "not a valid configuration [\n",
        )
        .unwrap();
        let error = function_authors(
            repo.path().join("code.rs"),
            ORIGINAL.into(),
            vec![0..4],
            false,
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("trusted"), "{error:#}");
    }

    #[tokio::test]
    async fn paths_with_spaces_and_non_ascii_are_literal() {
        let repo = Repo::new();
        let name = "--code 空 白.rs";
        repo.commit(name, ORIGINAL, "作者 Alice");
        assert_eq!(
            repo.authors(name, ORIGINAL, vec![0..4]).await[0]
                .as_ref()
                .unwrap()
                .author,
            "作者 Alice"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn paths_with_colons_are_literal() {
        let repo = Repo::new();
        let name = "code:name.rs";
        repo.commit(name, ORIGINAL, "Alice");
        assert_eq!(
            repo.authors(name, ORIGINAL, vec![0..4]).await[0]
                .as_ref()
                .unwrap()
                .author,
            "Alice"
        );
    }

    #[tokio::test]
    async fn external_diff_and_textconv_are_disabled() {
        let repo = Repo::new();
        repo.commit("code.rs", ORIGINAL, "Alice");
        fs::write(repo.path().join(".gitattributes"), "*.rs diff=tripwire\n").unwrap();
        repo.git(
            &["config", "diff.external", "helix-must-never-run"],
            "Alice",
        );
        repo.git(
            &["config", "diff.tripwire.textconv", "helix-must-never-run"],
            "Alice",
        );
        assert_eq!(
            repo.authors("code.rs", ORIGINAL, vec![0..4]).await[0]
                .as_ref()
                .unwrap()
                .author,
            "Alice"
        );
    }
}
