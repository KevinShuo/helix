use helix_core::{fold::function_candidates, syntax::Syntax, Rope};

fn candidates(language: &str, source: &str) -> Vec<(String, String)> {
    let loader = helix_core::config::default_lang_loader();
    let text = Rope::from(source);
    let language = loader.language_for_name(language).unwrap();
    let syntax = Syntax::new(text.slice(..), language, &loader).unwrap();
    function_candidates(text.slice(..), &syntax, &loader)
        .into_iter()
        .map(|candidate| {
            (
                text.slice(candidate.function.start..candidate.range.start)
                    .to_string(),
                text.slice(candidate.range).to_string(),
            )
        })
        .collect()
}

#[test]
fn python_preserves_decorators_and_complete_signature() {
    assert_eq!(
        candidates(
            "python",
            "@decorator\ndef hello(\n    name: str,\n) -> str:\n    return name\n\nnext_call()\n"
        ),
        [(
            "def hello(\n    name: str,\n) -> str:".into(),
            "\n    return name".into()
        )]
    );
}

#[test]
fn cpp_preserves_signature_initializers_and_closing_suffix() {
    assert_eq!(
        candidates(
            "cpp",
            "template<class T>\nT hello(\n  T name\n) {\n  return name;\n} // suffix\n"
        ),
        [(
            "T hello(\n  T name\n) {".into(),
            "\n  return name;\n".into()
        )]
    );
    assert_eq!(
        candidates(
            "cpp",
            "class Foo {\n Foo(int n)\n : value(n) {\n   value++;\n }\n int value;\n};\n"
        )
        .len(),
        1
    );
}

#[test]
fn functions_only_nested_and_no_indent_fallback() {
    let result = candidates(
        "python",
        "class A:\n def outer(self):\n  def inner():\n   pass\n  if True:\n   inner()\n",
    );
    assert_eq!(result.len(), 2);
    assert_eq!(result[1], ("def inner():".into(), "\n   pass".into()));
    assert!(candidates("cpp", "auto f = []() {\n return 1;\n};\n").is_empty());
}

#[test]
fn skips_single_line_empty_and_invalid_functions() {
    assert!(candidates("python", "def a(): pass\ndef b():\n").is_empty());
    assert!(candidates("cpp", "void a() {}\nvoid b() {\n}\nvoid c() {\n").is_empty());
}

#[test]
fn unicode_crlf_and_eof_do_not_change_boundaries() {
    assert_eq!(
        candidates("python", "def 问候(名字):\r\n\treturn '你好🙂'"),
        [("def 问候(名字):".into(), "\r\n\treturn '你好🙂'".into())]
    );
    assert_eq!(
        candidates("cpp", "void f() {\r\n\tcall(\"🙂\");\r\n}"),
        [("void f() {".into(), "\r\n\tcall(\"🙂\");\r\n".into())]
    );
}

#[test]
#[ignore = "fixed-size layout timing and allocation comparison; run with --ignored --nocapture"]
fn folding_large_body_layout_measurement() {
    use helix_core::{
        doc_formatter::{DocumentFormatter, FormattedGrapheme, TextFormat},
        text_annotations::TextAnnotations,
    };
    let source = format!(
        "void f() {{\n{}}}\nint tail;\n",
        "  call();\n".repeat(50_000)
    );
    let text = Rope::from(source);
    let end = text.line_to_char(50_001);
    let folds = [10..end];
    let text_format = TextFormat::default();
    for folded in [false, true] {
        let mut annotations = TextAnnotations::default();
        if folded {
            annotations.add_folds(&folds);
        }
        let started = std::time::Instant::now();
        let graphemes: Vec<_> = DocumentFormatter::new_at_prev_checkpoint(
            text.slice(..),
            &text_format,
            &annotations,
            0,
        )
        .collect();
        println!(
            "folded={folded}, source_chars={}, emitted={}, elapsed_us={}, output_capacity_bytes={}",
            text.len_chars(),
            graphemes.len(),
            started.elapsed().as_micros(),
            graphemes.capacity() * std::mem::size_of::<FormattedGrapheme>()
        );
        if folded {
            assert_eq!(graphemes.len(), 24);
        } else {
            assert_eq!(graphemes.len(), 500_024);
        }
    }
}
