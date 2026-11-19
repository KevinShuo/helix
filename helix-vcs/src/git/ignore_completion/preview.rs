//! On-demand preview of an edited ignore rule against untracked worktree paths.
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{ensure, Result};

pub use super::explain::Decision as Origin;
use super::explain::{Directory, Evaluation, Matcher};

const SCAN_LIMIT: usize = 16_384;
const DEPTH_LIMIT: usize = 64;
pub const CHANGED_SAMPLES: usize = 100;
pub const UNCHANGED_SAMPLES: usize = 50;

#[derive(Debug)]
pub struct Entry {
    /// Path relative to the worktree.
    pub path: PathBuf,
    pub before_ignored: bool,
    pub after_ignored: bool,
    /// The final rule, or `None` when the path is included by default.
    pub winner: Option<Arc<Origin>>,
}

#[derive(Debug)]
pub struct Report {
    pub root: PathBuf,
    /// Changed examples first, followed by examples with the same status.
    pub entries: Vec<Entry>,
    /// Some filesystem entries could not be examined.
    pub partial: bool,
    pub changed: usize,
    pub unchanged: usize,
}

/// Compare untracked files below `base` with and without the zero-based `line`
/// of its in-memory .gitignore. Cancellation discards the entire report.
pub fn scan(
    base: &Path,
    text: &str,
    line: usize,
    trust_full: bool,
    cancelled: impl Fn() -> bool,
) -> Result<Report> {
    scan_with_limit(
        base,
        text,
        line,
        trust_full,
        cancelled,
        SCAN_LIMIT,
        (CHANGED_SAMPLES, UNCHANGED_SAMPLES),
    )
}

/// Keep every match within the same bounded walk so an interactive preview
/// can apply its display limits after filtering, without rescanning the tree.
pub fn scan_all(
    base: &Path,
    text: &str,
    line: usize,
    trust_full: bool,
    cancelled: impl Fn() -> bool,
) -> Result<Report> {
    scan_with_limit(
        base,
        text,
        line,
        trust_full,
        cancelled,
        SCAN_LIMIT,
        (SCAN_LIMIT, SCAN_LIMIT),
    )
}

fn scan_with_limit(
    base: &Path,
    text: &str,
    line: usize,
    trust_full: bool,
    cancelled: impl Fn() -> bool,
    limit: usize,
    sample_limits: (usize, usize),
) -> Result<Report> {
    ensure!(!cancelled(), "ignore preview cancelled");
    ensure!(
        !base
            .components()
            .any(|part| part.as_os_str().eq_ignore_ascii_case(".git")),
        "cannot preview Git metadata"
    );
    let line = line
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("invalid rule line"))?;
    let (matcher, directory) = Matcher::load(base, text, line, trust_full, &cancelled)?;
    let mut walk = Walk {
        report: Report {
            root: matcher.root().to_owned(),
            entries: Vec::new(),
            partial: false,
            changed: 0,
            unchanged: 0,
        },
        matcher,
        unchanged: Vec::new(),
        remaining: limit,
        sample_limits,
        cancelled,
    };
    walk.visit(base, &directory, 0)?;
    walk.report.entries.append(&mut walk.unchanged);
    Ok(walk.report)
}

struct Walk<F> {
    matcher: Matcher,
    report: Report,
    unchanged: Vec<Entry>,
    remaining: usize,
    sample_limits: (usize, usize),
    cancelled: F,
}

fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0 // FILE_ATTRIBUTE_REPARSE_POINT
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

pub(super) fn validate_target(root: &Path, base: &Path) -> Result<()> {
    // The editor preserves lexical paths. Git ignores symlinks to ignore files
    // and treats links to directories inside the worktree as leaves.
    for directory in base.ancestors().take_while(|&path| path != root) {
        ensure!(
            !is_link(&fs::symlink_metadata(directory)?),
            "Git does not traverse linked directories: {}",
            directory.display()
        );
    }
    match fs::symlink_metadata(base.join(".gitignore")) {
        Ok(meta) => ensure!(!is_link(&meta), "Git does not read a linked .gitignore"),
        Err(error) if error.kind() == io::ErrorKind::NotFound => (),
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

impl<F: Fn() -> bool> Walk<F> {
    fn visit(&mut self, path: &Path, directory: &Directory, depth: usize) -> Result<()> {
        let Ok(mut children) = fs::read_dir(path) else {
            self.report.partial = true;
            return Ok(());
        };
        let mut entries = Vec::new();
        for item in children.by_ref().take(self.remaining) {
            ensure!(!(self.cancelled)(), "ignore preview cancelled");
            self.remaining -= 1;
            match item {
                Ok(item) if !item.file_name().eq_ignore_ascii_case(".git") => entries.push(item),
                Ok(_) => (),
                Err(_) => self.report.partial = true,
            }
        }
        self.report.partial |= children.next().is_some();
        entries.sort_by_cached_key(|entry| entry.file_name());
        for entry in entries {
            ensure!(!(self.cancelled)(), "ignore preview cancelled");
            let path = entry.path();
            match self.inspect(entry, &path, directory, depth) {
                Ok(Some(child)) => self.visit(&path, &child, depth + 1)?,
                Ok(None) => (),
                Err(_) => self.report.partial = true,
            }
        }
        Ok(())
    }

    fn inspect(
        &mut self,
        entry: fs::DirEntry,
        path: &Path,
        directory: &Directory,
        depth: usize,
    ) -> Result<Option<Directory>> {
        let kind = entry.file_type()?;
        #[cfg(windows)]
        let link = is_link(&entry.metadata()?);
        #[cfg(not(windows))]
        let link = kind.is_symlink();

        let is_directory = kind.is_dir() && !link;
        if self.matcher.tracked(path, is_directory)? {
            return Ok(None);
        }
        if is_directory {
            if nested_repository(path)? {
                return Ok(None);
            }
            if depth == DEPTH_LIMIT || self.remaining == 0 {
                self.report.partial = true;
                return Ok(None);
            }
            return Ok(Some(self.matcher.enter(directory, path)?));
        }
        if kind.is_file() || link {
            let evaluation = self.matcher.evaluate(directory, path)?;
            self.record(path, evaluation)?;
        }
        Ok(None)
    }

    fn record(&mut self, path: &Path, evaluation: Evaluation) -> Result<()> {
        let before_ignored = evaluation.before.as_ref().is_some_and(|d| d.ignored);
        let after_ignored = evaluation.after.as_ref().is_some_and(|d| d.ignored);
        let (examples, limit) = if before_ignored != after_ignored {
            self.report.changed += 1;
            (&mut self.report.entries, self.sample_limits.0)
        } else if evaluation.selected {
            self.report.unchanged += 1;
            (&mut self.unchanged, self.sample_limits.1)
        } else {
            return Ok(());
        };
        if examples.len() < limit {
            examples.push(Entry {
                path: path.strip_prefix(&self.report.root)?.to_owned(),
                before_ignored,
                after_ignored,
                winner: evaluation.after,
            });
        }
        Ok(())
    }
}

fn nested_repository(path: &Path) -> io::Result<bool> {
    fn metadata(path: PathBuf) -> io::Result<Option<fs::Metadata>> {
        match fs::symlink_metadata(path) {
            Ok(metadata) => Ok(Some(metadata)),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err),
        }
    }
    // Gitlinks in the index were already handled. Look for worktrees (including
    // .git files) and for bare repositories without reading their contents.
    Ok(metadata(path.join(".git"))?.is_some()
        || (metadata(path.join("HEAD"))?.is_some_and(|m| m.is_file())
            && metadata(path.join("objects"))?.is_some_and(|m| m.is_dir())
            && metadata(path.join("refs"))?.is_some_and(|m| m.is_dir())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, process::Command};

    fn git(dir: &Path, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn repository() -> anyhow::Result<tempfile::TempDir> {
        let temp = tempfile::tempdir()?;
        git(temp.path(), &["init", "-q"]);
        git(temp.path(), &["config", "core.excludesFile", ""]);
        Ok(temp)
    }

    #[test]
    fn scans_only_untracked_files_in_scope_and_shows_the_final_rule() -> anyhow::Result<()> {
        let temp = repository()?;
        let root = temp.path();
        let base = root.join("app");
        fs::create_dir_all(base.join("cache"))?;
        fs::create_dir_all(base.join("vendor"))?;
        fs::create_dir_all(base.join("module"))?;
        git(&base.join("vendor"), &["init", "-q"]);
        git(&base, &["init", "--bare", "-q", "mirror.git"]);
        fs::write(base.join("mirror.git/in-mirror.log"), "")?;
        fs::write(base.join("module/in-module.log"), "")?;
        // A gitlink remains a boundary even without a populated submodule's .git marker.
        git(
            root,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                "160000,1111111111111111111111111111111111111111,app/module",
            ],
        );
        for name in [
            "outside.log",
            "app/vendor/in-vendor.log",
            "app/tracked.log",
            "app/cache/prior.log",
            "app/sp ace[中]!.log",
            "app/keep.log",
            "app/no.txt",
        ] {
            fs::write(root.join(name), "")?;
        }
        git(root, &["add", "app/tracked.log"]);
        fs::write(root.join(".gitignore"), "app/cache/\n")?;
        let report = scan(&base, "*.log\n!keep.log\n", 0, true, || false)?;
        assert!(!report.partial);
        assert_eq!((report.changed, report.unchanged), (1, 2));
        assert_eq!(report.entries.len(), 3);
        let changed = &report.entries[0];
        assert_eq!(changed.path, Path::new("app/sp ace[中]!.log"));
        assert!(!changed.before_ignored && changed.after_ignored);
        let winner = changed.winner.as_ref().unwrap();
        assert_eq!(winner.source, Path::new("app/.gitignore"));
        assert_eq!((winner.pattern.as_str(), winner.line), ("*.log", 1));
        let prior = report
            .entries
            .iter()
            .find(|e| e.path == Path::new("app/cache/prior.log"))
            .unwrap();
        assert!(prior.before_ignored && prior.after_ignored);
        assert_eq!(
            prior.winner.as_ref().unwrap().source,
            Path::new(".gitignore")
        );
        let keep = report
            .entries
            .iter()
            .find(|e| e.path == Path::new("app/keep.log"))
            .unwrap();
        assert!(!keep.before_ignored && !keep.after_ignored);
        assert_eq!(keep.winner.as_ref().unwrap().line, 2);
        Ok(())
    }

    #[test]
    fn counts_beyond_display_limits_and_preserves_changed_examples() -> anyhow::Result<()> {
        let temp = repository()?;
        let root = temp.path();
        for i in 0..UNCHANGED_SAMPLES + 2 {
            fs::write(root.join(format!("a{i:03}.log")), "")?;
        }
        fs::write(root.join("z.log"), "")?;
        let report = scan(root, "*.log\na*.log\n", 0, true, || false)?;
        assert!(!report.partial);
        assert_eq!(
            (report.changed, report.unchanged),
            (1, UNCHANGED_SAMPLES + 2)
        );
        assert_eq!(report.entries.len(), 1 + UNCHANGED_SAMPLES);
        assert_eq!(report.entries[0].path, Path::new("z.log"));
        let all = scan_all(root, "*.log\na*.log\n", 0, true, || false)?;
        assert_eq!(all.entries.len(), all.changed + all.unchanged);
        assert_eq!(all.entries[all.changed].path, Path::new("a000.log"));
        assert_eq!(all.entries.last().unwrap().path, Path::new("a051.log"));
        Ok(())
    }

    #[test]
    fn truncated_scans_and_cancellation_are_explicit() -> anyhow::Result<()> {
        let temp = repository()?;
        let base = temp.path().join("src");
        fs::create_dir(&base)?;
        for file in ["1", "2", "3", "4", "5"] {
            fs::write(base.join(file), "")?;
        }
        let report = scan_with_limit(&base, "*", 0, true, || false, 3, (3, 3))?;
        assert!(report.partial);
        assert_eq!(report.changed, 3);
        let report = scan_with_limit(&base, "*", 0, true, || false, 5, (5, 5))?;
        assert!(!report.partial);
        let count = Cell::new(0);
        assert!(scan(&base, "*", 0, true, || {
            count.set(count.get() + 1);
            count.get() > 2
        })
        .is_err());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn preserves_odd_path_bytes_and_treats_symlinks_as_leaves() -> anyhow::Result<()> {
        use std::{
            ffi::OsStr,
            os::unix::{ffi::OsStrExt, fs::symlink},
        };
        let temp = repository()?;
        let root = temp.path();
        let external = tempfile::tempdir()?;
        fs::write(external.path().join("outside"), "")?;
        symlink(external.path(), root.join("link"))?;
        let odd = Path::new(OsStr::from_bytes(b"nonutf8_\xff\n"));
        fs::write(root.join(odd), "")?;
        let report = scan(root, "*", 0, true, || false)?;
        assert!(!report.partial);
        assert_eq!(report.changed, 2);
        assert!(report.entries.iter().any(|e| e.path == odd));
        assert!(report.entries.iter().any(|e| e.path == Path::new("link")));
        symlink(odd, root.join(".gitignore"))?;
        assert!(scan(root, "*", 0, true, || false).is_err());
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn directory_junctions_are_leaves() -> anyhow::Result<()> {
        let temp = repository()?;
        let root = temp.path();
        let base = root.join("scope");
        fs::create_dir(&base)?;
        fs::create_dir(root.join("outside"))?;
        fs::write(root.join("outside/unreachable"), "")?;
        let output = Command::new("cmd")
            .current_dir(&base)
            .args(["/d", "/c", "mklink", "/j", "link", "..\\outside"])
            .output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report = scan(&base, "*", 0, true, || false)?;
        assert!(!report.partial);
        assert_eq!(report.changed, 1);
        assert_eq!(report.entries[0].path, Path::new("scope/link"));
        assert!(scan(&base.join("link"), "*", 0, true, || false).is_err());
        Ok(())
    }
}
