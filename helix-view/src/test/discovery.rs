use crate::Document;
use helix_core::{syntax::Syntax, tree_sitter::Node, RopeSlice};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration {
    pub start_line: usize,
    pub line: usize,
    pub end_line: usize,
    pub name: String,
    pub suite: Option<String>,
    pub class: bool,
}
impl Declaration {
    pub fn matches_python(&self, id: &str) -> bool {
        // Split off parameter IDs first: IDs themselves can contain `::`.
        let id = id.split('[').next().unwrap_or(id);
        let parts: Vec<_> = id.split("::").skip(1).collect();
        if self.class {
            return parts.first().is_some_and(|s| *s == self.name);
        }
        parts.last().is_some_and(|s| *s == self.name)
            && match &self.suite {
                Some(suite) => parts.len() == 2 && parts[0] == suite,
                None => parts.len() == 1,
            }
    }
    pub fn matches_gtest(&self, id: &str) -> bool {
        let Some((suite, name)) = id.split_once('.') else {
            return false;
        };
        name.split('/').next() == Some(self.name.as_str())
            && self
                .suite
                .as_ref()
                .is_some_and(|s| suite.split('/').any(|part| part == s))
    }
}
pub fn discover(doc: &Document) -> Vec<Declaration> {
    let Some(syntax) = doc.syntax() else {
        return Vec::new();
    };
    discover_syntax(
        doc.text().slice(..),
        syntax,
        doc.language_name().unwrap_or_default(),
    )
}

fn field<'a>(node: &Node<'a>, name: &str) -> Option<Node<'a>> {
    let mut c = node.walk();
    if !c.goto_first_child() {
        return None;
    }
    loop {
        if c.field_name() == Some(name) {
            return Some(c.node());
        }
        if !c.goto_next_sibling() {
            return None;
        }
    }
}
fn source(text: RopeSlice, node: &Node) -> String {
    text.byte_slice(node.start_byte() as usize..node.end_byte() as usize)
        .to_string()
}
pub fn discover_syntax(text: RopeSlice, syntax: &Syntax, language: &str) -> Vec<Declaration> {
    if !matches!(language, "python" | "cpp" | "c") {
        return Vec::new();
    }
    fn visit(
        node: Node,
        text: RopeSlice,
        python: bool,
        suite: Option<&str>,
        result: &mut Vec<Declaration>,
    ) {
        if matches!(
            node.kind(),
            "comment" | "string" | "string_literal" | "concatenated_string"
        ) {
            return;
        }
        if python && node.kind() == "class_definition" {
            let Some(name) = field(&node, "name").map(|n| source(text, &n)) else {
                return;
            };
            let bases = field(&node, "superclasses")
                .map(|n| source(text, &n))
                .unwrap_or_default();
            if !name.starts_with("Test") && !bases.contains("TestCase") {
                return;
            }
            let start = node
                .parent()
                .filter(|p| p.kind() == "decorated_definition")
                .unwrap_or_else(|| node.clone());
            result.push(Declaration {
                start_line: text.byte_to_line(start.start_byte() as usize),
                line: text.byte_to_line(node.start_byte() as usize),
                end_line: text.byte_to_line(node.end_byte() as usize),
                name: name.clone(),
                suite: None,
                class: true,
            });
            if let Some(body) = field(&node, "body") {
                for child in body.children() {
                    visit(child, text, python, Some(&name), result);
                }
            }
            return;
        }
        if python && node.kind() == "function_definition" {
            if let Some(name) = field(&node, "name")
                .map(|n| source(text, &n))
                .filter(|s| s.starts_with("test"))
            {
                let start = node
                    .parent()
                    .filter(|p| p.kind() == "decorated_definition")
                    .unwrap_or_else(|| node.clone());
                result.push(Declaration {
                    start_line: text.byte_to_line(start.start_byte() as usize),
                    line: text.byte_to_line(node.start_byte() as usize),
                    end_line: text.byte_to_line(node.end_byte() as usize),
                    name,
                    suite: suite.map(str::to_owned),
                    class: false,
                });
            }
            return; // Never advertise a nested helper as a runnable test.
        }
        if !python && matches!(node.kind(), "function_definition" | "call_expression") {
            let value = source(text, &node);
            if let Some((mac, args)) = value.split_once('(') {
                if matches!(
                    mac.trim(),
                    "TEST" | "TEST_F" | "TEST_P" | "TYPED_TEST" | "TYPED_TEST_P"
                ) {
                    if let Some((suite, name)) = args
                        .split_once(')')
                        .and_then(|(args, _)| args.split_once(','))
                    {
                        let valid = |s: &str| {
                            !s.trim().is_empty()
                                && s.trim()
                                    .chars()
                                    .all(|c| c.is_ascii_alphanumeric() || c == '_')
                        };
                        if valid(suite) && valid(name) {
                            result.push(Declaration {
                                start_line: text.byte_to_line(node.start_byte() as usize),
                                line: text.byte_to_line(node.start_byte() as usize),
                                end_line: text.byte_to_line(node.end_byte() as usize),
                                name: name.trim().into(),
                                suite: Some(suite.trim().into()),
                                class: false,
                            });
                            return;
                        }
                    }
                }
            }
        }
        for child in node.children().filter(|c| c.is_named()) {
            visit(child, text, python, suite, result);
        }
    }
    let mut result = Vec::new();
    visit(
        syntax.tree().root_node(),
        text,
        language == "python",
        None,
        &mut result,
    );
    result.sort_by_key(|d| d.line);
    result.dedup_by_key(|d| d.line);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use helix_core::{config::default_lang_loader, syntax::Syntax, Rope};
    #[test]
    fn ast_declarations_exclude_comments_strings_and_nested_helpers() {
        for (lang, source, names) in [
            ("python", "# def test_fake(): pass\nclass MyCase(unittest.TestCase):\n    @decorator\n    def test_ok(self):\n        def test_nested(): pass\n        assert True\ndef test_top(): pass\n", vec!["MyCase", "test_ok", "test_top"]),
            ("cpp", "// TEST(Fake, Comment) {}\nconst char *s = \"TEST(Fake, String)\";\nTEST_F(Suite, Works) { EXPECT_TRUE(true); }\nTEST_P(Suite, Param) { EXPECT_TRUE(true); }\n", vec!["Works", "Param"]),
        ] {
            let text = Rope::from(source);
            let loader = default_lang_loader();
            let syntax = Syntax::new(text.slice(..), loader.language_for_name(lang).unwrap(), &loader).unwrap();
            let found = discover_syntax(text.slice(..), &syntax, lang);
            assert_eq!(found.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(), names);
            if lang == "python" { assert_eq!(found[1].start_line, 2); assert_eq!(found[1].line, 3); }
        }
    }
}
