//! Git's winning rule with and without one line of the unsaved buffer.
use std::{
    borrow::Cow,
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use anyhow::{Context, Result};
use gix::{
    bstr::{BStr, ByteSlice},
    ignore::{
        glob::{pattern::Case, search::pattern::List},
        search::{pattern_idx_matching_relative_path, Ignore},
    },
    index::entry::Mode,
};

use super::{
    git_path, git_pattern_bytes, global_excludes, ignore_case, open_index, read_ignore_file,
    worktree_root,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    pub source: PathBuf,
    pub line: usize,
    pub pattern: String,
    pub ignored: bool,
}

impl Decision {
    /// Compare an editor line with the original Git pattern, including non-UTF-8 patterns.
    pub fn matches_source_line(&self, bytes: &[u8]) -> bool {
        pattern_text(bytes) == self.pattern
    }
}

fn pattern_text(bytes: &[u8]) -> Cow<'_, str> {
    match std::str::from_utf8(bytes) {
        Ok(text) => Cow::Borrowed(text),
        Err(_) => Cow::Owned(format!("{:?}", bytes.as_bstr())),
    }
}

#[derive(Clone, Default)]
pub(super) struct Evaluation {
    /// The selected pattern matched this entry or one of its ancestors,
    /// independently of whether it won.
    pub selected: bool,
    pub before: Option<Arc<Decision>>,
    pub after: Option<Arc<Decision>>,
}

#[derive(Clone, Default)]
pub(super) struct Directory {
    inherited: Evaluation,
    higher: Vec<Arc<Layer>>,
}

impl Directory {
    fn blocked(&self) -> bool {
        self.inherited.before.as_ref().is_some_and(|r| r.ignored)
            && self.inherited.after.as_ref().is_some_and(|r| r.ignored)
    }
}

pub(super) struct Matcher {
    root: PathBuf,
    case: Case,
    tracked: HashMap<Vec<u8>, Mode>,
    lower: Vec<Layer>,
    selected: Layer,
}

struct Layer {
    list: List<Ignore>,
    source: Arc<Source>,
    decisions: Vec<OnceLock<Arc<Decision>>>,
}

struct Source {
    display_path: PathBuf,
    // gix normalizes globs as it parses them. Keep the original spelling for
    // explanations, including escapes, slash anchors, trailing slashes and '!'.
    lines: Vec<Box<str>>,
}

impl Layer {
    fn parse(bytes: &[u8], file: &Path, base: Option<&Path>, root: &Path) -> Self {
        let raw = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
        let list = List::from_bytes(
            &git_pattern_bytes(bytes),
            file.to_owned(),
            base,
            Ignore::default(),
        );
        let source = Source {
            display_path: file.strip_prefix(root).unwrap_or(file).to_owned(),
            lines: raw.lines().map(|s| Box::from(pattern_text(s))).collect(),
        };
        let decisions = vec![OnceLock::new(); list.patterns.len()];
        Self {
            list,
            source: Arc::new(source),
            decisions,
        }
    }

    fn load(file: &Path, base: Option<&Path>, root: &Path) -> Result<Option<Self>> {
        let bytes = read_ignore_file(file, base.is_some())
            .with_context(|| format!("reading {}", file.display()))?;
        Ok(bytes.map(|b| Self::parse(&b, file, base, root)))
    }

    fn split_at(mut self, selected: usize) -> Result<(Self, Self, Self)> {
        let i = self
            .list
            .patterns
            .binary_search_by_key(&selected, |p| p.sequence_number)
            .map_err(|_| anyhow::anyhow!("line {selected} has no Git ignore pattern"))?;
        let following = self.list.patterns.split_off(i + 1);
        let selected = self.list.patterns.pop().expect("selected pattern");
        let sibling = |patterns: Vec<_>| Self {
            decisions: vec![OnceLock::new(); patterns.len()],
            list: List {
                base: self.list.base.clone(),
                source: self.list.source.clone(),
                patterns,
            },
            source: self.source.clone(),
        };
        let following = sibling(following);
        let selected = sibling(vec![selected]);
        self.decisions.truncate(i);
        Ok((self, selected, following))
    }

    fn matching(
        &self,
        path: &BStr,
        basename: Option<usize>,
        dir: bool,
        case: Case,
    ) -> Option<Arc<Decision>> {
        let i = pattern_idx_matching_relative_path(&self.list, path, basename, Some(dir), case)?;
        let m = &self.list.patterns[i];
        Some(
            self.decisions[i]
                .get_or_init(|| {
                    Arc::new(Decision {
                        source: self.source.display_path.clone(),
                        line: m.sequence_number,
                        pattern: self.source.lines[m.sequence_number - 1].to_string(),
                        ignored: !m.pattern.is_negative(),
                    })
                })
                .clone(),
        )
    }
}

impl Matcher {
    pub fn load(
        base: &Path,
        buffer: &str,
        line: usize,
        trust_full: bool,
        cancelled: &impl Fn() -> bool,
    ) -> Result<(Self, Directory)> {
        let repo = super::super::open_repo(base, trust_full)?.to_thread_local();
        let root = worktree_root(&repo, base)?;
        super::preview::validate_target(root, base)?;
        let case = ignore_case(&repo);
        let (preceding, selected, following) = Layer::parse(
            buffer.as_bytes(),
            &base.join(".gitignore"),
            Some(root),
            root,
        )
        .split_at(line)?;
        let mut tracked = HashMap::new();
        if let Some(index) = open_index(&repo).context("reading Git index")? {
            for (index_position, entry) in index.entries().iter().enumerate() {
                if index_position % 1024 == 0 {
                    anyhow::ensure!(!cancelled(), "ignore preview cancelled");
                }
                let path = entry.path(&index).as_bytes();
                let path = path.strip_suffix(b"/").unwrap_or(path);
                let name = match case {
                    Case::Fold => path.to_ascii_lowercase(),
                    Case::Sensitive => path.to_vec(),
                };
                tracked.insert(name, entry.mode);
            }
        }

        let mut lower = Vec::new();
        // Linked worktrees can record their common directory as ../..
        // relative to .git/worktrees/<name>.
        let info = gix::path::realpath(repo.common_dir())?.join("info/exclude");
        for file in global_excludes(&repo, root)?.iter().chain([&info]) {
            lower.extend(Layer::load(file, None, root)?);
        }
        let mut matcher = Self {
            root: root.to_owned(),
            case,
            tracked,
            lower,
            selected,
        };
        let mut directory = Directory::default();
        let mut parent = root.to_owned();
        for component in base.strip_prefix(root)?.components() {
            if !directory.blocked() {
                matcher
                    .lower
                    .extend(Layer::load(&parent.join(".gitignore"), Some(root), root)?);
            }
            parent.push(component);
            anyhow::ensure!(
                !matcher.tracked(&parent, true)?,
                "preview directory is inside a Git submodule"
            );
            directory.inherited = matcher.evaluate_at(&directory, &parent, true)?;
        }
        matcher.lower.push(preceding);
        directory.higher.push(Arc::new(following));
        Ok((matcher, directory))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn relative<'a>(&self, path: &'a Path) -> Result<Cow<'a, BStr>> {
        git_path(path.strip_prefix(&self.root)?).context("invalid Git path")
    }

    pub fn tracked(&self, path: &Path, is_directory: bool) -> Result<bool> {
        let path = self.relative(path)?;
        let mode = match self.case {
            Case::Fold => self.tracked.get(&path.to_ascii_lowercase()),
            Case::Sensitive => self.tracked.get(path.as_bytes()),
        };
        match mode {
            // Sparse entries represent whole trees without listing their tracked
            // children. Mark a materialized sparse directory as unexamined.
            Some(&Mode::DIR) if is_directory => anyhow::bail!("unexpanded sparse index directory"),
            Some(mode) => Ok(!is_directory || *mode == Mode::COMMIT),
            None => Ok(false),
        }
    }

    pub fn enter(&self, parent: &Directory, path: &Path) -> Result<Directory> {
        let mut child = Directory {
            inherited: self.evaluate_at(parent, path, true)?,
            higher: parent.higher.clone(),
        };
        if !child.blocked() {
            if let Some(layer) =
                Layer::load(&path.join(".gitignore"), Some(&self.root), &self.root)?
            {
                child.higher.push(Arc::new(layer));
            }
        }
        Ok(child)
    }

    pub fn evaluate(&self, parent: &Directory, path: &Path) -> Result<Evaluation> {
        self.evaluate_at(parent, path, false)
    }

    fn evaluate_at(&self, parent: &Directory, path: &Path, is_dir: bool) -> Result<Evaluation> {
        let relative = self.relative(path)?;
        let basename = relative.rfind(b"/").map(|p| p + 1);
        let matching = |layer: &Layer| layer.matching(&relative, basename, is_dir, self.case);
        let selected = matching(&self.selected);
        // An excluded directory wins before Git even looks at its children.
        // A negation on the directory itself doesn't carry to those children.
        let before = parent.inherited.before.clone().filter(|r| r.ignored);
        let after = parent.inherited.after.clone().filter(|r| r.ignored);
        let related = parent.inherited.selected || selected.is_some();
        if before.is_some() && after.is_some() {
            return Ok(Evaluation {
                selected: related,
                before,
                after,
            });
        }
        let higher = parent.higher.iter().rev().find_map(|l| matching(l));
        let common = higher
            .clone()
            .or_else(|| self.lower.iter().rev().find_map(matching));
        Ok(Evaluation {
            selected: related,
            before: before.or_else(|| common.clone()),
            after: after.or(higher).or(selected).or(common),
        })
    }
}
