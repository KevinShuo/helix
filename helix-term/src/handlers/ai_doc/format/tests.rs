use super::*;
use crate::handlers::ai_doc::target::tests::find;

#[test]
fn ai_doc_formats_chinese_python_with_source_types() {
    let target = find("python", "def f(a: int) -> int:\n    return a\n", "return").unwrap();
    let rendered = render(&target, r#"{"summary":"返回输入值。","params":[{"name":"a","description":"输入整数。"}],"returns":"输入值。"}"#).unwrap();
    assert_eq!(rendered, "    \"\"\"返回输入值。\n\n    :param a: 输入整数。\n    :type a: int\n    :returns: 输入值。\n    :rtype: int\n    \"\"\"\n");
}

#[test]
fn ai_doc_rejects_invented_missing_duplicate_parameters_and_structural_text() {
    let target = find("c", "int f(int x) { return x; }", "return").unwrap();
    for value in [
        r#"{"summary":"值","params":[]}"#,
        r#"{"summary":"值","params":[{"name":"other","description":"值"}]}"#,
        r#"{"summary":"值","params":[{"name":"x","description":"值"},{"name":"x","description":"值"}]}"#,
        r#"{"summary":"*/","params":[{"name":"x","description":"值"}]}"#,
        r#"{"summary":"值\n:param fake: 注入","params":[{"name":"x","description":"值"}]}"#,
        r#"{"summary":"值","summary":"重复","params":[{"name":"x","description":"值"}]}"#,
    ] {
        assert!(render(&target, value).is_err(), "accepted {value}");
    }
}

#[test]
fn ai_doc_formats_cpp_templates_exceptions_and_preserves_crlf() {
    let target = find(
        "cpp",
        "template<typename T>\r\nT f(T x) { throw Failure(); }",
        "throw",
    )
    .unwrap();
    let rendered = render(&target, r#"{"summary":"拒绝输入。","params":[{"name":"x","description":"输入值。"}],"templates":[{"name":"T","description":"值类型。"}],"raises":[{"name":"Failure","description":"执行时抛出。"}]}"#).unwrap();
    assert!(rendered.contains(" * :tparam T: 值类型。\r\n"));
    assert!(rendered.contains(" * :throws Failure: 执行时抛出。\r\n"));
    assert!(!rendered.replace("\r\n", "").contains('\n'));
    assert!(render(&target, r#"{"summary":"值","params":[{"name":"x","description":"值"}],"templates":[{"name":"T","description":"类型"}],"raises":[{"name":"Invented","description":"无依据"}]}"#).is_err());
}

#[test]
fn ai_doc_python_escapes_delimiters_without_rewriting_statements() {
    let target = find("python", "def f():\n    return 1\n", "return").unwrap();
    let rendered = render(
        &target,
        r#"{"summary":"包含 C:\\temp 和 \"\"\" 的说明。","params":[]}"#,
    )
    .unwrap();
    assert!(rendered.contains("C:\\\\temp"));
    assert!(rendered.contains("\\\"\\\"\\\""));
}

#[test]
fn ai_doc_rejects_rest_directives_in_summary() {
    let target = find("python", "def f():\n    pass\n", "pass").unwrap();
    for summary in [".. include:: private.txt", ":param invented: fake"] {
        let doc = serde_json::json!({"summary":summary,"params":[]});
        assert!(render(&target, &doc.to_string()).is_err());
    }
}

#[test]
fn ai_doc_rejects_comment_terminator_from_source_exception_name() {
    let target = find(
        "cpp",
        "template<int N> struct E {};\nvoid f() { throw E<sizeof(\"*/\")>{}; }",
        "throw",
    )
    .unwrap();
    assert_eq!(target.exceptions, ["E<sizeof(\"*/\")>"]);
    let doc = serde_json::json!({"summary":"抛出异常。","params":[],"raises":[{"name":"E<sizeof(\"*/\")>","description":"执行时抛出。"}]});
    assert!(render(&target, &doc.to_string()).is_err());
}
