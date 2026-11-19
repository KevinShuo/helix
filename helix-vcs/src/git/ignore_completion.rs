//! Read-only Git evidence for completing an individual ignore rule. Only filesystem
//! data goes in the cache; each completion evaluates the current buffer separately.
use std::{
    borrow::Cow,
    collections::HashSet,
    fs, io,
    io::Read,
    path::{Path, PathBuf},
};

use gix::{
    bstr::{BStr, ByteSlice},
    ignore::{glob::pattern::Case, Search},
};

mod explain;
pub mod preview;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    Changed,
    Unchanged,
    Overridden,
    ParentExcluded,
    Unknown,
}

/// Index and ignore data for the direct children of one sampled directory.
#[derive(Default)]
pub struct Sources(Option<RepositorySources>);

struct RepositorySources {
    root: PathBuf,
    document: PathBuf,
    directory: PathBuf,
    case: Case,
    tracked: Option<HashSet<Vec<u8>>>,
    layers: Option<(Search, Search)>,
}

fn git_path(path: &Path) -> Option<Cow<'_, BStr>> {
    Some(gix::path::to_unix_separators_on_windows(
        gix::path::try_into_bstr(path).ok()?,
    ))
}

impl Sources {
    pub fn load(base: &Path, directory: &Path, trust_full: bool) -> Self {
        Self(RepositorySources::load(base, directory, trust_full).ok())
    }

    pub fn rules(&self, before: &str, after: &str) -> Rules<'_> {
        let mut preceding = Search::default();
        let mut following = Search::default();
        if let Some(repo) = &self.0 {
            for (text, search) in [(before, &mut preceding), (after, &mut following)] {
                search.add_patterns_buffer(
                    text.as_bytes(),
                    &repo.document,
                    Some(&repo.root),
                    Default::default(),
                );
            }
        }
        Rules {
            repo: self.0.as_ref(),
            preceding,
            following,
        }
    }
}

impl RepositorySources {
    fn load(base: &Path, directory: &Path, trust_full: bool) -> anyhow::Result<Self> {
        let repo = super::open_repo(base, trust_full)?.to_thread_local();
        let root = worktree_root(&repo, base)?;
        directory.strip_prefix(base)?;
        let case = ignore_case(&repo);
        let tracked = tracked_names(&repo, directory.strip_prefix(root)?, case);
        let layers = Self::layers(&repo, root, base, directory).ok();
        Ok(Self {
            root: root.to_owned(),
            document: base.join(".gitignore"),
            directory: directory.to_owned(),
            case,
            tracked,
            layers,
        })
    }

    fn layers(
        repo: &gix::Repository,
        root: &Path,
        base: &Path,
        directory: &Path,
    ) -> anyhow::Result<(Search, Search)> {
        let mut lower = Search::default();
        let mut higher = Search::default();
        if let Some(global) = global_excludes(repo, root)? {
            add_file(&mut lower, &global, None)?;
        }
        add_file(&mut lower, &repo.common_dir().join("info/exclude"), None)?;
        let mut current = root.to_owned();
        for component in
            std::iter::once(None).chain(directory.strip_prefix(root)?.components().map(Some))
        {
            if let Some(component) = component {
                current.push(component);
            }
            if current != base {
                let layer = if current.starts_with(base) {
                    &mut higher
                } else {
                    &mut lower
                };
                add_file(layer, &current.join(".gitignore"), Some(root))?;
            }
        }
        Ok((lower, higher))
    }
}

fn worktree_root<'a>(repo: &gix::Repository, base: &'a Path) -> anyhow::Result<&'a Path> {
    let worktree = repo
        .workdir()
        .ok_or_else(|| anyhow::anyhow!("bare repository"))?;
    // Preserve the editor's path spelling. On Windows canonicalize adds a verbatim prefix.
    let physical_base = base.canonicalize()?;
    let physical_root = worktree.canonicalize()?;
    let depth = physical_base
        .strip_prefix(&physical_root)?
        .components()
        .count();
    let root = base
        .ancestors()
        .nth(depth)
        .ok_or_else(|| anyhow::anyhow!("invalid worktree root"))?;
    anyhow::ensure!(
        root.canonicalize()? == physical_root,
        "path crosses a linked worktree directory"
    );
    Ok(root)
}

fn ignore_case(repo: &gix::Repository) -> Case {
    if repo
        .config_snapshot()
        .boolean("core.ignorecase")
        .unwrap_or(false)
    {
        Case::Fold
    } else {
        Case::Sensitive
    }
}

fn global_excludes(repo: &gix::Repository, root: &Path) -> anyhow::Result<Option<PathBuf>> {
    let config = repo.config_snapshot();
    Ok(match config.string("core.excludesFile") {
        Some(value) if value.is_empty() => None,
        Some(_) => {
            let trusted = config
                .trusted_path("core.excludesFile")
                .ok_or_else(|| anyhow::anyhow!("untrusted excludes file"))??;
            (!trusted.as_os_str().is_empty()).then(|| root.join(trusted))
        }
        None => gix::path::env::xdg_config("ignore", &mut gix::path::env::var),
    })
}

fn open_index(repo: &gix::Repository) -> anyhow::Result<Option<gix::worktree::Index>> {
    match fs::metadata(repo.index_path()) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
        // gix reads the trailing checksum before decoding the header.
        Ok(m) if m.is_file() && m.len() >= 12 + repo.object_hash().len_in_bytes() as u64 => {
            Ok(Some(repo.index()?))
        }
        Ok(_) => anyhow::bail!("invalid Git index"),
    }
}

fn tracked_names(repo: &gix::Repository, directory: &Path, case: Case) -> Option<HashSet<Vec<u8>>> {
    let Some(index) = open_index(repo).ok()? else {
        return Some(HashSet::new());
    };
    let prefix = git_path(directory)?;
    let mut names = HashSet::new();
    for entry in index.entries() {
        let path = entry.path(&index);
        let (parent, name) = path.rsplit_once_str(b"/").unwrap_or((b"".as_slice(), path));
        let same_dir = match case {
            Case::Fold => prefix.eq_ignore_ascii_case(parent),
            Case::Sensitive => prefix.as_bytes() == parent,
        };
        if same_dir {
            names.insert(match case {
                Case::Fold => name.to_ascii_lowercase(),
                Case::Sensitive => name.to_vec(),
            });
        }
    }
    Some(names)
}

fn add_file(search: &mut Search, file: &Path, root: Option<&Path>) -> io::Result<()> {
    if let Some(bytes) = read_ignore_file(file, root.is_some())? {
        search.add_patterns_buffer(&bytes, file, root, Default::default());
    }
    Ok(())
}

fn read_ignore_file(file: &Path, in_worktree: bool) -> io::Result<Option<Vec<u8>>> {
    let metadata = if in_worktree {
        fs::symlink_metadata(file)
    } else {
        fs::metadata(file)
    };
    match metadata {
        Ok(m) if m.is_file() => (),
        Ok(_) => return Ok(None),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    }
    // A speculative completion must not allocate arbitrarily for an external ignore file.
    const LIMIT: u64 = 2 * 1024 * 1024;
    let mut bytes = Vec::new();
    fs::File::open(file)?
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > LIMIT {
        return Err(io::Error::other("ignore file exceeds suggestion budget"));
    }
    Ok(Some(bytes))
}

pub struct Rules<'a> {
    repo: Option<&'a RepositorySources>,
    preceding: Search,
    following: Search,
}

impl Rules<'_> {
    pub fn ignore_case(&self) -> bool {
        self.repo.is_some_and(|repo| repo.case == Case::Fold)
    }

    pub fn tracked(&self, path: &Path) -> Option<bool> {
        let repo = self.repo?;
        if path.parent()? != repo.directory {
            return None;
        }
        let name = gix::path::os_str_into_bstr(path.file_name()?).ok()?;
        let tracked = repo.tracked.as_ref()?;
        Some(match repo.case {
            Case::Fold => tracked.contains(&name.to_ascii_lowercase()),
            Case::Sensitive => tracked.contains(name.as_bytes()),
        })
    }

    pub fn effect(&self, rule: &str, path: &Path, is_dir: bool) -> Effect {
        let Some(repo) = self.repo else {
            return Effect::Unknown;
        };
        let Some((lower, higher)) = &repo.layers else {
            return Effect::Unknown;
        };
        if self.tracked(path) != Some(false) {
            return Effect::Unknown;
        }
        let Some(relative) = path.strip_prefix(&repo.root).ok().and_then(git_path) else {
            return Effect::Unknown;
        };
        let mut candidate = Search::default();
        candidate.add_patterns_buffer(
            rule.as_bytes(),
            &repo.document,
            Some(&repo.root),
            Default::default(),
        );

        // Walk from the worktree root. An excluded directory blocks every
        // descendant, even a descendant with a higher-priority negation.
        let mut old = false;
        let mut new = false;
        let mut parent_excluded = false;
        let mut overridden = false;
        let ends = relative
            .find_iter(b"/")
            .chain(std::iter::once(relative.len()));
        for end in ends {
            let leaf = end == relative.len();
            let entry = relative[..end].as_bstr();
            let matches = |search: &Search| {
                search
                    .pattern_matching_relative_path(entry, Some(!leaf || is_dir), repo.case)
                    .map(|m| !m.pattern.is_negative())
            };
            let later = matches(higher).or_else(|| matches(&self.following));
            let inserted = matches(&candidate);
            let earlier = matches(&self.preceding).or_else(|| matches(lower));
            if leaf {
                parent_excluded = new;
                overridden = later.is_some() && inserted.is_some();
            }
            old |= later.or(earlier).unwrap_or(false);
            new |= later.or(inserted).or(earlier).unwrap_or(false);
        }
        if old != new {
            Effect::Changed
        } else if parent_excluded && rule.starts_with('!') {
            Effect::ParentExcluded
        } else if overridden {
            Effect::Overridden
        } else {
            Effect::Unchanged
        }
    }
}

#[cfg(test)]
mod tests;
