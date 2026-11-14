use super::*;
use helix_view::ai_commit::CommitLanguage;
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::mpsc,
    time::Duration,
};

fn file(id: usize) -> AiFileInput {
    AiFileInput {
        id,
        path: format!("src/{id}.rs"),
        staged: false,
        partial: false,
        diff: "@@ -1 +1 @@\n-old\n+new\n".into(),
        eligible: true,
    }
}

fn enabled() -> AiCommitConfig {
    AiCommitConfig {
        enabled: true,
        ..Default::default()
    }
}

// The only fake is the external HTTP service. Planning, transport, response
// validation, formatting and privacy filtering all run as production code.
fn provider(responses: Vec<Value>) -> (AiCompletionConfig, mpsc::Receiver<Value>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!(
        "http://{}/v1/chat/completions",
        listener.local_addr().unwrap()
    );
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for response in responses {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut bytes = Vec::new();
            let header_end = loop {
                let mut buffer = [0; 2048];
                let count = socket.read(&mut buffer).unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&buffer[..count]);
                if let Some(offset) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    break offset + 4;
                }
            };
            let headers = String::from_utf8_lossy(&bytes[..header_end]);
            let length: usize = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|s| s.trim().parse().unwrap())
                })
                .unwrap();
            while bytes.len() < header_end + length {
                let mut buffer = [0; 2048];
                let count = socket.read(&mut buffer).unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&buffer[..count]);
            }
            let _ =
                tx.send(serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap());
            let body = json!({"choices":[{"finish_reason":"stop","message":{"content":response.to_string()}}]}).to_string();
            let status = response["_http_status"].as_u64().unwrap_or(200);
            write!(socket, "HTTP/1.1 {status} Response\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
    });
    (
        AiCompletionConfig {
            endpoint,
            enabled: true,
            ..Default::default()
        },
        rx,
    )
}

#[test]
fn ai_commit_manual_messages_allow_breaking_footers_and_english_scopes() {
    for valid in [
        "feat(ai): 支持生成提交信息",
        "fix: correct index snapshot",
        "refactor(core)!: change API\n\nBREAKING CHANGE: callers must migrate",
    ] {
        validate_message(valid).unwrap();
    }
    for invalid in [
        "",
        "feat: ",
        "unknown: subject",
        "feat(中文): subject",
        "feat: subject\u{1b}[31m",
        "feat: subject\nbody without blank line",
    ] {
        assert!(validate_message(invalid).is_err(), "accepted {invalid:?}");
    }
}

#[tokio::test]
async fn ai_commit_disabled_and_empty_requests_fail_without_contacting_provider() {
    let model = AiCompletionConfig {
        endpoint: "invalid://should-not-connect".into(),
        ..Default::default()
    };
    assert!(recommend(&model, &AiCommitConfig::default(), &[], "")
        .await
        .is_err());
    assert!(generate_message(&model, &enabled(), &[], "").await.is_err());
}

#[tokio::test]
async fn ai_commit_recommends_from_complete_diff_and_filters_sensitive_contents() {
    let (model, rx) = provider(vec![
        json!({"summaries":[{"id":1,"summary":"更新实现"}]}),
        json!({"selected":[1],"reasons":[{"id":1,"reason":"实现同一功能"},{"id":2,"reason":"需手动检查"}]}),
    ]);
    let mut sensitive = file(2);
    sensitive.path = ".env.production".into();
    sensitive.diff = "SUPER_SECRET_PASSWORD".into();
    let result = recommend(&model, &enabled(), &[file(1), sensitive], "优化实现")
        .await
        .unwrap();
    assert_eq!(result.selected, vec![1]);
    assert!(result.incomplete);
    assert_eq!(result.reasons.len(), 2);
    let first = rx.recv().unwrap();
    let data: Value =
        serde_json::from_str(first["messages"][1]["content"].as_str().unwrap()).unwrap();
    assert_eq!(data["files"][0]["diff"], "@@ -1 +1 @@\n-old\n+new\n");
    for body in [first, rx.recv().unwrap()] {
        assert!(!body.to_string().contains("SUPER_SECRET_PASSWORD"));
        assert_eq!(body["max_tokens"], 768);
        let messages = body["messages"].as_array().unwrap();
        let bytes: usize = messages
            .iter()
            .map(|m| m["content"].as_str().unwrap().len())
            .sum();
        assert!(bytes + 768 + 256 <= model.context_size as usize);
    }
}

#[tokio::test]
async fn ai_commit_rejects_unknown_duplicate_and_partial_recommendations() {
    for selected in [vec![99], vec![1, 1], vec![2]] {
        let (model, _) = provider(vec![
            json!({"summaries":[{"id":1,"summary":"first"},{"id":2,"summary":"partial"}]}),
            json!({"selected":selected,"reasons":[{"id":1,"reason":"one"},{"id":2,"reason":"two"}]}),
        ]);
        let mut partial = file(2);
        partial.partial = true;
        assert!(recommend(&model, &enabled(), &[file(1), partial], "")
            .await
            .is_err());
    }
}

#[tokio::test]
async fn ai_commit_generates_staged_message_in_selected_language_without_test_claims() {
    for language in [CommitLanguage::Chinese, CommitLanguage::English] {
        let subject = if language == CommitLanguage::Chinese {
            "添加提交工作区"
        } else {
            "add commit workspace"
        };
        let (model, rx) = provider(vec![
            json!({"summaries":[{"id":1,"summary":"add workspace"}]}),
            json!({"type":"feat","scope":"git","subject":subject,"body":"Allow explicit staging"}),
        ]);
        let mut staged = file(1);
        staged.staged = true;
        let config = AiCommitConfig {
            language,
            ..enabled()
        };
        let result = generate_message(&model, &config, &[staged], "")
            .await
            .unwrap();
        assert_eq!(
            result.text,
            format!("feat(git): {subject}\n\nAllow explicit staging")
        );
        assert!(!result.incomplete);
        rx.recv().unwrap();
        let body = rx.recv().unwrap();
        assert!(body["messages"][0]["content"].as_str().unwrap().contains(
            if language == CommitLanguage::Chinese {
                "Simplified Chinese"
            } else {
                "English"
            }
        ));
    }
    let (model, _) = provider(vec![
        json!({"summaries":[{"id":1,"summary":"change"}]}),
        json!({"type":"fix","scope":null,"subject":"fix bug","body":"All tests passed"}),
    ]);
    let mut staged = file(1);
    staged.staged = true;
    assert!(generate_message(&model, &enabled(), &[staged], "")
        .await
        .is_err());
}

#[tokio::test]
async fn ai_commit_oversize_diff_is_metadata_only_and_message_is_incomplete() {
    let (model, rx) = provider(vec![
        json!({"type":"chore","scope":null,"subject":"review manual change","body":null}),
    ]);
    let mut staged = file(1);
    staged.staged = true;
    staged.diff = "PRIVATE_OVERSIZE_CONTENT".repeat(500);
    let result = generate_message(&model, &enabled(), &[staged], "")
        .await
        .unwrap();
    assert!(result.incomplete);
    assert!(!rx
        .recv()
        .unwrap()
        .to_string()
        .contains("PRIVATE_OVERSIZE_CONTENT"));
}

#[tokio::test]
async fn ai_commit_refuses_over_file_limit_and_duplicate_ids_before_http() {
    let model = AiCompletionConfig::default();
    let files: Vec<_> = (0..65).map(file).collect();
    assert!(recommend(&model, &enabled(), &files, "")
        .await
        .unwrap_err()
        .to_string()
        .contains("64"));
    assert!(recommend(&model, &enabled(), &[file(1), file(1)], "")
        .await
        .is_err());
}

#[tokio::test]
async fn ai_commit_rejects_malformed_json_unknown_summary_ids_and_invalid_types() {
    for summary in [
        json!("not an object"),
        json!({"summaries":[{"id":99,"summary":"wrong"}]}),
        json!({"summaries":[{"id":1,"summary":"one","command":"git add ."}]}),
        json!({"summaries":[]}),
    ] {
        let (model, _) = provider(vec![summary]);
        assert!(recommend(&model, &enabled(), &[file(1)], "").await.is_err());
    }
    for message in [
        json!({"type":"feat!","scope":null,"subject":"change","body":null}),
        json!({"type":"fix","scope":"中文","subject":"change","body":null}),
        json!({"type":"fix","scope":null,"subject":"one\ntwo","body":null}),
    ] {
        let (model, _) = provider(vec![
            json!({"summaries":[{"id":1,"summary":"change"}]}),
            message,
        ]);
        let mut staged = file(1);
        staged.staged = true;
        assert!(generate_message(&model, &enabled(), &[staged], "")
            .await
            .is_err());
    }
}

#[tokio::test]
async fn ai_commit_unsupported_objects_never_send_contents_or_auto_select() {
    for (path, diff, eligible) in [
        ("binary.png", "GIT binary patch\nSECRET_BINARY", true),
        ("link", "new file mode 120000\nSECRET_LINK", true),
        ("submodule", "Subproject commit SECRET_SUBMODULE", true),
        ("credentials.json", "SECRET_CREDENTIAL", true),
        ("src/file", "SECRET_INELIGIBLE", false),
    ] {
        let (model, rx) = provider(vec![
            json!({"selected":[],"reasons":[{"id":1,"reason":"manual review"}]}),
        ]);
        let input = AiFileInput {
            path: path.into(),
            diff: diff.into(),
            eligible,
            ..file(1)
        };
        let result = recommend(&model, &enabled(), &[input], "").await.unwrap();
        assert!(result.incomplete && result.selected.is_empty());
        assert!(!rx.recv().unwrap().to_string().contains("SECRET_"));
    }
}

#[tokio::test]
async fn ai_commit_request_cap_marks_unanalyzed_file_and_never_exceeds_64_calls() {
    let mut responses = (0..63)
        .map(|id| json!({"summaries":[{"id":id,"summary":"x"}]}))
        .collect::<Vec<_>>();
    responses.push(json!({"selected":[],"reasons":(0..64).map(|id|json!({"id":id,"reason":"review"})).collect::<Vec<_>>()}));
    let (mut model, rx) = provider(responses);
    model.context_size = 16_384;
    let files = (0..64)
        .map(|id| AiFileInput {
            path: "x".into(),
            diff: "+change\n".repeat(1500),
            ..file(id)
        })
        .collect::<Vec<_>>();
    let config = AiCommitConfig {
        max_input_bytes: 16_384,
        ..enabled()
    };
    let result = recommend(&model, &config, &files, "").await.unwrap();
    assert!(result.incomplete);
    assert_eq!(rx.try_iter().count(), 64);
}

#[tokio::test]
async fn ai_commit_actual_model_window_and_aggregation_budget_are_checked() {
    let model = AiCompletionConfig {
        context_size: 64,
        endpoint: "invalid://no-http".into(),
        ..Default::default()
    };
    let error = recommend(&model, &enabled(), &[file(1)], "")
        .await
        .unwrap_err();
    assert!(error.to_string().contains("context-size"));
    let model = AiCompletionConfig {
        endpoint: "invalid://no-http".into(),
        ..Default::default()
    };
    let error = recommend(&model, &enabled(), &[file(1)], &"a".repeat(10_000))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("max-input-bytes"));
    assert!(generate_message(&model, &enabled(), &[file(1)], "")
        .await
        .unwrap_err()
        .to_string()
        .contains("staged"));
}

#[tokio::test]
async fn ai_commit_edge_reduces_server_rejected_batches_without_truncation() {
    let (model, rx) = provider(vec![
        json!({"_http_status":413}),
        json!({"summaries":[{"id":1,"summary":"one"}]}),
        json!({"summaries":[{"id":2,"summary":"two"}]}),
        json!({"selected":[1,2],"reasons":[{"id":1,"reason":"one"},{"id":2,"reason":"two"}]}),
    ]);
    let result = recommend(&model, &enabled(), &[file(1), file(2)], "")
        .await
        .unwrap();
    assert!(!result.incomplete);
    let requests = rx.try_iter().collect::<Vec<_>>();
    assert_eq!(requests.len(), 4);
    for (index, expected) in [(0, 2), (1, 1), (2, 1)] {
        let data: Value =
            serde_json::from_str(requests[index]["messages"][1]["content"].as_str().unwrap())
                .unwrap();
        assert_eq!(data["files"].as_array().unwrap().len(), expected);
        assert_eq!(data["files"][0]["diff"], "@@ -1 +1 @@\n-old\n+new\n");
    }
}

#[tokio::test]
async fn ai_commit_edge_respects_small_output_budget_and_allows_password_tests() {
    let (model, rx) = provider(vec![
        json!({"summaries":[{"id":1,"summary":"add password tests"}]}),
        json!({"type":"test","scope":null,"subject":"add password tests","body":null}),
    ]);
    let mut staged = file(1);
    staged.staged = true;
    let config = AiCommitConfig {
        max_tokens: 64,
        ..enabled()
    };
    let result = generate_message(&model, &config, &[staged], "")
        .await
        .unwrap();
    assert_eq!(result.text, "test: add password tests");
    assert_eq!(rx.recv().unwrap()["max_tokens"], 64);
    assert_eq!(rx.recv().unwrap()["max_tokens"], 64);
}

#[test]
fn ai_commit_edge_editor_config_enables_provider_independently() {
    let config: helix_view::editor::Config =
        toml::from_str("[ai-commit]\nenabled = true\nlanguage = 'en'").unwrap();
    assert!(config.ai_commit.enabled);
    assert!(!config.ai_completion.enabled);
    assert!(!config.ai_doc.enabled);
    assert!(config.ai_model_config().enabled);
}
