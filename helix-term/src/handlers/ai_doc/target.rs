use anyhow::{bail, ensure, Context, Result};
use helix_core::{syntax::Syntax, tree_sitter::Node, RopeSlice};

pub(crate) use helix_view::ai_doc::{Parameter, Target};

fn named(node: Node<'_>) -> Vec<Node<'_>> {
    node.children().filter(|n| n.is_named()).collect()
}

fn source(text: RopeSlice, node: Node<'_>) -> String {
    text.byte_slice(node.start_byte() as usize..node.end_byte() as usize)
        .to_string()
}

fn identifier(node: Node<'_>) -> Option<Node<'_>> {
    match node.kind() {
        "identifier" | "field_identifier" | "type_identifier" | "operator_name"
        | "destructor_name" => Some(node),
        "qualified_identifier" => node.child_by_field_name("name").and_then(identifier),
        _ => node
            .child_by_field_name("declarator")
            .and_then(identifier)
            .or_else(|| {
                named(node.clone())
                    .into_iter()
                    .filter(|n| n.kind() != "parameter_list")
                    .find_map(identifier)
            }),
    }
}

fn callable(node: &Node<'_>) -> bool {
    matches!(
        node.kind(),
        "function_definition" | "lambda" | "lambda_expression"
    )
}

fn walk_body<'a>(node: Node<'a>, visit: &mut impl FnMut(Node<'a>)) {
    visit(node.clone());
    for child in named(node.clone()) {
        if !callable(&child) {
            walk_body(child, visit);
        }
    }
}

fn is_docstring(node: Node<'_>, text: RopeSlice) -> bool {
    match node.kind() {
        "expression_statement" | "parenthesized_expression" => named(node.clone())
            .into_iter()
            .find(|n| n.kind() != "comment")
            .is_some_and(|n| is_docstring(n, text)),
        "concatenated_string" => named(node.clone())
            .into_iter()
            .filter(|n| n.kind() != "comment")
            .all(|n| is_docstring(n, text)),
        "string" => {
            let value = source(text, node.clone()).to_ascii_lowercase();
            let prefix = value.split(['\'', '"']).next().unwrap_or_default();
            !prefix.contains(['b', 'f'])
        }
        _ => false,
    }
}

pub(crate) fn locate(
    text: RopeSlice,
    syntax: &Syntax,
    cursor: usize,
    language: &str,
) -> Result<Target> {
    ensure!(
        matches!(language, "c" | "cpp" | "python"),
        "仅支持 C、C++ 和 Python"
    );
    ensure!(
        cursor <= text.len_chars() && text.len_chars() > 0,
        "光标不在函数中"
    );
    let pos = if cursor == text.len_chars() {
        cursor - 1
    } else {
        cursor
    };
    let byte = text.char_to_byte(pos) as u32;
    let mut node = syntax
        .tree()
        .root_node()
        .descendant_for_byte_range(byte, byte)
        .context("光标不在函数中")?;
    // Decorators are siblings of the wrapped function, not its descendants.
    let function = loop {
        if callable(&node) {
            break node.clone();
        }
        if node.kind() == "decorated_definition" {
            break node
                .child_by_field_name("definition")
                .filter(|n| n.kind() == "function_definition")
                .context("光标不在函数中")?;
        }
        if node.kind() == "template_declaration" {
            let mut declaration = node.clone();
            loop {
                declaration = declaration
                    .children()
                    .find(|child| {
                        matches!(child.kind(), "function_definition" | "template_declaration")
                    })
                    .context("模板不是函数定义")?;
                if declaration.kind() == "function_definition" {
                    break;
                }
            }
            break declaration;
        }
        node = node.parent().context("光标不在函数中")?;
    };
    ensure!(
        function.kind() == "function_definition",
        "暂不支持匿名函数，请将光标移出 lambda"
    );
    ensure!(!function.has_error(), "函数语法不完整，无法可靠生成注释");
    ensure!(
        function.byte_range().contains(&byte)
            || matches!(node.kind(), "decorated_definition" | "template_declaration"),
        "光标不在函数中"
    );
    let body = function
        .child_by_field_name("body")
        .context("暂不支持没有函数体的声明")?;
    let mut wrapper = function.clone();
    while let Some(parent) = wrapper
        .parent()
        .filter(|p| matches!(p.kind(), "template_declaration" | "decorated_definition"))
    {
        wrapper = parent;
    }
    let start = text.byte_to_char(wrapper.start_byte() as usize);
    let end = text.byte_to_char(wrapper.end_byte() as usize);
    let newline = if text.to_string().contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
    .to_owned();
    let mut target = Target {
        language: language.into(),
        name: String::new(),
        source: source(text, wrapper.clone()),
        supporting: String::new(),
        range: start..end,
        insert: 0,
        indent: String::new(),
        newline,
        parameters: Vec::new(),
        templates: Vec::new(),
        return_type: None,
        returns: false,
        exceptions: Vec::new(),
        line: text.byte_to_line(wrapper.start_byte() as usize) + 1,
    };
    if language == "python" {
        target.name = source(
            text,
            function
                .child_by_field_name("name")
                .context("函数缺少名称")?,
        );
        let first = named(body.clone())
            .into_iter()
            .find(|n| n.kind() != "comment")
            .context("函数体为空")?;
        ensure!(
            !is_docstring(first.clone(), text),
            "当前函数已有文档注释，未修改"
        );
        ensure!(
            text.byte_to_line(first.start_byte() as usize)
                > text.byte_to_line(function.start_byte() as usize),
            "请先将单行 Python 函数改成多行函数"
        );
        target.insert = text.line_to_char(text.byte_to_line(first.start_byte() as usize));
        target.indent = text
            .slice(target.insert..text.byte_to_char(first.start_byte() as usize))
            .to_string();
        ensure!(
            target.indent.chars().all(|c| matches!(c, ' ' | '\t')) && !target.indent.is_empty(),
            "无法确定函数体缩进"
        );
        let params = function
            .child_by_field_name("parameters")
            .context("函数缺少参数列表")?;
        for param in named(params) {
            if matches!(
                param.kind(),
                "positional_separator" | "keyword_separator" | "comment"
            ) {
                continue;
            }
            let ty = param.child_by_field_name("type").map(|n| {
                source(text, n)
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            });
            let name_node = param
                .child_by_field_name("name")
                .or_else(|| {
                    if param.kind() == "identifier" {
                        Some(param.clone())
                    } else {
                        named(param.clone()).into_iter().find(|n| {
                            matches!(
                                n.kind(),
                                "identifier" | "list_splat_pattern" | "dictionary_splat_pattern"
                            )
                        })
                    }
                })
                .and_then(identifier)
                .context("无法确定 Python 参数名")?;
            target.parameters.push(Parameter {
                name: source(text, name_node),
                ty,
            });
        }
        let method = wrapper
            .parent()
            .and_then(|p| p.parent())
            .is_some_and(|p| p.kind() == "class_definition");
        let decorators: Vec<_> = named(wrapper.clone())
            .into_iter()
            .filter(|n| n.kind() == "decorator")
            .map(|n| source(text, n))
            .collect();
        let receiver = if decorators.iter().any(|d| d == "@classmethod") {
            Some("cls")
        } else if decorators.is_empty() {
            Some("self")
        } else {
            None
        };
        if method
            && target
                .parameters
                .first()
                .is_some_and(|p| Some(p.name.as_str()) == receiver)
        {
            target.parameters.remove(0);
        }
        target.return_type = function.child_by_field_name("return_type").map(|n| {
            source(text, n)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        });
        walk_body(body, &mut |n| {
            if (n.kind() == "return_statement"
                && named(n.clone())
                    .first()
                    .is_some_and(|value| source(text, value.clone()) != "None"))
                || n.kind() == "yield"
            {
                target.returns = true;
            }
            if n.kind() == "raise_statement" {
                if let Some(value) = named(n.clone()).first().cloned() {
                    let exception = value.child_by_field_name("function").unwrap_or(value);
                    if matches!(exception.kind(), "identifier" | "attribute") {
                        target.exceptions.push(source(text, exception));
                    }
                }
            }
        });
        if target.return_type.as_deref() == Some("None") {
            target.returns = false;
        }
    } else {
        let line_start = text.line_to_char(text.char_to_line(start));
        target.indent = text.slice(line_start..start).to_string();
        ensure!(
            target.indent.chars().all(|c| matches!(c, ' ' | '\t')),
            "函数与其他代码共享同一行，暂不支持"
        );
        let line_end = text
            .line(text.char_to_line(end.saturating_sub(1)))
            .to_string();
        let end_column = text
            .slice(text.line_to_char(text.char_to_line(end.saturating_sub(1)))..end)
            .len_chars();
        ensure!(
            line_end.chars().skip(end_column).all(char::is_whitespace),
            "函数与其他代码共享同一行，暂不支持"
        );
        target.insert = line_start;
        let mut declaration = function.clone();
        loop {
            if let Some(previous) = declaration
                .prev_named_sibling()
                .filter(|n| n.kind() == "comment")
            {
                let gap = text
                    .byte_slice(previous.end_byte() as usize..declaration.start_byte() as usize)
                    .to_string();
                if gap.chars().all(char::is_whitespace) && gap.matches('\n').count() <= 1 {
                    bail!("当前函数前已有注释，未修改");
                }
            }
            if declaration.id() == wrapper.id() {
                break;
            }
            declaration = declaration.parent().context("无法确定函数声明边界")?;
        }
        let mut declarator = function
            .child_by_field_name("declarator")
            .context("无法识别函数声明")?;
        let mut signature = None;
        let mut return_pointer = false;
        loop {
            return_pointer |= declarator.kind() == "pointer_declarator";
            if declarator.kind() == "function_declarator" {
                signature = Some(declarator.clone());
            }
            match declarator.child_by_field_name("declarator") {
                Some(next) => declarator = next,
                None if declarator.kind() == "parenthesized_declarator" => {
                    declarator = named(declarator)
                        .into_iter()
                        .next()
                        .context("无法识别函数声明")?;
                }
                _ => break,
            }
        }
        let signature = signature.context("暂不支持此函数声明形式")?;
        target.name = source(
            text,
            signature
                .child_by_field_name("declarator")
                .and_then(identifier)
                .context("无法确定函数名称")?,
        );
        for param in named(
            signature
                .child_by_field_name("parameters")
                .context("函数缺少参数列表")?,
        ) {
            if param.kind() == "comment" {
                continue;
            }
            if source(text, param.clone()) == "void" {
                continue;
            }
            let name = param
                .child_by_field_name("declarator")
                .and_then(identifier)
                .context("暂不支持未命名参数或可变参数省略号")?;
            target.parameters.push(Parameter {
                name: source(text, name),
                ty: None,
            });
        }
        target.return_type = function
            .child_by_field_name("type")
            .map(|n| source(text, n));
        target.returns = target
            .return_type
            .as_deref()
            .is_some_and(|t| t != "void" || return_pointer);
        if let Some(trailing) = signature
            .children()
            .find(|n| n.kind() == "trailing_return_type")
        {
            target.returns = source(text, trailing).trim_start_matches("->").trim() != "void";
        }
        let mut template_groups = Vec::new();
        let mut parent = function.parent();
        while let Some(n) = parent.filter(|n| n.kind() == "template_declaration") {
            let mut group = Vec::new();
            if let Some(params) = n.child_by_field_name("parameters") {
                for param in named(params) {
                    if param.kind() == "comment" {
                        continue;
                    }
                    let name = param
                        .child_by_field_name("name")
                        .or_else(|| param.child_by_field_name("declarator").and_then(identifier))
                        .or_else(|| {
                            named(param.clone())
                                .into_iter()
                                .find(|n| matches!(n.kind(), "type_identifier" | "identifier"))
                        })
                        .context("暂不支持未命名模板参数")?;
                    group.push(source(text, name));
                }
            }
            template_groups.push(group);
            parent = n.parent();
        }
        target.templates = template_groups.into_iter().rev().flatten().collect();
        walk_body(body, &mut |n| {
            if n.kind() == "throw_statement" {
                if let Some(value) = named(n.clone()).first().cloned() {
                    let ty = value
                        .child_by_field_name("function")
                        .or_else(|| value.child_by_field_name("type"));
                    if let Some(ty) = ty {
                        target.exceptions.push(source(text, ty));
                    }
                }
            }
        });
    }
    target.exceptions.sort();
    target.exceptions.dedup();
    // Bounded same-file context, independent of cursor injection layers.
    let mut parent = wrapper.parent();
    while let Some(node) = parent {
        if matches!(
            node.kind(),
            "class_definition" | "class_specifier" | "struct_specifier" | "namespace_definition"
        ) {
            if let Some(body) = node.child_by_field_name("body") {
                let header = text
                    .byte_slice(node.start_byte() as usize..body.start_byte() as usize)
                    .to_string();
                if target.supporting.len() + header.len() < 1000 {
                    target.supporting.push_str(&header);
                    target.supporting.push('\n');
                }
            }
        }
        parent = node.parent();
    }
    for node in named(syntax.tree().root_node()) {
        if matches!(
            node.kind(),
            "import_statement" | "import_from_statement" | "preproc_include" | "type_definition"
        ) {
            let note = source(text, node.clone());
            if target.supporting.len() + note.len() < 1000 {
                target.supporting.push_str(&note);
                target.supporting.push('\n');
            }
        }
    }
    Ok(target)
}

#[cfg(test)]
#[path = "target_tests.rs"]
pub(crate) mod tests;

// The repository uses tree-house bindings rather than tree-sitter's Node API.
trait NodeFields<'a> {
    fn child_by_field_name(&self, name: &str) -> Option<Node<'a>>;
    fn has_error(&self) -> bool;
}
impl<'a> NodeFields<'a> for Node<'a> {
    fn child_by_field_name(&self, name: &str) -> Option<Node<'a>> {
        let mut cursor = self.walk();
        if !cursor.goto_first_child() {
            return None;
        }
        loop {
            if cursor.field_name() == Some(name) {
                return Some(cursor.node());
            }
            if !cursor.goto_next_sibling() {
                return None;
            }
        }
    }
    fn has_error(&self) -> bool {
        self.kind() == "ERROR" || self.is_missing() || self.children().any(|n| n.has_error())
    }
}
