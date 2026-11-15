use helix_core::{diagnostic::Severity, regex::Regex};
use helix_lsp::{lsp, OffsetEncoding};
use helix_view::problems::{Problem, ProblemSource};
use std::{
    path::{Path, PathBuf},
    sync::LazyLock,
};

static GCC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(.+?):([0-9]+)(?::([0-9]+))?:\s*(fatal error|error|warning|note|remark):\s*(.*)$")
        .unwrap()
});
static MSVC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:[0-9]+>)?(.+)\(([0-9]+)(?:,\s*([0-9]+))?\)\s*:\s*(fatal error|error|warning|note)\s+(.+)$").unwrap()
});
static RUST: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(error|warning|note|help)(?:\[([^\]]+)\])?:\s*(.*)$").unwrap());
static ARROW: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*-->\s*(.+):([0-9]+):([0-9]+)\s*$").unwrap());
static ANSI: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)").unwrap()
});

pub(super) struct Parser {
    cwd: PathBuf,
    pending: Option<(Severity, String)>,
}

fn level(value: &str) -> Severity {
    match value {
        "error" | "fatal error" => Severity::Error,
        "warning" => Severity::Warning,
        "help" => Severity::Hint,
        _ => Severity::Info,
    }
}

fn coordinate(value: &str) -> Option<u32> {
    value.parse::<u32>().ok()?.checked_sub(1)
}

fn absolute(cwd: &Path, file: &str) -> Option<PathBuf> {
    let file = file.trim().trim_matches('"');
    if file.is_empty() || file.starts_with('<') {
        return None;
    }
    let path = Path::new(file);
    Some(helix_stdx::path::canonicalize(if path.is_absolute() {
        path.to_owned()
    } else {
        cwd.join(path)
    }))
}

fn problem(
    cwd: &Path,
    file: &str,
    row: u32,
    col: u32,
    severity: Severity,
    message: String,
    encoding: OffsetEncoding,
) -> Option<Problem> {
    let position = lsp::Position::new(row, col);
    Some(Problem {
        path: absolute(cwd, file)?,
        range: lsp::Range::new(position, position),
        encoding,
        severity,
        source: ProblemSource::Build,
        origin: if encoding == OffsetEncoding::Utf32 {
            "rustc"
        } else {
            "compiler"
        }
        .into(),
        message,
    })
}

impl Parser {
    pub(super) fn new(cwd: PathBuf) -> Self {
        Self { cwd, pending: None }
    }

    pub(super) fn push(&mut self, line: &str) -> Vec<Problem> {
        let clean = ANSI.replace_all(line.trim_end_matches('\r'), "");
        let line = clean.as_ref();
        if line.starts_with('{') {
            self.pending = None;
            return self.json(line);
        }
        for pattern in [&*MSVC, &*GCC] {
            if let Some(c) = pattern.captures(line) {
                self.pending = None;
                let Some(row) = coordinate(&c[2]) else {
                    return vec![];
                };
                let Some(col) = c.get(3).map_or(Some(0), |c| coordinate(c.as_str())) else {
                    return vec![];
                };
                return problem(
                    &self.cwd,
                    &c[1],
                    row,
                    col,
                    level(&c[4]),
                    c[5].to_owned(),
                    OffsetEncoding::Utf8,
                )
                .into_iter()
                .collect();
            }
        }
        if let Some(c) = RUST.captures(line) {
            self.pending = Some((
                level(&c[1]),
                match c.get(2) {
                    Some(code) => format!("[{}] {}", code.as_str(), &c[3]),
                    None => c[3].to_owned(),
                },
            ));
        } else if let Some(c) = ARROW.captures(line) {
            if let Some((severity, message)) = self.pending.take() {
                if let (Some(row), Some(col)) = (coordinate(&c[2]), coordinate(&c[3])) {
                    return problem(
                        &self.cwd,
                        &c[1],
                        row,
                        col,
                        severity,
                        message,
                        OffsetEncoding::Utf32,
                    )
                    .into_iter()
                    .collect();
                }
            }
        } else if !line.trim().is_empty() && !line.starts_with(' ') && !line.starts_with('\t') {
            self.pending = None;
        }
        vec![]
    }

    fn json(&self, line: &str) -> Vec<Problem> {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            return vec![];
        };
        if value["reason"] != "compiler-message" {
            return vec![];
        }
        let msg = &value["message"];
        let (Some(text), Some(severity), Some(spans)) = (
            msg["message"].as_str(),
            msg["level"].as_str(),
            msg["spans"].as_array(),
        ) else {
            return vec![];
        };
        let text = match msg["code"]["code"].as_str() {
            Some(code) => format!("[{code}] {text}"),
            None => text.into(),
        };
        spans
            .iter()
            .filter(|s| s["is_primary"] == true)
            .filter_map(|s| {
                let file = s["file_name"].as_str()?;
                let row = u32::try_from(s["line_start"].as_u64()?)
                    .ok()?
                    .checked_sub(1)?;
                let col = u32::try_from(s["column_start"].as_u64()?)
                    .ok()?
                    .checked_sub(1)?;
                // Rust normally reports paths relative to Cargo's invocation directory;
                // external wrappers may emit package-relative paths instead.
                let root = if self.cwd.join(file).exists() {
                    self.cwd.as_path()
                } else {
                    value["manifest_path"]
                        .as_str()
                        .and_then(|p| Path::new(p).parent())
                        .unwrap_or(&self.cwd)
                };
                let mut p = problem(
                    root,
                    file,
                    row,
                    col,
                    level(severity),
                    text.clone(),
                    OffsetEncoding::Utf32,
                )?;
                if let (Some(end_row), Some(end_col)) =
                    (s["line_end"].as_u64(), s["column_end"].as_u64())
                {
                    let end = lsp::Position::new(
                        u32::try_from(end_row).ok()?.checked_sub(1)?,
                        u32::try_from(end_col).ok()?.checked_sub(1)?,
                    );
                    if end < p.range.start {
                        return None;
                    }
                    p.range.end = end;
                }
                Some(p)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use helix_core::diagnostic::Severity;
    use helix_lsp::OffsetEncoding;

    fn parser() -> Parser {
        Parser::new(std::env::current_dir().unwrap())
    }

    #[test]
    fn gcc_clang_paths_colors_crlf_and_missing_columns() {
        let mut parser = parser();
        for (line, file, row, col, severity) in [
            (
                "src/main.c:12:8: error: undefined name",
                "src/main.c",
                11,
                7,
                Severity::Error,
            ),
            (
                "\x1b[33msrc/中文 space.cpp:2: warning: unused value\x1b[0m\r",
                "src/中文 space.cpp",
                1,
                0,
                Severity::Warning,
            ),
            (
                "C:\\my project\\a.cpp:3:2: fatal error: missing header",
                "C:\\my project\\a.cpp",
                2,
                1,
                Severity::Error,
            ),
        ] {
            let entries = parser.push(line);
            assert_eq!(entries.len(), 1, "{line}");
            let p = &entries[0];
            assert!(p
                .path
                .to_string_lossy()
                .replace('\\', "/")
                .ends_with(&file.replace('\\', "/")));
            assert_eq!(
                (p.range.start.line, p.range.start.character, p.severity),
                (row, col, severity)
            );
        }
    }

    #[test]
    fn msvc_with_and_without_columns_and_build_prefix() {
        let mut parser = parser();
        for line in ["1>C:\\my project\\中文.cpp(22,5): error C2065: 'x': undeclared identifier [project.vcxproj]", "src/a.cpp(22): warning C4101: unused variable"] {
            let entries = parser.push(line);
            assert_eq!(entries.len(), 1, "{line}");
            assert_eq!(entries[0].range.start.line, 21);
            assert!(entries[0].message.contains('C'));
        }
    }

    #[test]
    fn rust_multiline_message_and_primary_location() {
        let mut parser = parser();
        assert!(parser
            .push("error[E0425]: cannot find value `x`")
            .is_empty());
        let entries = parser.push("  --> src/main.rs:7:4");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].range.start.line, 6);
        assert_eq!(entries[0].encoding, OffsetEncoding::Utf32);
        assert!(entries[0].message.contains("E0425"));
        assert!(parser.push("  = note: not another problem").is_empty());
        assert!(parser.push(" --> src/lib.rs:2:1").is_empty());
    }

    #[test]
    fn cargo_json_primary_spans_and_manifest_directory() {
        let dir = tempfile::tempdir().unwrap();
        let mut parser = Parser::new(dir.path().to_owned());
        let json = serde_json::json!({"reason":"compiler-message", "manifest_path":dir.path().join("crate/Cargo.toml"), "message":{
            "level":"warning", "message":"unused café", "code":{"code":"unused"},
            "spans":[{"file_name":"src/lib.rs","line_start":4,"column_start":3,"line_end":4,"column_end":7,"is_primary":true},
                     {"file_name":"src/other.rs","line_start":1,"column_start":1,"is_primary":false}]
        }});
        let entries = parser.push(&json.to_string());
        assert_eq!(entries.len(), 1);
        assert!(entries[0].path.ends_with("crate/src/lib.rs"));
        assert_eq!(entries[0].range.start.line, 3);
        assert_eq!(entries[0].range.start.character, 2);
        assert_eq!(entries[0].encoding, OffsetEncoding::Utf32);
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/lib.rs"), "").unwrap();
        let entries = parser.push(&json.to_string());
        assert_eq!(
            entries[0].path,
            helix_stdx::path::canonicalize(dir.path().join("src/lib.rs"))
        );
        assert!(parser
            .push("{\"reason\":\"build-finished\",\"success\":false}")
            .is_empty());
    }

    #[test]
    fn rejects_invalid_or_unlocated_output() {
        let mut parser = parser();
        for line in [
            "a.c:0:2: error: invalid line",
            "a.c:2:0: error: invalid column",
            "a.c:99999999999999999999:1: error: overflow",
            "LINK : fatal error LNK1120: unresolved",
            "ordinary output",
            "{invalid json}",
            "src/a.c:1:2: something else",
        ] {
            assert!(parser.push(line).is_empty(), "{line}");
        }
    }
}
