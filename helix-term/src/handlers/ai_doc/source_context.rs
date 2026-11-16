//! Bounded lexical dependency context from the current buffer, without filesystem reads.
use super::{identifier, named, source, NodeFields};
use helix_core::{tree_sitter::Node, RopeSlice};
use std::collections::{HashMap, HashSet};

// The boolean distinguishes a known self/cls/this member from a bare name.
type Reference = (String, bool);
type Symbols<'a> = HashMap<String, Vec<Node<'a>>>;

fn references(node: Node<'_>, text: RopeSlice, out: &mut HashSet<Reference>) {
    if matches!(
        node.kind(),
        "identifier" | "field_identifier" | "type_identifier"
    ) {
        let mut member = false;
        let mut include = true;
        if let Some(parent) = node.parent() {
            let field = match parent.kind() {
                "attribute" => Some("attribute"),
                "field_expression" => Some("field"),
                _ => None,
            };
            if field
                .and_then(|field| parent.child_by_field_name(field))
                .is_some_and(|n| n.id() == node.id())
            {
                let object = parent
                    .child_by_field_name("object")
                    .or_else(|| parent.child_by_field_name("argument"));
                member = object
                    .is_some_and(|n| matches!(source(text, n).as_str(), "self" | "cls" | "this"));
                include = member;
            }
            // A qualified name is not an unqualified declaration of the same spelling.
            if parent.kind() == "qualified_identifier" {
                include = false;
            }
        }
        if include {
            out.insert((source(text, node.clone()), member));
        }
    }
    for child in named(node) {
        if !matches!(
            child.kind(),
            "function_definition"
                | "lambda"
                | "lambda_expression"
                | "class_definition"
                | "class_specifier"
        ) {
            references(child, text, out);
        }
    }
}

fn definition(mut node: Node<'_>) -> Node<'_> {
    while matches!(node.kind(), "decorated_definition" | "template_declaration") {
        let Some(inner) = named(node.clone())
            .into_iter()
            .find(|n| matches!(n.kind(), "function_definition" | "template_declaration"))
        else {
            break;
        };
        node = inner;
    }
    node
}

fn parameter_names(function: Node<'_>, text: RopeSlice) -> HashSet<String> {
    let params = function.child_by_field_name("parameters").or_else(|| {
        let mut candidate = function.child_by_field_name("declarator");
        while let Some(node) = candidate {
            if let Some(params) = node.child_by_field_name("parameters") {
                return Some(params);
            }
            candidate = node.child_by_field_name("declarator");
        }
        None
    });
    let mut names = HashSet::new();
    if let Some(params) = params {
        for param in named(params) {
            if param.kind() == "comment" {
                continue;
            }
            if let Some(name) = param
                .child_by_field_name("name")
                .or_else(|| param.child_by_field_name("declarator"))
                .or_else(|| {
                    named(param.clone()).into_iter().find(|n| {
                        matches!(
                            n.kind(),
                            "identifier" | "list_splat_pattern" | "dictionary_splat_pattern"
                        )
                    })
                })
                .or_else(|| (param.kind() == "identifier").then_some(param))
                .and_then(identifier)
            {
                names.insert(source(text, name));
            }
        }
    }
    names
}

fn local_names(node: Node<'_>, text: RopeSlice, out: &mut HashSet<String>) {
    out.extend(declared_names(node.clone(), text));
    if matches!(
        node.kind(),
        "function_definition"
            | "decorated_definition"
            | "class_definition"
            | "lambda"
            | "lambda_expression"
    ) {
        return;
    }
    if matches!(node.kind(), "for_statement" | "for_in_clause") {
        if let Some(left) = node
            .child_by_field_name("left")
            .filter(|n| n.kind() == "identifier")
        {
            out.insert(source(text, left));
        }
    }
    for child in named(node) {
        local_names(child, text, out);
    }
}

fn bindings(function: Node<'_>, text: RopeSlice) -> HashSet<String> {
    let mut names = parameter_names(function.clone(), text);
    if let Some(body) = function.child_by_field_name("body") {
        local_names(body.clone(), text, &mut names);
    }
    names
}

fn dependencies(node: Node<'_>, text: RopeSlice) -> HashSet<Reference> {
    let node = definition(node);
    let mut result = HashSet::new();
    references(node.clone(), text, &mut result);
    if node.kind() == "function_definition" {
        for name in bindings(node.clone(), text)
            .into_iter()
            .chain(declared_names(node, text))
        {
            result.remove(&(name, false));
        }
    }
    result
}

fn declared_names(node: Node<'_>, text: RopeSlice) -> Vec<String> {
    match node.kind() {
        "decorated_definition" => node
            .child_by_field_name("definition")
            .map(|n| declared_names(n, text))
            .unwrap_or_default(),
        "template_declaration" => named(node)
            .into_iter()
            .filter(|n| n.kind() != "template_parameter_list")
            .flat_map(|n| declared_names(n, text))
            .collect(),
        "function_definition" => node
            .child_by_field_name("name")
            .or_else(|| node.child_by_field_name("declarator").and_then(identifier))
            .map(|n| vec![source(text, n)])
            .unwrap_or_default(),
        "preproc_def"
        | "preproc_function_def"
        | "class_definition"
        | "class_specifier"
        | "struct_specifier"
        | "alias_declaration" => node
            .child_by_field_name("name")
            .map(|n| vec![source(text, n)])
            .unwrap_or_default(),
        "enum_specifier" => {
            let mut names = node
                .child_by_field_name("name")
                .map(|n| vec![source(text, n)])
                .unwrap_or_default();
            if let Some(body) = node.child_by_field_name("body") {
                names.extend(
                    named(body)
                        .into_iter()
                        .filter_map(|n| n.child_by_field_name("name"))
                        .map(|n| source(text, n)),
                );
            }
            names
        }
        "expression_statement" => named(node)
            .into_iter()
            .flat_map(|n| declared_names(n, text))
            .collect(),
        "assignment" | "annotated_assignment" => node
            .child_by_field_name("left")
            .filter(|n| n.kind() == "identifier")
            .map(|n| vec![source(text, n)])
            .unwrap_or_default(),
        "declaration" | "field_declaration" | "type_definition" => {
            let mut names = Vec::new();
            let mut cursor = node.walk();
            if cursor.goto_first_child() {
                loop {
                    if cursor.field_name() == Some("declarator") {
                        if let Some(name) = identifier(cursor.node()) {
                            names.push(source(text, name));
                        }
                    }
                    if !cursor.goto_next_sibling() {
                        break;
                    }
                }
            }
            if let Some(ty) = node
                .child_by_field_name("type")
                .filter(|n| n.kind() == "enum_specifier")
            {
                names.extend(declared_names(ty, text));
            }
            names
        }
        _ => Vec::new(),
    }
}

fn resolve<'a>(
    text: RopeSlice,
    origin: Node<'a>,
    reference: &Reference,
    python: bool,
    cache: &mut HashMap<usize, Symbols<'a>>,
) -> Vec<Node<'a>> {
    let mut scope = origin.parent();
    while let Some(node) = scope {
        let class = node.kind() == "field_declaration_list"
            || (node.kind() == "block"
                && node
                    .parent()
                    .is_some_and(|n| n.kind() == "class_definition"));
        let lexical = matches!(
            node.kind(),
            "block"
                | "compound_statement"
                | "field_declaration_list"
                | "declaration_list"
                | "module"
                | "translation_unit"
        );
        if lexical && (!reference.1 || class) && !(python && class && !reference.1) {
            let symbols = cache.entry(node.id()).or_insert_with(|| {
                let mut symbols: Symbols<'a> = HashMap::new();
                for declaration in named(node.clone()) {
                    for name in declared_names(declaration.clone(), text) {
                        symbols.entry(name).or_default().push(declaration.clone());
                    }
                }
                symbols
            });
            if let Some(declarations) = symbols.get(&reference.0) {
                return declarations.clone();
            }
            if reference.1 && class {
                return Vec::new();
            }
            // Unknown local/closure values must not fall through to an unrelated global.
            if python && node.kind() == "block" {
                if let Some(function) = node.parent().filter(|n| n.kind() == "function_definition")
                {
                    if bindings(function, text).contains(&reference.0) {
                        return Vec::new();
                    }
                }
            }
        }
        scope = node.parent();
    }
    Vec::new()
}

pub(super) fn collect(text: RopeSlice, target: Node<'_>) -> Vec<String> {
    let mut root = target.clone();
    while let Some(parent) = root.parent() {
        root = parent;
    }
    let python = root.kind() == "module";
    let mut cache = HashMap::new();
    let mut origins = vec![target.clone()];
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    let mut used = 0;
    for _ in 0..3 {
        let mut candidates = Vec::new();
        for origin in origins {
            for reference in dependencies(origin.clone(), text) {
                candidates.extend(resolve(
                    text,
                    origin.clone(),
                    &reference,
                    python,
                    &mut cache,
                ));
            }
        }
        candidates.sort_by_key(|n| n.start_byte());
        let mut next = Vec::new();
        for node in candidates {
            let size = (node.end_byte() - node.start_byte()) as usize;
            if !seen.insert(node.id())
                || size > 4096
                || used + size > 8192
                || result.len() >= 12
                || (node.start_byte() <= target.start_byte()
                    && node.end_byte() >= target.end_byte())
            {
                continue;
            }
            if node.has_error() {
                continue;
            }
            used += size;
            result.push(source(text, node.clone()));
            next.push(node);
        }
        if next.is_empty() {
            break;
        }
        origins = next;
    }
    result
}
