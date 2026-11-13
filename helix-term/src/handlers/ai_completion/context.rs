//! Bounded supporting declarations for an unchanged cursor snapshot.

use helix_core::{syntax::Syntax, tree_sitter::Node, RopeSlice};
use helix_lsp::lsp;
use std::{collections::HashSet, sync::Arc, time::Duration};

pub(super) const MAX_SUPPORTING_CHARS: usize = 1200;

pub(super) fn member_stem(prefix: &str) -> Option<&str> {
    let line = prefix.rsplit('\n').next()?;
    let stem = line
        .rsplit(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
        .next()?;
    let receiver = &line[..line.len() - stem.len()];
    (receiver.ends_with('.') || receiver.ends_with("->") || receiver.ends_with("::"))
        .then_some(stem)
}

pub(super) struct MemberContext {
    pub notes: String,
    pub names: Option<Vec<String>>,
}

impl MemberContext {
    pub fn from_response(response: lsp::CompletionResponse, stem: &str, budget: usize) -> Self {
        let (items, complete) = match response {
            lsp::CompletionResponse::Array(items) => (items, true),
            lsp::CompletionResponse::List(list) => (list.items, !list.is_incomplete),
        };
        let mut authoritative = complete && items.len() <= 128;
        let mut items: Vec<_> = items.into_iter().take(128).collect();
        items.sort_by(|a, b| {
            a.sort_text
                .as_deref()
                .unwrap_or(&a.label)
                .cmp(b.sort_text.as_deref().unwrap_or(&b.label))
        });
        let mut names = HashSet::new();
        let mut notes = String::new();
        for item in items {
            if item.deprecated == Some(true)
                || item
                    .tags
                    .as_ref()
                    .is_some_and(|tags| tags.contains(&lsp::CompletionItemTag::DEPRECATED))
            {
                continue;
            }
            if item.kind.is_some_and(|kind| {
                !matches!(
                    kind,
                    lsp::CompletionItemKind::METHOD
                        | lsp::CompletionItemKind::FUNCTION
                        | lsp::CompletionItemKind::FIELD
                        | lsp::CompletionItemKind::VARIABLE
                        | lsp::CompletionItemKind::PROPERTY
                        | lsp::CompletionItemKind::CONSTANT
                        | lsp::CompletionItemKind::ENUM_MEMBER
                        | lsp::CompletionItemKind::CLASS
                        | lsp::CompletionItemKind::STRUCT
                        | lsp::CompletionItemKind::MODULE
                        | lsp::CompletionItemKind::ENUM
                        | lsp::CompletionItemKind::INTERFACE
                        | lsp::CompletionItemKind::TYPE_PARAMETER
                )
            }) {
                authoritative = false;
                continue;
            }
            let name = item
                .filter_text
                .as_deref()
                .unwrap_or(&item.label)
                .split(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
                .next()
                .unwrap_or_default();
            if name.is_empty() || name.chars().count() > 96 || !name.starts_with(stem) {
                authoritative = false;
                continue;
            }
            if !names.insert(name.to_owned()) {
                continue;
            }
            let label = item.label.split_whitespace().collect::<Vec<_>>().join(" ");
            if label.chars().count() > 160 {
                continue;
            }
            let detail = item
                .detail
                .as_deref()
                .unwrap_or_default()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let label = if detail.is_empty() || detail.chars().count() > 160 {
                label
            } else {
                format!("{label}: {detail}")
            };
            add_note(
                &mut notes,
                "Language server member candidate",
                &label,
                budget,
            );
        }
        Self {
            notes,
            names: (authoritative && !names.is_empty()).then(|| names.into_iter().collect()),
        }
    }
}

pub(super) async fn fetch_members(
    server: Arc<helix_lsp::Client>,
    params: lsp::CompletionParams,
    stem: &str,
    budget: usize,
) -> Option<MemberContext> {
    let response = tokio::time::timeout(
        Duration::from_millis(200),
        server.call_cancellable::<lsp::request::Completion>(params),
    )
    .await
    .ok()?
    .ok()??;
    Some(MemberContext::from_response(response, stem, budget))
}

pub(super) fn source_notes(
    text: RopeSlice,
    syntax: Option<&Syntax>,
    cursor: usize,
    prefix_chars: usize,
) -> String {
    let Some(syntax) = syntax else {
        return String::new();
    };
    let prefix_chars = prefix_chars.min(64_000);
    let budget = (prefix_chars / 4).min(600);
    let recent_start = cursor.saturating_sub(prefix_chars.saturating_sub(2 * budget));
    let preceding = text
        .chars_at(cursor)
        .reversed()
        .take(256)
        .take_while(|ch| !matches!(ch, '\r' | '\n'))
        .position(|ch| !ch.is_whitespace())
        .unwrap_or(0);
    let point = text.char_to_byte(cursor.saturating_sub(preceding + 1)) as u32;
    let tree = syntax.tree_for_byte_range(point, point);
    let mut notes = String::new();
    let mut node = tree.root_node().descendant_for_byte_range(point, point);
    for _ in 0..32 {
        let Some(current) = node else { break };
        if matches!(
            current.kind(),
            "function_definition"
                | "function_item"
                | "method_definition"
                | "method_declaration"
                | "constructor_declaration"
                | "function_declaration"
                | "arrow_function"
                | "class_definition"
                | "class_specifier"
                | "struct_specifier"
                | "impl_item"
                | "class_declaration"
        ) {
            if let Some(body) = current.children().find(|child| {
                matches!(
                    child.kind(),
                    "block"
                        | "compound_statement"
                        | "statement_block"
                        | "field_declaration_list"
                        | "declaration_list"
                        | "class_body"
                )
            }) {
                let start = text.byte_to_char(current.start_byte() as usize);
                let header_end = current
                    .children()
                    .find(|child| child.kind() == ":" && child.end_byte() <= body.start_byte())
                    .map_or(body.start_byte(), |colon| colon.end_byte());
                let end = text.byte_to_char(header_end as usize);
                if start < recent_start && end <= cursor && end.saturating_sub(start) <= budget {
                    add_note(
                        &mut notes,
                        "Enclosing declaration",
                        text.slice(start..end).to_string().trim(),
                        budget,
                    );
                }
                add_contract_notes(
                    &mut notes,
                    text,
                    &current,
                    &body,
                    recent_start,
                    cursor,
                    budget,
                );
            }
        }
        node = current.parent();
    }
    for node in tree.root_node().children().take(128) {
        if matches!(
            node.kind(),
            "import_statement"
                | "import_from_statement"
                | "use_declaration"
                | "preproc_include"
                | "using_declaration"
                | "using_directive"
        ) {
            let start = text.byte_to_char(node.start_byte() as usize);
            let end = text.byte_to_char(node.end_byte() as usize);
            if start < recent_start && end <= cursor && end.saturating_sub(start) <= budget {
                add_note(
                    &mut notes,
                    "Import from this file",
                    text.slice(start..end).to_string().trim(),
                    budget,
                );
            }
        }
    }
    notes
}

fn add_contract_notes(
    notes: &mut String,
    text: RopeSlice,
    declaration: &Node,
    body: &Node,
    recent_start: usize,
    cursor: usize,
    budget: usize,
) {
    // Python docstrings are the first expression in a suite, not arbitrary
    // strings later in the function. Formatted/byte literals are not docstrings.
    if declaration.kind() == "function_definition" && body.kind() == "block" {
        if let Some(expression) = body
            .children()
            .take(32)
            .find(|child| child.is_named() && child.kind() != "comment")
            .filter(|child| child.kind() == "expression_statement")
        {
            if let Some(string) = expression
                .children()
                .find(|child| child.is_named())
                .filter(|child| child.kind() == "string")
            {
                let start = text.byte_to_char(string.start_byte() as usize);
                let end = text.byte_to_char(string.end_byte() as usize);
                if start < recent_start && end <= cursor && end - start <= budget {
                    let source = text.slice(start..end).to_string();
                    let prefix = source.split(['\'', '"']).next().unwrap_or_default();
                    if prefix.chars().all(|ch| matches!(ch, 'r' | 'R' | 'u' | 'U')) {
                        add_note(notes, "Enclosing documentation", &source, budget);
                    }
                }
            }
        }
    }

    // Only a contiguous comment block directly attached to this declaration.
    // A blank line or other syntax is a boundary; never search past it.
    let mut first = declaration.clone();
    for _ in 0..8 {
        let Some(previous) = first.prev_named_sibling() else {
            break;
        };
        if !matches!(
            previous.kind(),
            "comment" | "line_comment" | "block_comment"
        ) {
            break;
        }
        let start = text.byte_to_char(previous.start_byte() as usize);
        let line_start = text.line_to_char(text.char_to_line(start));
        if start - line_start > budget
            || text
                .slice(line_start..start)
                .chars()
                .any(|ch| !ch.is_whitespace())
        {
            break;
        }
        let gap = text.byte_slice(previous.end_byte() as usize..first.start_byte() as usize);
        if gap.len_bytes() > budget * 4
            || gap.chars().any(|ch| !ch.is_whitespace())
            || gap.chars().filter(|ch| *ch == '\n').count() > 1
        {
            break;
        }
        first = previous;
    }
    if &first != declaration {
        let start = text.byte_to_char(first.start_byte() as usize);
        let end = text.byte_to_char(declaration.start_byte() as usize);
        if start < recent_start && end <= cursor && end - start <= budget {
            add_note(
                notes,
                "Enclosing documentation",
                text.slice(start..end).to_string().trim(),
                budget,
            );
        }
    }
}

fn add_note(notes: &mut String, label: &str, source: &str, budget: usize) {
    let separator = if notes.is_empty() { "" } else { "\n\n" };
    let required = separator.len() + label.len() + 2 + source.chars().count();
    if !source.is_empty() && notes.chars().count() + required <= budget {
        notes.push_str(separator);
        notes.push_str(label);
        notes.push_str(":\n");
        notes.push_str(source);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use helix_core::Rope;

    fn notes(language: &str, source: &str, cursor: usize, budget: usize) -> String {
        let loader = helix_core::config::default_lang_loader();
        let text = Rope::from(source);
        let syntax = Syntax::new(
            text.slice(..),
            loader.language_for_name(language).unwrap(),
            &loader,
        )
        .unwrap();
        source_notes(text.slice(..), Some(&syntax), cursor, budget)
    }

    #[test]
    fn source_context_restores_distant_python_contract() {
        let source = format!(
            "def process(values):\r\n    \"\"\"Return the largest strictly negative value; use 0 when absent. 中文。\"\"\"\r\n{}    return \r\n",
            "    # validated\r\n".repeat(160)
        );
        let result = notes("python", &source, source.chars().count() - 2, 3000);
        assert!(result.contains("def process(values):"));
        assert!(result.contains("largest strictly negative value"));
        assert!(result.contains("中文。"));
        assert!(result.chars().count() <= 600);
    }

    #[test]
    fn source_context_restores_only_attached_contract_comments() {
        for (language, header, tail) in [
            ("cpp", "// unrelated old note\n\n/// Return the smallest nonzero value.\n/// Use zero if there are none.\nint process(int* values) {\n", "}\n"),
            ("rust", "// unrelated old note\n\n/// Return the smallest nonzero value.\n/// Use zero if there are none.\nfn process(values: &[i32]) -> i32 {\n", "}\n"),
        ] {
            let source = format!("{header}{}    return 0;\n{tail}", "    // validated\n".repeat(160));
            let result = notes(language, &source, source.find("return 0").unwrap(), 3000);
            assert!(result.contains("smallest nonzero value"), "{language}: {result}");
            assert!(result.contains("Use zero if there are none"));
            assert!(!result.contains("unrelated old note"));
        }
    }

    #[test]
    fn source_context_does_not_attach_trailing_comments_to_next_function() {
        let source = format!(
            "int previous; // DO_NOT_INCLUDE\nint process(int value) {{\n{}    return value;\n}}\n",
            "    // validated\n".repeat(160)
        );
        let result = notes("cpp", &source, source.find("return value").unwrap(), 3000);
        assert!(result.contains("int process(int value)"));
        assert!(!result.contains("DO_NOT_INCLUDE"));
    }

    #[test]
    fn source_context_excludes_non_contract_strings_and_future_docs() {
        for statement in [
            "description = 'DO_NOT_INCLUDE'",
            "f'DO_NOT_INCLUDE'",
            "b'DO_NOT_INCLUDE'",
            "1\n    'DO_NOT_INCLUDE'",
        ] {
            let source = format!(
                "def process(values):\n    {statement}\n{}    return \n",
                "    # validated\n".repeat(160)
            );
            let result = notes("python", &source, source.chars().count() - 1, 3000);
            assert!(!result.contains("DO_NOT_INCLUDE"), "{statement}");
        }
        let source = "def process(values):\n    \"\"\"DO_NOT_INCLUDE\"\"\"\n    return 0\n";
        let result = notes("python", source, source.find("DO_NOT_INCLUDE").unwrap(), 32);
        assert!(!result.contains("DO_NOT_INCLUDE"));
    }

    #[test]
    fn source_context_does_not_repeat_nearby_or_oversized_docs() {
        let source =
            "def process(values):\n    \"\"\"Return only negative values.\"\"\"\n    return \n";
        assert!(notes("python", source, source.len() - 1, 3000).is_empty());
        let source = format!(
            "def process(values):\n    \"\"\"{}\"\"\"\n{}    return \n",
            "OVERSIZED ".repeat(200),
            "    # validated\n".repeat(160)
        );
        let result = notes("python", &source, source.chars().count() - 1, 3000);
        assert!(result.contains("def process(values):"));
        assert!(!result.contains("OVERSIZED"));
        assert!(result.chars().count() <= 600);
    }

    #[test]
    fn source_context_restores_distant_signatures_and_aliases() {
        let source = format!("from pathlib import Path as FilePath\r\n\r\ndef config_path(基础: str):\r\n{}    return \r\n", "    # checked\r\n".repeat(150));
        let result = notes("python", &source, source.chars().count() - 2, 1500);
        assert!(result.contains("def config_path(基础: str):"));
        assert!(result.contains("from pathlib import Path as FilePath"));
        assert!(result.chars().count() <= 375);
        assert!(notes("python", &source, source.chars().count() - 2, 32).is_empty());
    }

    #[test]
    fn source_context_preserves_multiline_cpp_parameters() {
        let source = format!(
            "int total(\n  const int* amounts,\n  int count\n) {{\n{}    return 0;\n}}\n",
            "    // checked\n".repeat(150)
        );
        let cursor = source.find("return").unwrap();
        let result = notes("cpp", &source, cursor, 1500);
        assert!(result.contains("const int* amounts,\n  int count\n)"));
        assert!(!result.contains("return 0"));
    }

    #[test]
    fn source_context_omits_nearby_or_unparsed_declarations() {
        let source = "def total(items):\n    return \n";
        assert!(notes("python", source, source.len() - 1, 1500).is_empty());
        let text = Rope::from(source);
        assert!(source_notes(text.slice(..), None, 0, 1500).is_empty());
    }

    #[test]
    fn member_context_limits_metadata_and_authority() {
        let item = lsp::CompletionItem {
            label: "lookup_by_id(int id)".into(),
            filter_text: Some("lookup_by_id".into()),
            detail: Some("const Record*".into()),
            insert_text: Some("DO_NOT_EXPORT_SNIPPET".into()),
            ..Default::default()
        };
        let result = MemberContext::from_response(
            lsp::CompletionResponse::Array(vec![item.clone()]),
            "look",
            200,
        );
        assert_eq!(result.names, Some(vec!["lookup_by_id".into()]));
        assert!(result.notes.contains("const Record*"));
        assert!(!result.notes.contains("DO_NOT_EXPORT"));
        assert!(result.notes.chars().count() <= 200);
        let result = MemberContext::from_response(
            lsp::CompletionResponse::List(lsp::CompletionList {
                is_incomplete: true,
                items: vec![item.clone()],
            }),
            "",
            200,
        );
        assert!(result.names.is_none());
        assert!(!result.notes.is_empty());
        let result = MemberContext::from_response(
            lsp::CompletionResponse::Array(vec![item.clone(); 129]),
            "",
            200,
        );
        assert!(result.names.is_none());
        let result = MemberContext::from_response(
            lsp::CompletionResponse::Array(vec![lsp::CompletionItem {
                deprecated: Some(true),
                ..item
            }]),
            "",
            200,
        );
        assert!(result.names.is_none());
        assert!(result.notes.is_empty());
        assert_eq!(member_stem("return ptr->look"), Some("look"));
        assert_eq!(member_stem("return 模块::"), Some(""));
        assert_eq!(member_stem("return value"), None);
    }

    #[test]
    fn namespace_members_do_not_exclude_enums_or_unknown_kinds() {
        let function = lsp::CompletionItem {
            label: "make".into(),
            kind: Some(lsp::CompletionItemKind::FUNCTION),
            ..Default::default()
        };
        let enumeration = lsp::CompletionItem {
            label: "Color".into(),
            kind: Some(lsp::CompletionItemKind::ENUM),
            ..Default::default()
        };
        let result = MemberContext::from_response(
            lsp::CompletionResponse::Array(vec![function.clone(), enumeration]),
            "",
            600,
        );
        assert!(result.names.unwrap().contains(&"Color".to_owned()));
        let unknown = lsp::CompletionItem {
            label: "unknown".into(),
            kind: Some(lsp::CompletionItemKind::KEYWORD),
            ..Default::default()
        };
        let result = MemberContext::from_response(
            lsp::CompletionResponse::Array(vec![function, unknown]),
            "",
            600,
        );
        assert!(result.names.is_none());
    }
}
