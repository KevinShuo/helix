use anyhow::{bail, ensure, Context, Result};
use helix_core::{syntax::Syntax, RopeSlice};
use helix_view::{ai_chat::Scope, document::Mode, Document, Editor};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Read,
    ops::Range,
    path::{Path, PathBuf},
};

pub(super) struct Snapshot {
    pub scope: Scope,
    pub label: String,
    pub source: String,
    pub context: String,
    pub language: String,
    pub range: Option<Range<usize>>,
    pub project_root: Option<PathBuf>,
}

fn line_span(text: RopeSlice, row: usize) -> Range<usize> {
    text.line_to_char(row)..text.line_to_char((row + 1).min(text.len_lines()))
}

fn line_start(text: RopeSlice, row: usize) -> String {
    let line = text.line(row);
    line.slice(..line.len_chars().min(512)).to_string()
}

fn line_numbers(text: RopeSlice, range: &Range<usize>) -> (usize, usize) {
    (
        1 + text.char_to_line(range.start),
        1 + text.char_to_line(range.end.saturating_sub(1).max(range.start)),
    )
}

fn explicit_selection(editor: &Editor) -> bool {
    let (view, doc) = current_ref!(editor);
    let range = doc.selection(view.id).primary();
    editor.mode() == Mode::Select
        || (!range.is_empty() && !range.is_single_grapheme(doc.text().slice(..)))
}

fn syntax_target(
    text: RopeSlice,
    syntax: &Syntax,
    cursor: usize,
    blank: bool,
) -> Option<(&'static str, Range<usize>)> {
    let byte = u32::try_from(text.char_to_byte(cursor)).ok()?;
    let node = syntax.named_descendant_for_byte_range(byte, byte)?;
    let mut declaration = None;
    for node in std::iter::successors(Some(node), |n| n.parent()) {
        if !(node.start_byte()..node.end_byte()).contains(&byte) {
            continue;
        }
        let kind = node.kind();
        let range = || {
            text.byte_to_char(node.start_byte() as usize)
                ..text.byte_to_char(node.end_byte() as usize)
        };
        if matches!(
            kind,
            "function_definition"
                | "function_declaration"
                | "function_item"
                | "function_expression"
                | "function_signature_item"
                | "function_signature"
                | "local_function"
                | "generator_function"
                | "generator_function_declaration"
                | "arrow_function"
                | "method"
                | "singleton_method"
                | "method_declaration"
                | "method_definition"
                | "constructor_declaration"
                | "local_function_statement"
                | "lambda"
                | "lambda_expression"
                | "closure_expression"
        ) {
            if let Some(parent) = node.parent().filter(|p| p.kind() == "decorated_definition") {
                return Some((
                    "函数",
                    text.byte_to_char(parent.start_byte() as usize)
                        ..text.byte_to_char(parent.end_byte() as usize),
                ));
            }
            return Some(("函数", range()));
        }
        if kind == "decorated_definition"
            && node.children().any(|n| n.kind() == "function_definition")
        {
            return Some(("函数", range()));
        }
        if !blank
            && declaration.is_none()
            && (kind.ends_with("_declaration")
                || matches!(
                    kind,
                    "declaration"
                        | "class"
                        | "struct_item"
                        | "enum_item"
                        | "trait_item"
                        | "impl_item"
                        | "type_item"
                        | "const_item"
                        | "static_item"
                        | "class_definition"
                        | "class_specifier"
                        | "struct_specifier"
                        | "enum_specifier"
                ))
        {
            declaration = Some(("声明", range()));
        }
    }
    declaration
}

pub(super) fn declaration(line: &str) -> bool {
    static C_DECL: std::sync::LazyLock<helix_core::regex::Regex> = std::sync::LazyLock::new(|| {
        helix_core::regex::Regex::new(
            r"^[\w:]+(?:<[^;{}=]*>)?(?:\s+|\s*[*&]+\s*)[*&]*[\w:]+\s*(?:[=(;\[])",
        )
        .unwrap()
    });
    let line = line.trim();
    let first = line
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .next()
        .unwrap_or("");
    matches!(
        first,
        "def"
            | "async"
            | "fn"
            | "func"
            | "function"
            | "class"
            | "struct"
            | "enum"
            | "trait"
            | "impl"
            | "interface"
            | "type"
            | "typedef"
            | "const"
            | "let"
            | "var"
            | "static"
            | "extern"
            | "pub"
            | "public"
            | "protected"
            | "private"
            | "export"
            | "namespace"
    ) || (line.ends_with(';')
        && !matches!(
            first,
            "" | "return" | "throw" | "break" | "continue" | "delete" | "goto"
        )
        && C_DECL.is_match(line))
}

pub(super) fn function_header(line: &str) -> bool {
    let line = line.trim();
    let first = line
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .next()
        .unwrap_or("");
    line.contains('(')
        && !matches!(
            first,
            "" | "if"
                | "for"
                | "while"
                | "switch"
                | "match"
                | "catch"
                | "return"
                | "else"
                | "try"
                | "do"
        )
        && (declaration(line)
            || line.contains("=>")
            || (line.contains(')')
                && (line.ends_with('{')
                    || line
                        .split_once('(')
                        .is_some_and(|(name, _)| name.split_whitespace().count() > 1))))
}

// A bounded fallback for buffers without installed grammars. Only publish a
// complete body: an unterminated brace or an exhausted search window is not a range.
fn brace_end(text: RopeSlice, row: usize) -> Option<usize> {
    let start = text.line_to_char(row);
    let slice = text.slice(start..(start + 64 * 1024).min(text.len_chars()));
    let mut chars = slice.chars().enumerate().peekable();
    let mut depth = 0usize;
    let mut quote = None;
    let mut line_comment = false;
    let mut block_comment = false;
    while let Some((i, ch)) = chars.next() {
        let next = chars.peek().map(|(_, c)| *c);
        if line_comment {
            if ch == '\n' {
                line_comment = false;
            }
        } else if block_comment {
            if ch == '*' && next == Some('/') {
                chars.next();
                block_comment = false;
            }
        } else if let Some(open) = quote {
            if ch == '\\' {
                chars.next();
            } else if ch == open {
                quote = None;
            }
        } else {
            match (ch, next) {
                ('/', Some('/')) => {
                    chars.next();
                    line_comment = true;
                }
                ('/', Some('*')) => {
                    chars.next();
                    block_comment = true;
                }
                ('"' | '\'' | '`', _) => quote = Some(ch),
                ('{', _) => depth += 1,
                ('}', _) if depth == 1 => return Some(start + i + 1),
                ('}', _) => depth = depth.checked_sub(1)?,
                (';', _) if depth == 0 => return None,
                _ if depth == 0 && i > 2048 => return None,
                _ => {}
            }
        }
    }
    None
}

fn fallback_target(text: RopeSlice, cursor: usize) -> Option<(&'static str, Range<usize>)> {
    let row = text.char_to_line(cursor);
    for head in (row.saturating_sub(400)..=row).rev() {
        let header = line_start(text, head);
        let trimmed = header.trim_start();
        if !function_header(trimmed) {
            continue;
        }
        let start = text.line_to_char(head);
        let end = if trimmed.starts_with("def ") || trimmed.starts_with("async def ") {
            let indent = header.len() - trimmed.len();
            let mut last = head;
            let limit = (head + 1200).min(text.len_lines());
            let mut complete = limit == text.len_lines();
            for next in head + 1..limit {
                let line = line_start(text, next);
                if line.trim().is_empty() {
                    continue;
                }
                if line.len() - line.trim_start().len() <= indent {
                    complete = true;
                    break;
                }
                last = next;
            }
            complete.then(|| line_span(text, last).end)
        } else {
            brace_end(text, head)
        };
        if let Some(end) = end.filter(|end| (start..*end).contains(&cursor)) {
            return Some(("函数", start..end));
        }
    }
    declaration(&line_start(text, row)).then(|| ("声明", line_span(text, row)))
}

fn function(doc: &Document, cursor: usize) -> Option<(&'static str, Range<usize>)> {
    let text = doc.text().slice(..);
    let blank = line_start(text, text.char_to_line(cursor))
        .trim()
        .is_empty();
    doc.syntax()
        .and_then(|syntax| syntax_target(text, syntax, cursor, blank))
        .or_else(|| fallback_target(text, cursor))
}

pub(crate) fn auto_target(editor: &Editor) -> Option<(Scope, &'static str)> {
    let (view, doc) = current_ref!(editor);
    let sel = doc.selection(view.id);
    if sel.len() != 1 {
        return None;
    }
    let text = doc.text().slice(..);
    let range = sel.primary();
    if explicit_selection(editor) {
        return range
            .slice(text)
            .chars()
            .any(|c| !c.is_whitespace())
            .then_some((Scope::Selection, "选区"));
    }
    function(doc, range.cursor(text)).map(|(label, _)| (Scope::Function, label))
}

const MARKERS: &[&str] = &[
    ".git",
    ".jj",
    ".svn",
    ".helix",
    "Cargo.toml",
    "go.mod",
    "go.work",
    "package.json",
    "pyproject.toml",
    "setup.py",
    "CMakeLists.txt",
    "meson.build",
    "build.zig",
    "pom.xml",
    "settings.gradle",
    "settings.gradle.kts",
    "build.gradle",
    "build.gradle.kts",
    "Makefile",
    "composer.json",
    "Gemfile",
    "mix.exs",
];

pub(super) fn root_from(dir: &Path) -> Result<PathBuf> {
    let mut nearest = None;
    for ancestor in dir.ancestors().take_while(|p| p.parent().is_some()) {
        if [".git", ".jj", ".svn", ".helix"]
            .iter()
            .any(|m| ancestor.join(m).exists())
        {
            return Ok(ancestor.to_owned());
        }
        if let Ok(file) = File::open(ancestor.join("Cargo.toml")) {
            let mut manifest = String::new();
            if file.take(64 * 1024).read_to_string(&mut manifest).is_ok()
                && manifest.lines().any(|l| {
                    l.trim().starts_with("[workspace]") || l.trim().starts_with("[workspace.")
                })
            {
                return Ok(ancestor.to_owned());
            }
        }
        if nearest.is_none() && MARKERS.iter().any(|m| ancestor.join(m).exists()) {
            nearest = Some(ancestor);
        }
    }
    let root = nearest.context("未找到工程根目录，请在包含项目清单或版本库的目录中打开文件")?;
    Ok(root.to_owned())
}

fn logical_project_root(editor: &Editor) -> Result<PathBuf> {
    let (_, doc) = current_ref!(editor);
    if let Some(parent) = doc.path().and_then(|p| p.parent()) {
        root_from(parent)
    } else {
        root_from(&helix_stdx::env::current_working_dir())
    }
}

pub(crate) fn project_root(editor: &Editor) -> Result<PathBuf> {
    Ok(fs::canonicalize(logical_project_root(editor)?)?)
}

fn nearby(text: RopeSlice, range: &Range<usize>, budget: usize) -> String {
    let (first, last) = line_numbers(text, range);
    let mut context = String::new();
    for row in (first.saturating_sub(4)..first - 1).chain(last..(last + 3).min(text.len_lines())) {
        let line = text.line(row);
        if line.len_bytes() + 24 > budget.saturating_sub(context.len()) {
            continue;
        }
        context.push_str(&format!(
            "{} | {}\n",
            row + 1,
            line.to_string().trim_end_matches(['\r', '\n'])
        ));
    }
    context
}

fn overview(text: RopeSlice, cursor: usize, budget: usize) -> Result<String> {
    let mut result = format!("文件概览（共 {} 行，以下为节选）\n", text.len_lines());
    ensure!(budget >= result.len() + 100, "上下文预算不足，请缩小范围");
    let row = text.char_to_line(cursor);
    let mut rows = vec![row, 0];
    for at in 0..text.len_lines().min(8192) {
        let line = line_start(text, at);
        if function_header(&line) || (line.trim_start() == line && declaration(&line)) {
            rows.push(at);
            if rows.len() >= 50 {
                break;
            }
        }
    }
    rows.extend(row.saturating_sub(2)..(row + 3).min(text.len_lines()));
    rows.extend(0..text.len_lines().min(3));
    let mut chosen = BTreeMap::new();
    let mut remaining = budget - result.len();
    for at in rows {
        if chosen.contains_key(&at) || remaining < 24 {
            continue;
        }
        let line = text.line(at);
        let mut visible = line.slice(..line.len_chars().min(160)).to_string();
        let limit = remaining
            .saturating_sub(20)
            .min((budget / 4).clamp(32, 480));
        while visible.len() > limit {
            visible.pop();
        }
        if visible.trim().is_empty() {
            continue;
        }
        let shortened = visible.len() < line.len_bytes();
        let entry = format!(
            "{} | {}{}\n",
            at + 1,
            visible.trim_end_matches(['\n', '\r']),
            if shortened { "…" } else { "" }
        );
        remaining -= entry.len();
        chosen.insert(at, entry);
    }
    for line in chosen.into_values() {
        result.push_str(&line);
    }
    Ok(result)
}

pub(super) fn resolve(editor: &Editor, requested: Scope, budget: usize) -> Result<Snapshot> {
    let budget = budget.min(128 * 1024);
    let (view, doc) = current_ref!(editor);
    let selection = doc.selection(view.id);
    let primary = selection.primary();
    let text = doc.text().slice(..);
    let cursor = primary.cursor(text);
    let mut scope = requested;
    if scope == Scope::Auto {
        scope = if explicit_selection(editor) {
            Scope::Selection
        } else {
            Scope::Function
        };
    }
    let language = doc.language_name().unwrap_or("").to_owned();
    if scope == Scope::Project {
        let logical_root = logical_project_root(editor)?;
        let root = fs::canonicalize(&logical_root)?;
        let mut source = String::new();
        if let Some(path) = doc
            .path()
            .and_then(|p| current_project_file(&logical_root, &root, p))
        {
            let rel = path.strip_prefix(&root)?.display();
            let header = format!("当前编辑缓冲 {rel}\n");
            let available = budget.saturating_sub(header.len());
            if available >= 160 {
                let available = available.min(2048);
                let (first, last) = line_numbers(text, &(0..text.len_chars()));
                let location = format!("第 {first}–{last} 行\n");
                source = header;
                if text.len_bytes() + location.len() <= available {
                    source.push_str(&location);
                    source.push_str(&text.to_string());
                } else {
                    source.push_str(&overview(text, cursor, available)?);
                }
            }
        }
        if secret_material(&source) {
            source.clear();
        }
        let label = format!(
            "工程 · {}",
            root.file_name().unwrap_or_default().to_string_lossy()
        );
        return Ok(Snapshot {
            scope,
            label,
            source,
            context: String::new(),
            language,
            range: None,
            project_root: Some(root),
        });
    }
    let (kind, range) = match scope {
        Scope::Selection => {
            ensure!(
                selection.len() == 1 && explicit_selection(editor),
                "请选择一段连续的代码"
            );
            ("选区", primary.from()..primary.to())
        }
        Scope::Function => {
            ensure!(selection.len() == 1, "请仅保留一个光标以确定函数或声明");
            function(doc, cursor)
                .context("光标不在可识别的函数或声明中，请选择代码或改用文件范围")?
        }
        Scope::File => ("文件", 0..text.len_chars()),
        _ => unreachable!(),
    };
    let slice = text.slice(range.clone());
    let (first, last) = line_numbers(text, &range);
    let label = format!("{kind} · {} · 第 {first}–{last} 行", doc.display_name());
    let mut context = String::new();
    let (source, range) = if slice.len_bytes() <= budget {
        let source = slice.to_string();
        ensure!(!source.trim().is_empty(), "所选范围没有可供分析的代码");
        if scope != Scope::File {
            context = nearby(text, &range, budget.saturating_sub(source.len()).min(2048));
        }
        (source, Some(range))
    } else if scope == Scope::File {
        (overview(text, cursor, budget)?, None)
    } else {
        bail!("{kind}超出当前模型预算，请缩小选区或改用文件概览");
    };
    Ok(Snapshot {
        scope,
        label,
        source,
        context,
        language,
        range,
        project_root: None,
    })
}

pub(super) fn allowed_name(path: &Path, directory: bool) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let n = name.to_ascii_lowercase();
    if n.starts_with('.')
        || n.chars().any(char::is_control)
        || n.contains(".env.")
        || n.contains("api_key")
        || n.contains("api-key")
        || n.contains("credential")
        || n.contains("secret")
        || n.contains("private_key")
        || n.contains("private-key")
        || n.contains("service-account")
        || n.contains("service_account")
        || n.contains("password")
        || n.starts_with("id_rsa")
        || n.starts_with("id_ed25519")
        || n.starts_with("id_ecdsa")
    {
        return false;
    }
    if directory {
        return !matches!(
            n.as_str(),
            "target"
                | "build"
                | "dist"
                | "out"
                | "obj"
                | "node_modules"
                | "vendor"
                | "vendors"
                | "coverage"
                | "__pycache__"
                | "venv"
                | "generated"
                | "cmakefiles"
                | "third_party"
        );
    }
    if n.ends_with(".lock")
        || n.ends_with(".min.js")
        || n.ends_with(".min.css")
        || matches!(
            n.as_str(),
            "package-lock.json"
                | "pnpm-lock.yaml"
                | "auth.json"
                | "token.json"
                | "tokens.json"
                | "keys.json"
                | "key.txt"
                | "token.txt"
                | "tokens.txt"
                | "cmakeuserpresets.json"
        )
    {
        return false;
    }
    if MARKERS.iter().any(|marker| marker.eq_ignore_ascii_case(&n))
        || matches!(n.as_str(), "readme" | "dockerfile" | "license")
    {
        return true;
    }
    matches!(
        n.rsplit('.').next(),
        Some(
            "rs" | "c"
                | "cc"
                | "cpp"
                | "cxx"
                | "h"
                | "hpp"
                | "hxx"
                | "go"
                | "py"
                | "pyi"
                | "js"
                | "jsx"
                | "ts"
                | "tsx"
                | "mjs"
                | "mts"
                | "vue"
                | "svelte"
                | "java"
                | "kt"
                | "kts"
                | "scala"
                | "cs"
                | "swift"
                | "rb"
                | "php"
                | "lua"
                | "sh"
                | "bash"
                | "zsh"
                | "fish"
                | "ps1"
                | "zig"
                | "ex"
                | "exs"
                | "erl"
                | "hrl"
                | "hs"
                | "elm"
                | "ml"
                | "mli"
                | "clj"
                | "cljs"
                | "dart"
                | "r"
                | "jl"
                | "proto"
                | "graphql"
                | "html"
                | "css"
                | "scss"
                | "md"
                | "rst"
                | "org"
                | "txt"
                | "toml"
                | "yaml"
                | "yml"
                | "xml"
                | "json"
                | "cmake"
                | "gradle"
        )
    )
}

pub(super) fn walker(root: &Path) -> ignore::WalkBuilder {
    let mut builder = ignore::WalkBuilder::new(root);
    builder
        .hidden(true)
        .require_git(false)
        .follow_links(false)
        .max_depth(Some(14))
        .add_custom_ignore_filename(".helix/ignore")
        .sort_by_file_name(|a, b| a.cmp(b));
    builder
}

pub(super) fn allowed_entry(entry: &ignore::DirEntry) -> bool {
    if entry.depth() == 0 {
        return true;
    }
    let Some(ty) = entry.file_type() else {
        return false;
    };
    if ty.is_symlink() || (!ty.is_dir() && !ty.is_file()) {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if entry
            .metadata()
            .is_ok_and(|m| m.file_attributes() & 0x400 != 0)
        {
            return false;
        }
    }
    allowed_name(entry.path(), ty.is_dir())
}

pub(super) fn included(root: &Path, path: &Path) -> bool {
    if !path.starts_with(root) || !allowed_name(path, false) {
        return false;
    }
    let target = path.to_owned();
    walker(root)
        .filter_entry(move |e| allowed_entry(e) && target.starts_with(e.path()))
        .build()
        .flatten()
        .any(|entry| entry.file_type().is_some_and(|t| t.is_file()) && entry.path() == path)
}

// Test the route used to open the buffer before following any links. The
// walker and file identity checks use canonical paths, but cannot distinguish
// an ignored directory alias from its ordinary source directory afterwards.
fn current_project_file(logical_root: &Path, root: &Path, path: &Path) -> Option<PathBuf> {
    let relative = path.strip_prefix(logical_root).ok()?;
    let mut candidate = logical_root.to_owned();
    let is_link = |entry: &Path| -> Option<bool> {
        let metadata = fs::symlink_metadata(entry).ok()?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            Some(metadata.file_attributes() & 0x400 != 0)
        }
        #[cfg(not(windows))]
        {
            Some(metadata.file_type().is_symlink())
        }
    };
    if is_link(&candidate)? {
        return None;
    }
    for component in relative.components() {
        let std::path::Component::Normal(name) = component else {
            return None;
        };
        candidate.push(name);
        if is_link(&candidate)? {
            return None;
        }
    }
    let real = fs::canonicalize(candidate).ok()?;
    included(root, &real).then_some(real)
}

fn priority(rel: &Path) -> usize {
    let name = rel
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    let at_root = rel.components().count() == 1;
    if name.starts_with("readme") {
        return if at_root { 14 } else { 3 };
    }
    if MARKERS.iter().any(|m| m.eq_ignore_ascii_case(&name)) {
        return if at_root { 13 } else { 5 };
    }
    if matches!(
        rel.file_stem().and_then(|n| n.to_str()),
        Some("main" | "lib" | "index" | "app" | "server" | "cli")
    ) {
        return 8;
    }
    1
}

fn query_terms(query: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let identifiers = query.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'));
    for term in identifiers.chain(query.split(|c: char| !(c.is_alphanumeric() || c == '_'))) {
        if term.len() < 2 || term.len() > 96 {
            continue;
        }
        let term = term.to_lowercase();
        if !matches!(
            term.as_str(),
            "the"
                | "and"
                | "of"
                | "to"
                | "in"
                | "is"
                | "this"
                | "how"
                | "please"
                | "explain"
                | "where"
                | "does"
                | "src"
                | "rs"
                | "py"
                | "js"
                | "ts"
                | "cpp"
        ) && !terms.contains(&term)
        {
            terms.push(term);
            if terms.len() == 16 {
                break;
            }
        }
    }
    terms
}

pub(super) fn secret_material(text: &str) -> bool {
    [
        "-----BEGIN PRIVATE KEY-----",
        "-----BEGIN ENCRYPTED PRIVATE KEY-----",
        "-----BEGIN OPENSSH PRIVATE KEY-----",
        "-----BEGIN RSA PRIVATE KEY-----",
        "-----BEGIN EC PRIVATE KEY-----",
        "-----BEGIN DSA PRIVATE KEY-----",
    ]
    .iter()
    .any(|marker| text.contains(marker))
}

fn hits(text: &str, terms: &[String]) -> usize {
    let lower = text.to_lowercase();
    terms.iter().filter(|t| lower.contains(t.as_str())).count()
}

struct Excerpt {
    rank: usize,
    path: PathBuf,
    body: String,
    rows: Vec<usize>,
}

fn excerpt(
    path: PathBuf,
    content: &str,
    rank: usize,
    terms: &[String],
    limit: usize,
) -> Option<Excerpt> {
    let lines: Vec<_> = content.lines().collect();
    if lines.is_empty() {
        return None;
    }
    let mut best = 0;
    let mut relevance = 0;
    for (i, line) in lines.iter().enumerate() {
        let score = hits(line, terms);
        if score > relevance {
            best = i;
            relevance = score
        }
    }
    let window = best.saturating_sub(2)..(best + 10).min(lines.len());
    let mut chosen = BTreeMap::new();
    let mut used = 0;
    for row in std::iter::once(best).chain(window.filter(|&r| r != best)) {
        let line = lines[row];
        let cut = line.char_indices().nth(240).map_or(line.len(), |(i, _)| i);
        let mut display = &line[..cut];
        let remaining = limit.saturating_sub(used + 20);
        if display.len() > remaining {
            let mut end = remaining;
            while !display.is_char_boundary(end) {
                end -= 1;
            }
            display = &display[..end];
        }
        if display.is_empty() && !line.is_empty() {
            break;
        }
        let entry = format!(
            "{} | {}{}\n",
            row + 1,
            display,
            if display.len() < line.len() {
                "…"
            } else {
                ""
            }
        );
        used += entry.len();
        chosen.insert(row, entry);
        if used >= limit.saturating_sub(20) {
            break;
        }
    }
    (!chosen.is_empty()).then(|| Excerpt {
        rank: rank + relevance * 20,
        path,
        rows: chosen.keys().map(|i| i + 1).collect(),
        body: chosen.into_values().collect(),
    })
}

/// Read complete UTF-8 lines from one bounded prefix. An open buffer always
/// supersedes its disk file, including when its current contents are excluded.
pub(super) fn read_source(
    path: &Path,
    buffer: Option<&helix_core::Rope>,
    limit: usize,
) -> Result<(String, bool)> {
    let (mut data, partial) = if let Some(buffer) = buffer {
        let size = buffer.len_bytes();
        let mut data = Vec::with_capacity(size.min(limit));
        for chunk in buffer.chunks() {
            let take = chunk.len().min(limit - data.len());
            data.extend_from_slice(&chunk.as_bytes()[..take]);
            if data.len() == limit {
                break;
            }
        }
        (data, size > limit)
    } else {
        let file = File::open(path)?;
        ensure!(file.metadata()?.is_file(), "该路径不是普通文件");
        let mut data = Vec::new();
        file.take(limit as u64 + 1).read_to_end(&mut data)?;
        let partial = data.len() > limit;
        data.truncate(limit);
        (data, partial)
    };
    if partial {
        let last = data
            .iter()
            .rposition(|b| *b == b'\n')
            .context("文件前缀没有完整行")?;
        data.truncate(last + 1);
    }
    let text = String::from_utf8(data).context("不支持二进制内容")?;
    ensure!(
        !secret_material(&text)
            && !text
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')),
        "该文件内容不适合导出"
    );
    Ok((text, partial))
}

/// Collect the initial evidence using the same sources as subsequent tool calls.
pub(super) fn project_seed(
    root: &Path,
    question: &str,
    budget: usize,
    current_file: Option<&Path>,
    buffers: &BTreeMap<PathBuf, helix_core::Rope>,
) -> Result<super::ai_agent_tools::ToolResult> {
    let canonical = fs::canonicalize(root).context("工程根目录已不存在")?;
    ensure!(canonical == root, "工程根路径已变化");
    let root = canonical;
    ensure!(
        root.parent().is_some() && root.is_dir() && MARKERS.iter().any(|m| root.join(m).exists()),
        "工程根目录已变化，请重新打开工程问答"
    );
    let budget = budget.min(64 * 1024);
    ensure!(budget >= 192, "工程上下文预算不足");
    let terms = query_terms(question);
    let current = current_file.and_then(|p| fs::canonicalize(p).ok());
    let mut candidates = Vec::new();
    for entry in walker(&root)
        .filter_entry(allowed_entry)
        .build()
        .take(4096)
        .flatten()
    {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let rel = entry.path().strip_prefix(&root)?.to_owned();
        let rank = priority(&rel) + 60 * hits(&rel.to_string_lossy(), &terms);
        candidates.push((rank, rel));
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let mut excerpts = Vec::new();
    let mut files = Vec::new();
    let mut remaining = 8 * 1024 * 1024;
    for (rank, rel) in candidates.iter().take(512) {
        if remaining == 0 {
            break;
        }
        let path = root.join(rel);
        let Ok(real) = fs::canonicalize(&path) else {
            continue;
        };
        if real != path || current.as_ref() == Some(&real) {
            continue;
        }
        let buffer = buffers.get(rel);
        let bytes = buffer.map(helix_core::Rope::len_bytes).or_else(|| {
            fs::metadata(&path)
                .ok()
                .map(|m| m.len().min(usize::MAX as u64) as usize)
        });
        let Some(size) = bytes else { continue };
        let limit = size.min(remaining).min(256 * 1024);
        remaining -= limit;
        let Ok((content, _)) = read_source(&path, buffer, limit) else {
            continue;
        };
        files.push(rel.as_path());
        if let Some(item) = excerpt(
            rel.clone(),
            &content,
            *rank,
            &terms,
            (budget / 3).clamp(32, 1800),
        ) {
            excerpts.push(item);
        }
    }
    excerpts.sort_by(|a, b| b.rank.cmp(&a.rank).then_with(|| a.path.cmp(&b.path)));
    let mut output = String::from("工程文件索引（有界抽样）：\n");
    for rel in files.iter().take(48) {
        let name = rel.to_string_lossy();
        if output.len() + name.len() + 1 > (budget / 4).min(2400) {
            continue;
        }
        output.push_str(&name);
        output.push('\n');
    }
    output.push_str("\n工程源码节选（行号从 1 开始）：\n");
    let mut sources = Vec::new();
    let mut hits = Vec::new();
    for item in excerpts {
        let block = format!("\n{}\n{}", item.path.display(), item.body);
        if output.len() + block.len() <= budget {
            output.push_str(&block);
            hits.extend(item.rows.into_iter().map(|row| (item.path.clone(), row)));
            sources.push(item.path);
        }
        if sources.len() == 12 {
            break;
        }
    }
    Ok(super::ai_agent_tools::ToolResult {
        text: output,
        paths: sources,
        hits,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use helix_core::Rope;

    #[test]
    fn fallback_selects_containing_function_or_actual_declaration() {
        let python =
            Rope::from("def first(a):\n    x = a + 1\n\n    return x\n\ndef other():\n    pass\n");
        let text = python.slice(..);
        let (kind, span) = fallback_target(text, text.line_to_char(2)).unwrap();
        assert_eq!(kind, "函数");
        assert_eq!(
            text.slice(span).to_string(),
            "def first(a):\n    x = a + 1\n\n    return x\n"
        );
        assert!(fallback_target(text, text.line_to_char(4)).is_none());
        let c = Rope::from("int queue_size;\nint run() {\n  // }\n  const char *s = \"{\";\n  return 2;\n}\nresult += x;\n");
        let text = c.slice(..);
        assert_eq!(fallback_target(text, 2).unwrap().0, "声明");
        let (_, span) = fallback_target(text, text.line_to_char(4)).unwrap();
        assert!(text.slice(span).to_string().ends_with("return 2;\n}"));
        assert!(fallback_target(text, text.line_to_char(6)).is_none());
    }

    #[test]
    fn large_file_overview_respects_utf8_and_keeps_real_cursor_line() {
        let source = format!("fn main() {{}}\n{}第九十九行\n", "// 中文\n".repeat(97));
        let rope = Rope::from(source.as_str());
        let summary = overview(rope.slice(..), rope.line_to_char(98), 280).unwrap();
        assert!(summary.len() <= 280, "{summary}");
        assert!(summary.contains("1 | fn main()"), "{summary}");
        assert!(summary.contains("99 | 第九十九行"), "{summary}");
        assert!(summary.contains("节选"));
    }

    #[test]
    fn project_is_confined_and_only_current_question_controls_file_ranking() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::create_dir(root.join(".git")).unwrap();
        fs::create_dir_all(root.join("crate/src/bin")).unwrap();
        fs::write(root.join("crate/Cargo.toml"), "[package]\nname='fixture'\n").unwrap();
        fs::write(root.join(".gitignore"), "ignored.py\n").unwrap();
        fs::write(root.join(".ignore"), "extra.rs\n").unwrap();
        for name in [
            ".env",
            "ignored.py",
            "extra.rs",
            "credentials.json",
            "bad.bin",
            "config.pem",
        ] {
            fs::write(root.join(name), "FORBIDDEN").unwrap();
        }
        fs::write(root.join("binary.py"), [0, 1, 2, 3]).unwrap();
        fs::write(root.join("key.txt"), "-----BEGIN PRIVATE KEY-----").unwrap();
        fs::write(root.join("current.rs"), "STALE_DISK_VERSION").unwrap();
        fs::write(
            root.join("crate/src/bin/launcher.rs"),
            "fn first() {}\nfn AlphaDispatcher() {}\n",
        )
        .unwrap();
        fs::write(root.join("worker.py"), "def BetaWorker(): pass\n").unwrap();
        let canonical = root_from(&root.join("crate/src/bin"))
            .unwrap()
            .canonicalize()
            .unwrap();
        assert_eq!(canonical, root.canonicalize().unwrap());
        assert!(!included(
            &canonical,
            &root.join("ignored.py").canonicalize().unwrap()
        ));
        assert!(included(
            &canonical,
            &root.join("current.rs").canonicalize().unwrap()
        ));
        let seed = project_seed(
            &canonical,
            "请解释AlphaDispatcher入口",
            1100,
            Some(&root.join("current.rs")),
            &BTreeMap::new(),
        )
        .unwrap();
        let (result, paths) = (seed.text, seed.paths);
        assert!(
            result.len() <= 1100 && result.contains("2 | fn AlphaDispatcher()"),
            "{result}"
        );
        assert!(
            !result.contains("FORBIDDEN")
                && !result.contains("STALE_DISK_VERSION")
                && !result.contains("binary.py")
                && !result.contains("key.txt"),
            "{result}"
        );
        assert_eq!(
            paths.first(),
            Some(&PathBuf::from("crate/src/bin/launcher.rs"))
        );
        let paths = project_seed(&canonical, "BetaWorker", 1100, None, &BTreeMap::new())
            .unwrap()
            .paths;
        assert_eq!(paths.first(), Some(&PathBuf::from("worker.py")));
    }

    #[test]
    fn ignored_directory_alias_cannot_contribute_an_unsaved_buffer() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::create_dir(root.join(".git")).unwrap();
        fs::create_dir(root.join("actual")).unwrap();
        fs::write(root.join("actual/code.py"), "x = 1").unwrap();
        fs::write(root.join(".gitignore"), "alias/\n").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("actual"), root.join("alias")).unwrap();
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let output = std::process::Command::new("cmd")
                .current_dir(root)
                .args(["/D", "/C", "mklink", "/J", "alias", "actual"])
                .creation_flags(0x08000000)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let physical = fs::canonicalize(root).unwrap();
        assert!(current_project_file(root, &physical, &root.join("actual/code.py")).is_some());
        assert!(current_project_file(root, &physical, &root.join("alias/code.py")).is_none());
    }
}
