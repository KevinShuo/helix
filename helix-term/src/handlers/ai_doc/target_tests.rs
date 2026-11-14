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
