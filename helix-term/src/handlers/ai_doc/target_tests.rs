use super::*;
use helix_core::{config::default_lang_loader, Rope};

pub(crate) fn find(lang: &str, source: &str, needle: &str) -> Result<Target> {
    let text = Rope::from(source);
    let loader = default_lang_loader();
    let syntax = Syntax::new(
        text.slice(..),
        loader.language_for_name(lang).unwrap(),
        &loader,
    )
    .unwrap();
    let cursor = source[..source.find(needle).unwrap()].chars().count();
    locate(text.slice(..), &syntax, cursor, lang)
}

#[test]
fn ai_doc_python_decorators_nested_receivers_and_types() {
    let source = "class C:\n    @classmethod\n    def work(cls, a: int, /, *args, value: str = 'x', **kwargs) -> str:\n        # keep\n        return value\n";
    let target = find("python", source, "@classmethod").unwrap();
    assert_eq!(target.name, "work");
    assert_eq!(
        target
            .parameters
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["a", "args", "value", "kwargs"]
    );
    assert_eq!(target.parameters[0].ty.as_deref(), Some("int"));
    assert_eq!(target.return_type.as_deref(), Some("str"));
    assert_eq!(
        target.insert,
        source[..source.find("        return").unwrap()]
            .chars()
            .count()
    );
    assert_eq!(
        find("python", "def f(self):\n    return self\n", "return")
            .unwrap()
            .parameters[0]
            .name,
        "self"
    );
    assert_eq!(
        find(
            "python",
            "class C:\n    @staticmethod\n    def f(self):\n        return self\n",
            "return"
        )
        .unwrap()
        .parameters[0]
            .name,
        "self"
    );
    let nested = "async def outer(a):\n    def inner(b):\n        return b\n    return a\n";
    assert_eq!(find("python", nested, "return b").unwrap().name, "inner");
}

#[test]
fn ai_doc_python_existing_docstrings_and_unsupported_callable() {
    for doc in [
        "'old'",
        "r'''old'''",
        "u\"old\"",
        "'old' ' more'",
        "('old')",
        "(('old' ' more'))",
    ] {
        assert!(find(
            "python",
            &format!("def f():\n    {doc}\n    return 1\n"),
            "return"
        )
        .is_err());
    }
    for expr in ["b'bytes'", "f'value'"] {
        assert!(find(
            "python",
            &format!("def f():\n    {expr}\n    return 1\n"),
            "return"
        )
        .is_ok());
    }
    assert!(find("python", "def f(): return 1", "return").is_err());
    assert!(find("python", "def f():\n    x = lambda a: a + 1\n", "a +").is_err());
}

#[test]
fn ai_doc_cpp_templates_attributes_constructors_and_pointer_parameters() {
    let source =
        "template <typename T>\n[[nodiscard]] T larger(const T& a, T b) { return a > b ? a : b; }";
    let target = find("cpp", source, "return").unwrap();
    assert_eq!(target.name, "larger");
    assert_eq!(target.templates, ["T"]);
    assert_eq!(target.insert, 0);
    assert_eq!(target.source, source);
    assert_eq!(
        target
            .parameters
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert!(find("cpp", "class C {\n    C(int x) {}\n};", "x)").is_ok_and(|t| !t.returns));
    assert!(find(
        "cpp",
        "void f() {\n    auto x = [](int a) { return a; };\n}",
        "return"
    )
    .is_err());
    let c = find(
        "c",
        "int f(int (*callback)(int), const char *name) { return callback(1); }",
        "return",
    )
    .unwrap();
    assert_eq!(
        c.parameters
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["callback", "name"]
    );
}

#[test]
fn ai_doc_c_existing_comments_separators_and_declarations() {
    assert!(find("c", "/* old */\nint f(void) { return 1; }", "return").is_err());
    assert!(find(
        "cpp",
        "// old\ntemplate<typename T>\nT f(T x) { return x; }",
        "return"
    )
    .is_err());
    assert!(find("c", "/* chapter */\n\nint f(void) { return 1; }", "return").is_ok());
    assert!(find("c", "int f(int x);", "f(").is_err());
    assert!(find("cpp", "int x; int f() { return 1; }", "return").is_err());
    assert!(find("cpp", "int f(int) { return 1; }", "return").is_err());
}

#[test]
fn ai_doc_eof_unicode_crlf_and_function_pointer_return() {
    let source = "def 函数(a):\r\n    return a";
    let target = find("python", source, "return").unwrap();
    assert_eq!(target.newline, "\r\n");
    assert_eq!(target.indent, "    ");
    let text = Rope::from(source);
    let loader = default_lang_loader();
    let syntax = Syntax::new(
        text.slice(..),
        loader.language_for_name("python").unwrap(),
        &loader,
    )
    .unwrap();
    assert!(locate(text.slice(..), &syntax, text.len_chars(), "python").is_ok());
    let target = find("c", "int (*factory(int x))(int) { return 0; }", "return").unwrap();
    assert_eq!(target.name, "factory");
    assert_eq!(target.parameters[0].name, "x");
}

#[test]
fn ai_doc_distinguishes_void_pointer_trailing_return_and_none() {
    assert!(
        find("c", "void *f(void) { return 0; }", "return")
            .unwrap()
            .returns
    );
    assert!(!find("cpp", "auto f() -> void {}", "f()").unwrap().returns);
    assert!(
        !find("python", "def f():\n    return None\n", "return")
            .unwrap()
            .returns
    );
    let target = find(
        "python",
        "def f(a):\n    if not a:\n        raise ValueError('empty')\n    return a\n",
        "return",
    )
    .unwrap();
    assert_eq!(target.exceptions, ["ValueError"]);
}

#[test]
fn ai_doc_host_tree_and_eof_do_not_jump_to_other_functions() {
    assert_eq!(
        find(
            "python",
            "def f(a):\n    x = r'(?P<name>hello)'\n    return a\n",
            "name"
        )
        .unwrap()
        .name,
        "f"
    );
    let source = "def f():\n    pass\n\n";
    let text = Rope::from(source);
    let loader = default_lang_loader();
    let syntax = Syntax::new(
        text.slice(..),
        loader.language_for_name("python").unwrap(),
        &loader,
    )
    .unwrap();
    assert!(locate(text.slice(..), &syntax, text.len_chars(), "python").is_err());
}

#[test]
fn ai_doc_template_header_is_part_of_the_target_signature() {
    let target = find(
        "cpp",
        "template<typename T>\nT identity(T value) { return value; }",
        "typename",
    )
    .unwrap();
    assert_eq!(target.name, "identity");
    assert_eq!(target.insert, 0);
}

#[test]
fn ai_doc_comments_inside_parameter_lists_are_not_parameters() {
    let python = find(
        "python",
        "def f(\n    a,  # input value\n):\n    return a\n",
        "return",
    )
    .unwrap();
    assert_eq!(python.parameters[0].name, "a");
    let cpp = find(
        "cpp",
        "template<typename T, /* value type */ typename U>\nT f(T a, U b) { return a; }",
        "return",
    )
    .unwrap();
    assert_eq!(cpp.templates, ["T", "U"]);
}

#[test]
fn ai_doc_nested_template_parameters_follow_source_order() {
    let target = find(
        "cpp",
        "template<class T>\ntemplate<class U>\nT Outer<T>::f(U value) { return T(value); }",
        "return",
    )
    .unwrap();
    assert_eq!(target.templates, ["T", "U"]);
}

#[test]
fn ai_doc_cpp_preserves_docs_between_template_header_and_function() {
    assert!(find(
        "cpp",
        "template<class T>\n// Return the input value.\nT f(T value) { return value; }",
        "return"
    )
    .is_err());
}

#[test]
fn ai_doc_python_parenthesized_docstring_may_contain_comments() {
    let source = "def f():\n    ( # existing documentation\n        'old'\n    )\n    return 1\n";
    assert!(find("python", source, "return").is_err());
}

#[test]
fn ai_doc_generator_semantics_override_none_annotation() {
    let target = find(
        "python",
        "def f(values) -> None:\n    yield from values\n",
        "yield",
    )
    .unwrap();
    assert!(
        target.returns,
        "a generator returns an iterator even with a misleading annotation"
    );
}

#[test]
fn ai_doc_parenthesized_none_has_no_meaningful_return() {
    let target = find("python", "def f():\n    return (None)\n", "return").unwrap();
    assert!(!target.returns);
}

#[test]
fn ai_doc_context_resolves_helpers_and_constants_without_other_scopes() {
    let source = "LIMIT = 8\nUNRELATED = 999\n\ndef normalize(value):\n    return min(value, LIMIT)\n\ndef target(value):\n    return normalize(value)\n\nclass Other:\n    def normalize(self, value):\n        return 'unrelated'\n";
    let target = find("python", source, "return normalize").unwrap();
    let request = crate::handlers::ai_doc::client::request(
        &helix_view::ai::AiProviderConfig {
            context_size: 16384,
            ..Default::default()
        },
        &helix_view::ai_doc::AiDocConfig::default(),
        &target,
    )
    .unwrap();
    let context = request["messages"][1]["content"].as_str().unwrap();
    assert!(context.contains("def normalize(value):"));
    assert!(context.contains("LIMIT = 8"));
    assert!(!context.contains("UNRELATED"));
    assert!(!context.contains("unrelated"));
    let input: serde_json::Value = serde_json::from_str(context).unwrap();
    assert!(!input["related"]
        .as_array()
        .unwrap()
        .iter()
        .any(|block| block.as_str() == Some(target.source.as_str())));
}

#[test]
fn ai_doc_related_context_respects_parameter_shadowing() {
    let source = "LIMIT = 999\n\ndef f(LIMIT):\n    return LIMIT + 1\n";
    let target = find("python", source, "return").unwrap();
    assert!(
        target.related.is_empty(),
        "a parameter does not reference the same-named global"
    );
}

#[test]
fn ai_doc_related_context_includes_cpp_macros_and_member_helpers() {
    let source = "#define LIMIT 8\nnamespace Demo {\nint cap(int value) { return value < LIMIT ? value : LIMIT; }\nint f(int value) { return cap(value); }\n}\n";
    let target = find("cpp", source, "return cap").unwrap();
    assert!(target
        .related
        .iter()
        .any(|s| s.contains("int cap(int value)")));
    assert!(target.related.iter().any(|s| s.contains("#define LIMIT 8")));
    let source = "class C:\n    ceiling = 8\n    def cap(self, value):\n        return min(value, self.ceiling)\n    def f(self, value):\n        return self.cap(value)\n\nclass Other:\n    def cap(self, value):\n        return 'wrong'\n";
    let target = find("python", source, "return self.cap").unwrap();
    let context = target.related.join("\n");
    assert!(context.contains("def cap(self, value):"));
    assert!(context.contains("ceiling = 8"));
    assert!(!context.contains("wrong"));
}

#[test]
fn ai_doc_callable_facts_do_not_leak_from_nested_functions() {
    let source = "async def f(value):\n    def inner():\n        yield 1\n    return value\n";
    let target = find("python", source, "return value").unwrap();
    assert!(target.facts.asynchronous);
    assert!(!target.facts.generator);
    assert!(target.facts.value_return);
    let target = find(
        "python",
        "async def f(values):\n    async for x in values:\n        yield x\n",
        "yield",
    )
    .unwrap();
    assert!(target.facts.asynchronous && target.facts.generator);
}

#[test]
fn ai_doc_helper_dependencies_resolve_in_the_helpers_own_scope() {
    let source = "LIMIT = 8\n\ndef cap(x):\n    return min(x, LIMIT)\n\nclass C:\n    LIMIT = 999\n    def f(self, x):\n        return cap(x)\n";
    let target = find("python", source, "return cap").unwrap();
    let context = target.related.join("\n");
    assert!(context.contains("LIMIT = 8"), "{context}");
    assert!(!context.contains("LIMIT = 999"), "{context}");
}

#[test]
fn ai_doc_bare_method_names_and_object_attributes_do_not_resolve_as_class_globals() {
    let source = "def helper(x):\n    return x + 1\n\nfetch = 999\n\nclass C:\n    def helper(self, x):\n        return 'wrong'\n    def f(self, backend, x):\n        backend.fetch(x)\n        return helper(x)\n";
    let target = find("python", source, "return helper").unwrap();
    let context = target.related.join("\n");
    assert!(context.contains("return x + 1"), "{context}");
    assert!(!context.contains("wrong"), "{context}");
    assert!(!context.contains("fetch = 999"), "{context}");
}

#[test]
fn ai_doc_local_binding_does_not_include_an_unrelated_global() {
    let source = "LIMIT = 999\n\ndef f():\n    LIMIT = 3\n    return LIMIT\n";
    assert!(find("python", source, "return").unwrap().related.is_empty());
}

#[test]
fn ai_doc_related_context_preserves_local_types_and_enum_error_codes() {
    let source =
        "struct Options { int limit; };\nint f() { Options options{}; return options.limit; }\n";
    let target = find("cpp", source, "return").unwrap();
    assert!(target.related.iter().any(|s| s.contains("struct Options")));
    let source = "enum Status { STATUS_OK = 0, STATUS_INVALID = -1 };\nint code(int valid) { return valid ? STATUS_OK : STATUS_INVALID; }\n";
    let target = find("c", source, "return").unwrap();
    assert!(target
        .related
        .iter()
        .any(|s| s.contains("STATUS_INVALID = -1")));
}
