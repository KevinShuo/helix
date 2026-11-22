//! Bounded read-only project tools. All paths are relative to the frozen root.
//! Only the worker touches the directory tree or materializes rope snapshots.
use super::{ai_agent_lsp::Semantic, ai_scope};
use anyhow::{bail, ensure, Context, Result};
use helix_core::Rope;
use helix_view::Editor;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

const FILE_BYTES: usize = 1024 * 1024;
const SEARCH_FILE_BYTES: usize = 256 * 1024;
const SEARCH_BYTES: usize = 8 * 1024 * 1024;
const MAX_ROWS: usize = 100;
const OUTPUT_BYTES: usize = 16 * 1024;

#[derive(Debug, Default)]
pub(super) struct ToolResult {
    pub text: String,
    pub paths: Vec<PathBuf>,
    pub hits: Vec<(PathBuf, usize)>,
}

impl ToolResult {
    fn evidence(&mut self, path: &Path, number: usize, line: &str) -> bool {
        let name = path.to_string_lossy().replace('\\', "/");
        let end = line.char_indices().nth(480).map_or(line.len(), |(i, _)| i);
        let suffix = if end < line.len() { "…" } else { "" };
        let row = format!("{name}:{number}: {}{suffix}\n", &line[..end]);
        if self.hits.len() >= MAX_ROWS || self.text.len() + row.len() > OUTPUT_BYTES {
            return false;
        }
        self.text.push_str(&row);
        if !self.paths.iter().any(|p| p == path) {
            self.paths.push(path.to_owned());
        }
        self.hits.push((path.to_owned(), number));
        true
    }
}

pub(super) struct Workspace {
    root: PathBuf,
    display_root: PathBuf,
    buffers: BTreeMap<PathBuf, Rope>,
    semantic: Semantic,
}

impl Workspace {
    pub(super) fn capture(editor: &Editor, root: &Path) -> Result<Self> {
        let root = fs::canonicalize(root)?;
        ensure!(root.is_dir() && root.parent().is_some(), "无效工程根目录");
        let display_root = helix_stdx::path::normalize(&root);
        // Preserve lexical routes. Canonicalizing an ignored symlink opened in
        // another view could overlay its contents onto a permitted file.
        let mut buffers = BTreeMap::new();
        for doc in editor.documents() {
            let Some(path) = doc.path() else { continue };
            if let Ok(rel) = path
                .strip_prefix(&display_root)
                .or_else(|_| path.strip_prefix(&root))
            {
                buffers.insert(rel.to_owned(), doc.text().clone());
            }
        }
        let semantic = Semantic::capture(editor, &root);
        Ok(Self {
            root,
            display_root,
            buffers,
            semantic,
        })
    }

    pub(super) fn seed(
        &self,
        question: &str,
        budget: usize,
        current_file: Option<&Path>,
    ) -> Result<ToolResult> {
        ai_scope::project_seed(&self.root, question, budget, current_file, &self.buffers)
    }

    fn relative<'a>(&self, path: &'a Path) -> Option<&'a Path> {
        // The LSP can return normalized paths without the Windows verbatim prefix.
        path.strip_prefix(&self.root)
            .or_else(|_| path.strip_prefix(&self.display_root))
            .ok()
    }

    fn checked(&self, relative: &Path) -> Result<PathBuf> {
        let path = self.root.join(relative);
        ensure!(
            ai_scope::included(&self.root, &path)
                && fs::canonicalize(&path).ok().as_ref() == Some(&path),
            "未找到可读取的工程文件"
        );
        Ok(path)
    }

    fn content(&self, rel: &Path, limit: usize) -> Result<(String, bool)> {
        let path = self.checked(rel)?;
        ai_scope::read_source(&path, self.buffers.get(rel), limit)
    }

    pub(super) async fn execute(self: &Arc<Self>, name: &str, args: &Value) -> Result<ToolResult> {
        let query = Query::parse(name, args)?;
        let semantic = if let Query::Symbol {
            symbol,
            references,
            origin: Some((rel, line, column)),
        } = &query
        {
            let workspace = self.clone();
            let rel_copy = rel.clone();
            let path = tokio::task::spawn_blocking(move || -> Result<PathBuf> {
                let path = workspace.checked(&rel_copy)?;
                ai_scope::read_source(&path, workspace.buffers.get(&rel_copy), FILE_BYTES)?;
                Ok(path)
            })
            .await??;
            self.semantic
                .lookup(&path, *line, *column, symbol, *references)
                .await
        } else {
            vec![]
        };
        let workspace = self.clone();
        tokio::task::spawn_blocking(move || workspace.run(query, semantic)).await?
    }

    fn run(&self, query: Query, semantic: Vec<(PathBuf, usize)>) -> Result<ToolResult> {
        ensure!(
            fs::canonicalize(&self.root).ok().as_ref() == Some(&self.root),
            "工程根路径已变化"
        );
        let mut result = ToolResult::default();
        if !semantic.is_empty() {
            result.text.push_str("语言服务器定位：\n");
            // Validate returned URIs before use; cache by file so dozens of
            // references to the same buffer require only one read.
            let mut locations: BTreeMap<PathBuf, Vec<usize>> = BTreeMap::new();
            for (path, line) in &semantic {
                if let Some(rel) = self.relative(path) {
                    locations.entry(rel.to_owned()).or_default().push(*line);
                }
            }
            let mut remaining = SEARCH_BYTES;
            for (rel, lines) in locations {
                if remaining == 0 {
                    break;
                }
                let limit = FILE_BYTES.min(remaining);
                remaining -= limit;
                let Ok((text, _)) = self.content(&rel, limit) else {
                    continue;
                };
                for (i, line) in text.lines().enumerate() {
                    if lines.contains(&(i + 1)) && !result.evidence(&rel, i + 1, line) {
                        return Ok(result);
                    }
                }
            }
            if !result.hits.is_empty() {
                return Ok(result);
            }
            result.text.clear();
        }
        match query {
            Query::Read { path, start, end } => {
                let (text, partial) = self.content(&path, FILE_BYTES)?;
                for (index, line) in text
                    .lines()
                    .enumerate()
                    .skip(start - 1)
                    .take(end - start + 1)
                {
                    if !result.evidence(&path, index + 1, line) {
                        result
                            .text
                            .push_str("已达到输出上限，可缩小行范围继续读取。\n");
                        break;
                    }
                }
                ensure!(
                    !result.hits.is_empty(),
                    "该行超出文件或单次可读取的源码前缀"
                );
                if partial && result.hits.last().is_some_and(|(_, number)| *number < end) {
                    result.text.push_str("已达到单文件读取上限。\n");
                }
            }
            Query::Structure(ref path)
            | Query::Search {
                path: Some(ref path),
                ..
            } => {
                let (text, partial) = self.content(path, FILE_BYTES)?;
                let complete = collect(&query, path, &text, &mut result);
                if partial || !complete {
                    result.text.push_str("仅显示限定范围内的结果。\n");
                }
            }
            _ => {
                if matches!(query, Query::Symbol { .. }) {
                    result
                        .text
                        .push_str("文本索引候选（未由语言服务器确认）：\n");
                }
                self.search(&query, &mut result);
            }
        }
        if result.paths.is_empty() {
            result.text.push_str("在已检查的范围内没有找到结果。\n");
        }
        Ok(result)
    }

    fn search(&self, query: &Query, result: &mut ToolResult) {
        let mut remaining = SEARCH_BYTES;
        let mut files = 0;
        for entry in ai_scope::walker(&self.root)
            .filter_entry(ai_scope::allowed_entry)
            .build()
            .take(4096)
            .flatten()
        {
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            let Ok(rel) = entry.path().strip_prefix(&self.root) else {
                continue;
            };
            if let Query::Files(term) = query {
                let name = rel.to_string_lossy().replace('\\', "/");
                if !name.to_lowercase().contains(term) {
                    continue;
                }
            }
            if files >= 512 || remaining == 0 {
                break;
            }
            files += 1;
            // A walk entry could have been replaced since enumeration.
            if fs::canonicalize(entry.path()).ok().as_deref() != Some(entry.path()) {
                continue;
            }
            let buffer = self.buffers.get(rel);
            let size = buffer.map(Rope::len_bytes).or_else(|| {
                entry
                    .metadata()
                    .ok()
                    .map(|m| m.len().min(usize::MAX as u64) as usize)
            });
            let Some(size) = size else { continue };
            let limit = size.min(SEARCH_FILE_BYTES).min(remaining);
            remaining -= limit;
            let Ok((text, _)) = ai_scope::read_source(entry.path(), buffer, limit) else {
                continue;
            };
            if let Query::Files(_) = query {
                let name = rel.to_string_lossy().replace('\\', "/");
                if result.paths.len() >= 100 || result.text.len() + name.len() + 1 > OUTPUT_BYTES {
                    break;
                }
                result.text.push_str(&name);
                result.text.push('\n');
                result.paths.push(rel.to_owned());
            } else if !collect(query, rel, &text, result) {
                break;
            }
        }
        result
            .text
            .push_str("工程检索有界；超过文件、字节或输出上限的内容可能未列出。\n");
    }
}

enum Query {
    Read {
        path: PathBuf,
        start: usize,
        end: usize,
    },
    Files(String),
    Search {
        term: String,
        path: Option<PathBuf>,
    },
    Structure(PathBuf),
    Symbol {
        symbol: String,
        references: bool,
        origin: Option<(PathBuf, usize, Option<usize>)>,
    },
}

fn relative_path(raw: &str) -> Result<PathBuf> {
    ensure!(
        !raw.is_empty()
            && raw.len() <= 512
            && !raw.contains(':')
            && !raw.chars().any(char::is_control),
        "无效工程相对路径"
    );
    let normalized = raw.replace('\\', "/");
    let path = Path::new(&normalized);
    ensure!(
        path.components().all(|c| matches!(c, Component::Normal(_)))
            && normalized
                .split('/')
                .all(|s| !s.is_empty() && s != "." && s != ".."),
        "只接受工程内相对路径"
    );
    Ok(path.to_owned())
}

impl Query {
    fn parse(name: &str, args: &Value) -> Result<Self> {
        let args = args.as_object().context("需要对象类型的工具参数")?;
        let string = |key: &str| -> Result<&str> {
            let text = args
                .get(key)
                .and_then(Value::as_str)
                .context("缺少文本参数")?;
            ensure!(
                !text.trim().is_empty() && text.len() <= 512 && !text.chars().any(char::is_control),
                "无效文本参数"
            );
            Ok(text)
        };
        let path = || relative_path(string("path")?);
        let number = |key: &str| -> Result<Option<usize>> {
            args.get(key)
                .map(|v| {
                    let n = v
                        .as_u64()
                        .and_then(|n| usize::try_from(n).ok())
                        .filter(|n| (1..=10_000_000).contains(n))
                        .context("行和列使用从 1 开始的整数")?;
                    Ok(n)
                })
                .transpose()
        };
        Ok(match name {
            "read_file" => {
                let start = number("start_line")?.unwrap_or(1);
                let end = number("end_line")?.unwrap_or(start + 59);
                ensure!(end >= start, "结束行应大于或等于起始行");
                Self::Read {
                    path: path()?,
                    start,
                    end: end.min(start + MAX_ROWS - 1),
                }
            }
            "find_files" => Self::Files(string("query")?.to_lowercase().replace('\\', "/")),
            "search_text" => Self::Search {
                term: string("query")?.to_owned(),
                path: args.contains_key("path").then(path).transpose()?,
            },
            "file_structure" => Self::Structure(path()?),
            "find_definition" | "find_references" => {
                let symbol = string("symbol")?;
                ensure!(symbol.chars().all(ident), "符号名需要是单个标识符");
                let line = number("line")?;
                let column = number("column")?;
                ensure!(
                    args.contains_key("path") == line.is_some()
                        && (column.is_none() || line.is_some()),
                    "语义定位需要同时提供 path 和 line"
                );
                Self::Symbol {
                    symbol: symbol.to_owned(),
                    references: name == "find_references",
                    origin: if let Some(line) = line {
                        Some((path()?, line, column))
                    } else {
                        None
                    },
                }
            }
            _ => bail!("未知只读工具"),
        })
    }
}

fn ident(c: char) -> bool {
    helix_core::chars::char_is_word(c) || c == '$'
}

fn symbol_match(line: &str, symbol: &str, references: bool) -> bool {
    if !references
        && ["#", "//", "/*", "*", "--"]
            .iter()
            .any(|prefix| line.trim_start().starts_with(prefix))
    {
        return false;
    }
    line.match_indices(symbol).any(|(offset, matched)| {
        let before = &line[..offset];
        let after = &line[offset + matched.len()..];
        if before.chars().next_back().is_some_and(ident) || after.chars().next().is_some_and(ident)
        {
            return false;
        }
        if references {
            return true;
        }
        let last = before
            .trim_end()
            .split(|c: char| !ident(c))
            .next_back()
            .unwrap_or("");
        matches!(
            last,
            "def"
                | "fn"
                | "func"
                | "function"
                | "class"
                | "struct"
                | "enum"
                | "trait"
                | "interface"
                | "type"
                | "alias"
                | "const"
                | "static"
                | "let"
                | "var"
                | "val"
                | "mod"
                | "namespace"
        ) || ((ai_scope::declaration(line) || ai_scope::function_header(line))
            && !before.contains(['=', '(', '{', ';'])
            && after.trim_start().starts_with(['(', ';', '=', ':', '[']))
    })
}

fn collect(query: &Query, path: &Path, text: &str, result: &mut ToolResult) -> bool {
    for (index, line) in text.lines().enumerate() {
        let matches = match query {
            Query::Search { term, .. } => line.contains(term),
            Query::Structure(_) => ai_scope::function_header(line) || ai_scope::declaration(line),
            Query::Symbol {
                symbol, references, ..
            } => symbol_match(line, symbol, *references),
            _ => false,
        };
        if matches && !result.evidence(path, index + 1, line) {
            return false;
        }
    }
    true
}

pub(super) fn definitions() -> Vec<Value> {
    let path =
        json!({"type":"string","description":"Project-relative file path, e.g. src/main.rs"});
    let line = json!({"type":"integer","minimum":1});
    let query = json!({"type":"string","minLength":1});
    let tool = |name: &str, description: &str, properties: Value, required: &[&str]| json!({"type":"function","function":{"name":name,"description":description,"parameters":{"type":"object","properties":properties,"required":required,"additionalProperties":false}}});
    let symbol = json!({"symbol":query,"path":path,"line":line,"column":line});
    vec![
        tool("read_file", "Read source lines inclusive. Rows are numbered from one; at most 100 per call.", json!({"path":path,"start_line":line,"end_line":line}), &["path"]),
        tool("find_files", "Find project paths by case-insensitive literal substring.", json!({"query":query}), &["query"]),
        tool("search_text", "Search for literal case-sensitive text, optionally in a single file.", json!({"query":query,"path":path}), &["query"]),
        tool("file_structure", "Show candidate declarations and functions with source lines.", json!({"path":path}), &["path"]),
        tool("find_definition", "Find a symbol definition. Provide its origin path and line together, and optional Unicode character column (all 1-based), to use a connected language server. Otherwise returns textual candidates.", symbol.clone(), &["symbol"]),
        tool("find_references", "Find references. Supply origin path and line together and optional Unicode character column (all 1-based) to resolve a symbol with a connected language server. Otherwise returns textual occurrences.", symbol, &["symbol"]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::{tempdir, TempDir};

    fn fixture() -> (TempDir, Arc<Workspace>) {
        let temp = tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        fs::create_dir(root.join(".git")).unwrap();
        fs::create_dir(root.join("src")).unwrap();
        fs::write(root.join("src/job.py"), "disk_only = 1\n").unwrap();
        fs::write(root.join("other.py"), "def job(): pass\nnot_job = 1\n").unwrap();
        fs::write(root.join(".gitignore"), "ignored.py\n").unwrap();
        for path in ["ignored.py", ".env", "credentials.json"] {
            fs::write(root.join(path), "forbidden").unwrap();
        }
        let buffers = BTreeMap::from([
            (
                PathBuf::from("src/job.py"),
                Rope::from("# unsaved\r\ndef job():\r\n    return job()\r\n"),
            ),
            (PathBuf::from("ignored.py"), Rope::from("forbidden_overlay")),
        ]);
        let workspace = Arc::new(Workspace {
            display_root: helix_stdx::path::normalize(&root),
            root,
            buffers,
            semantic: Semantic::default(),
        });
        (temp, workspace)
    }

    #[tokio::test]
    async fn every_tool_and_seed_use_overlay_and_one_based_lines() {
        let (_temp, ws) = fixture();
        let seed = ws.seed("job", 3000, None).unwrap();
        assert!(seed.text.contains("2 | def job()"));
        assert!(seed.hits.contains(&(PathBuf::from("src/job.py"), 2)));
        assert!(!seed.text.contains("forbidden") && !seed.text.contains("disk_only"));
        for (name, args, lines) in [
            (
                "read_file",
                json!({"path":"src/job.py","start_line":2,"end_line":2}),
                vec![2],
            ),
            ("file_structure", json!({"path":"src/job.py"}), vec![2]),
            (
                "search_text",
                json!({"query":"job()","path":"src/job.py"}),
                vec![2, 3],
            ),
            (
                "find_definition",
                json!({"symbol":"job","path":"src/job.py","line":3}),
                vec![2],
            ),
            ("find_references", json!({"symbol":"job"}), vec![2, 3]),
        ] {
            let result = ws.execute(name, &args).await.unwrap();
            let ours: Vec<_> = result
                .hits
                .iter()
                .filter(|(p, _)| p == Path::new("src/job.py"))
                .map(|(_, n)| *n)
                .collect();
            assert_eq!(ours, lines, "{name} {}", result.text);
            assert!(result.text.contains("src/job.py:2: def job():"), "{name}");
            if name.starts_with("find_") {
                assert!(result.hits.contains(&(PathBuf::from("other.py"), 1)));
            }
        }
        let files = ws
            .execute("find_files", &json!({"query":".py"}))
            .await
            .unwrap();
        assert_eq!(files.paths.len(), 2);
        assert!(files.hits.is_empty());
    }

    #[tokio::test]
    async fn ignored_files_binary_content_and_invalid_arguments_never_leak() {
        let (_temp, ws) = fixture();
        fs::write(ws.root.join("strange.py"), [0, 1, 2, 3]).unwrap();
        fs::write(ws.root.join("sample.txt"), "-----BEGIN PRIVATE KEY-----").unwrap();
        for path in [
            "ignored.py",
            ".env",
            "credentials.json",
            "src/../other.py",
            "..\\outside.py",
            "C:/test.rs",
            "/other.py",
            "other.py:stream",
            "strange.py",
            "sample.txt",
        ] {
            assert!(
                ws.execute("read_file", &json!({"path":path}))
                    .await
                    .is_err(),
                "{path}"
            );
        }
        for args in [
            json!({"path":"other.py","start_line":0}),
            json!({"path":"other.py","end_line":-1}),
            json!({"path":"other.py","start_line":3,"end_line":1}),
        ] {
            assert!(ws.execute("read_file", &args).await.is_err());
        }
        let result = ws
            .execute("search_text", &json!({"query":"forbidden"}))
            .await
            .unwrap();
        assert!(result.hits.is_empty());
        let forged = vec![
            (ws.root.join("ignored.py"), 1),
            (ws.root.join("other.py"), 999),
            (ws.root.join("other.py"), 1),
        ];
        let result = ws
            .run(
                Query::parse("find_definition", &json!({"symbol":"job"})).unwrap(),
                forged,
            )
            .unwrap();
        assert_eq!(result.hits, vec![(PathBuf::from("other.py"), 1)]);
        assert!(result.text.contains("语言服务器"));
    }

    #[test]
    fn oversized_results_preserve_bounded_unicode_evidence() {
        let (_temp, ws) = fixture();
        let source = format!("{}\n", "读".repeat(1200)).repeat(200);
        fs::write(ws.root.join("large.py"), source).unwrap();
        let result = ws
            .run(
                Query::parse(
                    "read_file",
                    &json!({"path":"large.py","start_line":87,"end_line":1000}),
                )
                .unwrap(),
                vec![],
            )
            .unwrap();
        assert!(result.text.len() <= OUTPUT_BYTES + 128);
        assert!(result.text.contains("large.py:87: 读"));
        assert!(result.hits.len() < MAX_ROWS);
        assert!(!symbol_match("fn unrelated(job: T) {", "job", false));
        assert!(!symbol_match("def jobs():", "job", true));
    }
}
