mod snapshot;
use self::snapshot::{project_markers, sample, Entry, Sample};
use super::rules::{Effect, Rules};
use super::TEMPLATES;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::{Component, Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Context {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) filter: String,
    prefix: String,
    segment: String,
    suffix: String,
}

impl Context {
    pub(crate) fn parse(line: &str, cursor: usize) -> Option<Self> {
        if line.starts_with('#') {
            return None;
        }
        let byte = line
            .char_indices()
            .map(|(n, _)| n)
            .chain([line.len()])
            .nth(cursor)?;
        // The leading negation and anchor are rule syntax, including when the
        // cursor is sitting immediately before either one.
        let pattern = line.strip_prefix('!').unwrap_or(line);
        let prefix_len = line.len() - pattern.len() + usize::from(pattern.starts_with('/'));
        let byte = byte.max(prefix_len);
        let left = &line[..byte];
        let start = left
            .rfind('/')
            .map_or(usize::from(left.starts_with('!')), |n| n + 1);
        let end = byte + line[byte..].find('/').unwrap_or(line.len() - byte);
        Some(Self {
            start: line[..start].chars().count(),
            end: line[..end].chars().count(),
            filter: line[start..byte].to_owned(),
            segment: line[start..end].to_owned(),
            prefix: line[..start].to_owned(),
            suffix: line[end..].to_owned(),
        })
    }

    fn blank(&self) -> bool {
        self.prefix.is_empty() && self.segment.is_empty() && self.suffix.is_empty()
    }

    fn rule(&self, replacement: &str) -> String {
        format!("{}{replacement}{}", self.prefix, self.suffix)
    }

    pub(crate) fn repeat_pattern(&self, replacement: &str) -> String {
        let mut pattern = String::new();
        if !self.prefix.is_empty() && replacement.starts_with(['!', '#']) {
            // This character was literal inside a path; protect it at the start of a new rule.
            pattern.push('\\');
        }
        pattern.push_str(replacement);
        if self.suffix.starts_with('/') && !pattern.ends_with('/') {
            pattern.push('/');
        }
        pattern
    }

    pub(crate) fn replay_segment<'a>(&self, mut pattern: &'a str) -> &'a str {
        if !self.prefix.is_empty() {
            pattern = pattern.strip_prefix('!').unwrap_or(pattern);
            pattern = pattern.strip_prefix('/').unwrap_or(pattern);
        }
        if self.suffix.starts_with('/') {
            pattern = pattern.trim_end_matches('/');
        }
        pattern
    }

    fn directory(&self, base: &Path) -> Option<PathBuf> {
        let prefix = self.prefix.strip_prefix('!').unwrap_or(&self.prefix);
        let relative = prefix.strip_prefix('/').unwrap_or(prefix);
        let mut dir = base.to_owned();
        for part in relative.split_terminator('/') {
            let mut name = String::new();
            let mut chars = part.chars();
            while let Some(ch) = chars.next() {
                match ch {
                    '*' | '?' | '[' => return None,
                    '\\' => name.push(chars.next()?),
                    _ => name.push(ch),
                }
            }
            let mut components = Path::new(&name).components();
            if !matches!(components.next(), Some(Component::Normal(value)) if value == name.as_str())
                || components.next().is_some()
                || name.eq_ignore_ascii_case(".git")
            {
                return None;
            }
            dir.push(name);
            let metadata = fs::symlink_metadata(&dir).ok()?;
            if !metadata.is_dir() || is_link(&metadata) {
                return None;
            }
        }
        Some(dir)
    }
}

fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0 // FILE_ATTRIBUTE_REPARSE_POINT
    }
    #[cfg(not(windows))]
    metadata.is_symlink()
}

fn escape(name: &str, first: bool) -> String {
    let mut escaped = String::new();
    let trailing_spaces = name.trim_end_matches(' ').len();
    for (byte, ch) in name.char_indices() {
        if matches!(ch, '*' | '?' | '[' | ']' | '\\')
            || (first && byte == 0 && matches!(ch, '#' | '!'))
            || byte >= trailing_spaces
        {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

#[derive(Debug)]
pub(crate) struct Candidate {
    pub(crate) label: String,
    pub(crate) replacement: String,
    pub(crate) filter: String,
    pub(crate) detail: String,
    pub(crate) rank: u8,
}

// Rank semantic strength separately from the menu's incremental text score.
fn status(effect: Effect, template: bool) -> (u8, &'static str) {
    match effect {
        Effect::Changed => (0, "Changes ignore status"),
        Effect::Unknown if template => (1, "Effect unknown"),
        Effect::Unknown => (2, "Effect unknown"),
        Effect::Unchanged => (3, "Already has this effect"),
        Effect::Overridden => (4, "Overridden by a later rule"),
        Effect::ParentExcluded => (5, "Parent directory is excluded"),
    }
}

pub(crate) fn match_class(query: &str, key: &str) -> Option<u8> {
    // Leading spaces in .gitignore are significant even with fuzzy matching.
    if !key.starts_with(&query[..query.len() - query.trim_start_matches(' ').len()]) {
        return None;
    }
    let query = query.to_lowercase();
    let key = key.to_lowercase();
    if key == query {
        Some(0)
    } else if key.starts_with(&query) {
        Some(1)
    } else {
        let mut chars = key.chars();
        query.chars().all(|c| chars.any(|k| c == k)).then_some(2)
    }
}

fn scope(rule: &str) -> &'static str {
    let pattern = rule.strip_prefix('!').unwrap_or(rule).trim_end_matches('/');
    match (pattern.contains('/'), rule.ends_with('/')) {
        (true, true) => "Directory relative to this .gitignore",
        (true, false) => "Relative to this .gitignore",
        (false, true) => "Directory at any depth",
        (false, false) => "At any depth",
    }
}

// Exact repetitions can be proven redundant even when there is no sample on
// disk. Stop looking backwards at the first potential exception.
struct BufferRules<'a> {
    previous: HashSet<&'a str>,
    following_patterns: HashSet<&'a str>,
}

impl<'a> BufferRules<'a> {
    fn new(before: &'a str, after: &'a str) -> Self {
        let active = |line: &&str| !line.is_empty() && !line.starts_with('#');
        let mut preceding = before.lines().rev().filter(active).peekable();
        let negative = preceding.peek().is_some_and(|line| line.starts_with('!'));
        Self {
            previous: preceding
                .take_while(|line| line.starts_with('!') == negative)
                .collect(),
            following_patterns: after
                .lines()
                .filter(active)
                .map(|line| line.strip_prefix('!').unwrap_or(line))
                .collect(),
        }
    }

    fn effect(&self, rule: &str) -> Option<Effect> {
        if self
            .following_patterns
            .contains(rule.strip_prefix('!').unwrap_or(rule))
        {
            Some(Effect::Overridden)
        } else {
            self.previous.contains(rule).then_some(Effect::Unchanged)
        }
    }
}

fn local_candidates(
    context: &Context,
    sample: &Sample,
    rules: &Rules<'_>,
    buffer: &BufferRules<'_>,
    cancelled: &impl Fn() -> bool,
) -> Vec<Candidate> {
    let extensions = context.filter.starts_with("*.");
    let mut groups: BTreeMap<String, Vec<&Entry>> = BTreeMap::new();
    let mut items = Vec::new();
    for entry in &sample.entries {
        if cancelled() {
            return Vec::new();
        }
        if !context.suffix.is_empty() && !entry.directory {
            continue;
        }
        if !entry.directory && rules.tracked(&entry.path) == Some(true) {
            continue;
        }
        let mut replacement = escape(&entry.name, context.prefix.is_empty());
        if extensions {
            if entry.regular {
                if let Some(ext) = entry
                    .path
                    .extension()
                    .and_then(|s| s.to_str())
                    .filter(|s| !s.is_empty())
                {
                    groups
                        .entry(format!("*.{}", escape(ext, false)))
                        .or_default()
                        .push(entry);
                }
            }
            continue;
        }
        if match_class(&context.filter, &replacement).is_none() {
            continue;
        }
        let filter = replacement.clone();
        if entry.directory && context.suffix.is_empty() {
            replacement.push('/');
        }
        let label = context.rule(&replacement);
        let effect = buffer.effect(&label).unwrap_or_else(|| {
            if context.suffix.trim_end_matches('/').is_empty() {
                rules.effect(&label, &entry.path, entry.directory)
            } else {
                Effect::Unknown
            }
        });
        let (rank, reason) = status(effect, false);
        let kind = if entry.directory {
            "Local directory (untracked contents)"
        } else {
            "Local path"
        };
        let detail = format!("{} · {kind} · {reason}", scope(&label));
        items.push(Candidate {
            label,
            replacement,
            filter,
            rank,
            detail,
        });
    }
    for (replacement, entries) in groups {
        if match_class(&context.filter, &replacement).is_none() {
            continue;
        }
        let label = context.rule(&replacement);
        // Display a representative that benefits from the rule whenever one exists.
        let mut best = None;
        for entry in &entries {
            if cancelled() {
                return Vec::new();
            }
            let result = status(
                buffer
                    .effect(&label)
                    .unwrap_or_else(|| rules.effect(&label, &entry.path, false)),
                false,
            );
            if best.is_none_or(|(rank, _, _)| result.0 < rank) {
                best = Some((result.0, result.1, &entry.name));
            }
            if result.0 == 0 {
                break;
            }
        }
        let (rank, reason, name) = best.unwrap();
        let detail = format!(
            "{} · {name} ({} samples) · {reason}",
            scope(&label),
            entries.len()
        );
        items.push(Candidate {
            label,
            filter: replacement.clone(),
            replacement,
            detail,
            rank,
        });
    }
    items
}

// None means the literal rule would only offer a file known to be tracked.
fn template_effect(
    base: &Path,
    rule: &str,
    sample: &Sample,
    rules: &Rules<'_>,
    cancelled: &impl Fn() -> bool,
) -> Option<(u8, &'static str)> {
    let mut builder = ignore::gitignore::GitignoreBuilder::new(base);
    builder.case_insensitive(rules.ignore_case()).ok()?;
    builder.add_line(None, rule).ok()?;
    let matcher = builder.build().ok()?;
    let mut sampled_tracked = false;
    let mut best = None;
    for entry in &sample.entries {
        if cancelled() {
            return None;
        }
        if matcher.matched(&entry.path, entry.directory).is_none() {
            continue;
        }
        if !entry.directory && rules.tracked(&entry.path) == Some(true) {
            sampled_tracked = true;
            continue;
        }
        let result = status(rules.effect(rule, &entry.path, entry.directory), true);
        if result.0 == 0 {
            return Some((1, result.1));
        }
        if best.is_none_or(|(rank, _)| result.0 < rank) {
            best = Some(result);
        }
    }
    if best.is_none() && sampled_tracked && !rule.contains(['*', '?', '[']) {
        None
    } else {
        best.or(Some((1, "No local sample")))
    }
}

/// Generate insertions for exactly one path segment using a snapshot of the edited document.
pub(crate) fn suggest(
    base: &Path,
    context: &Context,
    before: &str,
    after: &str,
    paths: bool,
    manual: bool,
    trust_full: bool,
    cancelled: impl Fn() -> bool,
) -> Vec<Candidate> {
    if cancelled() || (context.blank() && !manual) {
        return Vec::new();
    }
    let markers = project_markers(base, manual);
    let sampled = paths
        .then(|| context.directory(base))
        .flatten()
        .and_then(|directory| sample(base, &directory, trust_full, manual, &cancelled));
    let rules = sampled.as_ref().map(|s| s.sources.rules(before, after));
    let buffer = BufferRules::new(before, after);
    let mut items = match (&sampled, &rules) {
        (Some(sample), Some(rules)) => {
            local_candidates(context, sample, rules, &buffer, &cancelled)
        }
        _ => Vec::new(),
    };
    let mut indices: HashMap<String, usize> = items
        .iter()
        .enumerate()
        .map(|(i, c)| (c.label.clone(), i))
        .collect();
    let mut templates = Vec::new();
    for template in TEMPLATES {
        let Some(evidence) = markers.get(template.id) else {
            continue;
        };
        for rule in template
            .text
            .lines()
            .filter(|s| !s.is_empty() && !s.starts_with('#'))
        {
            if cancelled() {
                return Vec::new();
            }
            if !context.blank() && rule.starts_with('!') != context.prefix.starts_with('!') {
                continue;
            }
            let Some(replacement) = rule
                .strip_prefix(&context.prefix)
                .and_then(|r| r.strip_suffix(&context.suffix))
            else {
                continue;
            };
            if replacement.is_empty() || (!context.suffix.is_empty() && replacement.contains('/')) {
                continue;
            }
            let filter = replacement
                .trim_start_matches(['!', '/'])
                .trim_end_matches('/');
            if match_class(&context.filter, filter).is_none() {
                continue;
            }
            let source = format!("{} template ({})", template.name, evidence.join(", "));
            if let Some(&index) = indices.get(rule) {
                let item = &mut items[index];
                if item.rank == status(Effect::Unknown, false).0 {
                    item.rank = status(Effect::Unknown, true).0;
                }
                item.detail.push_str(&format!(" · {source}"));
                templates.push(item.label.clone());
                continue;
            }
            let (rank, reason) = match (&sampled, &rules) {
                (Some(sample), Some(rules)) => {
                    let Some(result) = template_effect(base, rule, sample, rules, &cancelled)
                    else {
                        continue;
                    };
                    result
                }
                _ => (1, "No local sample"),
            };
            let (rank, reason) = buffer
                .effect(rule)
                .map_or((rank, reason), |effect| status(effect, true));
            templates.push(rule.to_string());
            indices.insert(rule.to_string(), items.len());
            items.push(Candidate {
                label: rule.to_string(),
                replacement: replacement.into(),
                filter: filter.into(),
                detail: format!("{} · {source} · {reason}", scope(rule)),
                rank,
            });
        }
    }
    if cancelled() {
        return Vec::new();
    }
    if context.blank() {
        items.retain(|c| c.rank < 3);
    }
    items.sort_by_cached_key(|c| {
        (
            match_class(&context.filter, &c.filter),
            c.rank,
            c.label.clone(),
        )
    });
    if context.blank() && items.len() > 8 {
        // Reserve a few slots for project rules when the directory has many untracked files.
        let featured: Vec<_> = items
            .iter()
            .filter(|c| templates.contains(&c.label))
            .take(3)
            .map(|c| c.label.clone())
            .collect();
        let mut rest = 8 - featured.len();
        items.retain(|c| {
            if featured.contains(&c.label) {
                return true;
            }
            if rest == 0 {
                return false;
            }
            rest -= 1;
            true
        });
    }
    items.truncate(if context.blank() { 8 } else { 50 });
    if sampled.is_some_and(|s| s.partial) {
        for item in &mut items {
            item.detail.push_str(" · Partial directory sample");
        }
    }
    items
}

#[cfg(test)]
mod tests;
