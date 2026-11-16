use super::*;
use crate::handlers::ai_doc::target::tests::find;
use serde_json::json;

// Supply otherwise valid grounding so negative tests reach the guard they name.
fn evidenced(target: &Target, mut doc: serde_json::Value) -> String {
    if target.returns && doc["returns"].is_null() {
        doc["returns"] = json!("返回结果。");
    }
    let mut evidence = vec![json!({"field":"summary","quote":target.source})];
    if !doc["returns"].is_null() {
        evidence.push(json!({"field":"returns","quote":target.source}));
    }
    for (key, prefix) in [
        ("params", "param"),
        ("templates", "template"),
        ("raises", "raises"),
    ] {
        if let Some(entries) = doc[key].as_array() {
            for entry in entries {
                evidence.push(json!({"field":format!("{prefix}:{}", entry["name"].as_str().unwrap()),"quote":target.source}));
            }
        }
    }
    doc["evidence"] = json!(evidence);
    doc.to_string()
}

#[test]
fn ai_doc_formats_chinese_python_with_source_types() {
    let target = find("python", "def f(a: int) -> int:\n    return a\n", "return").unwrap();
    let rendered = render(&target, r#"{"summary":"返回输入值。","params":[{"name":"a","description":"输入整数。"}],"returns":"输入值。","evidence":[{"field":"summary","quote":"return a"},{"field":"param:a","quote":"a"},{"field":"returns","quote":"return a"}]}"#).unwrap();
    assert_eq!(rendered, "    \"\"\"返回输入值。\n\n    :param a: 输入整数。\n    :type a: int\n    :returns: 输入值。\n    :rtype: int\n    \"\"\"\n");
}

#[test]
fn ai_doc_rejects_invented_missing_duplicate_parameters_and_structural_text() {
    let target = find("c", "int f(int x) { return x; }", "return").unwrap();
    for (value, expected) in [
        (r#"{"summary":"值","params":[]}"#, "参数数量"),
        (
            r#"{"summary":"值","params":[{"name":"other","description":"值"}]}"#,
            "不存在的参数",
        ),
        (
            r#"{"summary":"值","params":[{"name":"x","description":"值"},{"name":"x","description":"值"}]}"#,
            "参数数量",
        ),
        (
            r#"{"summary":"*/","params":[{"name":"x","description":"值"}]}"#,
            "不安全",
        ),
        (
            r#"{"summary":"值\n:param fake: 注入","params":[{"name":"x","description":"值"}]}"#,
            "不安全",
        ),
    ] {
        let doc = evidenced(&target, serde_json::from_str(value).unwrap());
        assert!(
            render(&target, &doc)
                .unwrap_err()
                .to_string()
                .contains(expected),
            "{value}"
        );
    }
    assert!(
        render(&target, r#"{"summary":"值","summary":"重复","params":[]}"#)
            .unwrap_err()
            .to_string()
            .contains("JSON")
    );
    let target = find("c", "int f(int x, int y) { return x + y; }", "return").unwrap();
    let doc = evidenced(
        &target,
        json!({"summary":"返回和。","params":[{"name":"x","description":"值。"},{"name":"x","description":"值。"}]}),
    );
    assert!(render(&target, &doc)
        .unwrap_err()
        .to_string()
        .contains("重复参数"));
}

#[test]
fn ai_doc_formats_cpp_templates_exceptions_and_preserves_crlf() {
    let target = find(
        "cpp",
        "template<typename T>\r\nT f(T x) { throw Failure(); }",
        "throw",
    )
    .unwrap();
    let rendered = render(&target, r#"{"summary":"拒绝输入。","params":[{"name":"x","description":"输入值。"}],"templates":[{"name":"T","description":"值类型。"}],"raises":[{"name":"Failure","description":"执行时抛出。"}],"evidence":[{"field":"summary","quote":"throw Failure()"},{"field":"param:x","quote":"T x"},{"field":"template:T","quote":"typename T"},{"field":"raises:Failure","quote":"throw Failure()"}]}"#).unwrap();
    assert!(rendered.contains(" * :tparam T: 值类型。\r\n"));
    assert!(rendered.contains(" * :throws Failure: 执行时抛出。\r\n"));
    assert!(!rendered.replace("\r\n", "").contains('\n'));
    let doc = evidenced(
        &target,
        json!({"summary":"值","params":[{"name":"x","description":"值"}],"templates":[{"name":"T","description":"类型"}],"raises":[{"name":"Invented","description":"无依据"}]}),
    );
    assert!(render(&target, &doc)
        .unwrap_err()
        .to_string()
        .contains("无依据"));
}

#[test]
fn ai_doc_python_escapes_delimiters_without_rewriting_statements() {
    let target = find("python", "def f():\n    return 1\n", "return").unwrap();
    let rendered = render(
        &target,
        r#"{"summary":"包含 C:\\temp 和 \"\"\" 的说明。","params":[],"returns":"一。","evidence":[{"field":"summary","quote":"return 1"},{"field":"returns","quote":"return 1"}]}"#,
    )
    .unwrap();
    assert!(rendered.contains("C:\\\\temp"));
    assert!(rendered.contains("\\\"\\\"\\\""));
}

#[test]
fn ai_doc_rejects_rest_directives_in_summary() {
    let target = find("python", "def f():\n    pass\n", "pass").unwrap();
    for summary in [".. include:: private.txt", ":param invented: fake"] {
        let doc = evidenced(&target, json!({"summary":summary,"params":[]}));
        assert!(render(&target, &doc)
            .unwrap_err()
            .to_string()
            .contains("纯文本"));
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
    let doc = evidenced(
        &target,
        json!({"summary":"抛出异常。","params":[],"raises":[{"name":"E<sizeof(\"*/\")>","description":"执行时抛出。"}]}),
    );
    assert!(render(&target, &doc)
        .unwrap_err()
        .to_string()
        .contains("不安全"));
}

#[test]
fn ai_doc_accepts_grounded_descriptions_and_keeps_evidence_out_of_comment() {
    let target = find("python", "def f(value):\n    return value + 1\n", "return").unwrap();
    let content = r#"{"summary":"返回输入值加一。","params":[{"name":"value","description":"输入值。"}],"returns":"输入值加一。","evidence":[{"field":"summary","quote":"return value + 1"},{"field":"param:value","quote":"value"},{"field":"returns","quote":"return value + 1"}]}"#;
    let text = render(&target, content).unwrap();
    assert!(text.contains(":returns: 输入值加一。"));
    assert!(!text.contains("evidence"));
}

#[test]
fn ai_doc_rejects_missing_return_description() {
    let target = find("python", "def f():\n    return 1\n", "return").unwrap();
    let error = render(
        &target,
        r#"{"summary":"返回一。","params":[],"evidence":[{"field":"summary","quote":"return 1"}]}"#,
    )
    .unwrap_err();
    assert!(error.to_string().contains("returns"));
}

#[test]
fn ai_doc_does_not_emit_none_return_type_for_a_generator() {
    let target = find("python", "def f() -> None:\n    yield 1\n", "yield").unwrap();
    let text = render(&target, r#"{"summary":"逐个产生一。","params":[],"returns":"产生一的迭代器。","evidence":[{"field":"summary","quote":"yield 1"},{"field":"returns","quote":"yield 1"}]}"#).unwrap();
    assert!(!text.contains(":rtype: None"));
    assert!(text.contains(":returns:"));
}

#[test]
fn ai_doc_requires_evidence_for_each_description() {
    let target = find("python", "def f(value):\n    return value\n", "return").unwrap();
    for evidence in [
        json!([]),
        json!([{"field":"summary","quote":"return value"}]),
        json!([{"field":"summary","quote":"delete_all_files()"},{"field":"param:value","quote":"value"},{"field":"returns","quote":"return value"}]),
        json!([{"field":"summary","quote":"return value"},{"field":"param:value","quote":"value"},{"field":"returns","quote":"return value"},{"field":"param:invented","quote":"value"}]),
    ] {
        let content = json!({"summary":"返回输入。","params":[{"name":"value","description":"输入。"}],"returns":"输入。","evidence":evidence});
        assert!(render(&target, &content.to_string()).is_err());
    }
}
