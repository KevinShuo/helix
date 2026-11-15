use super::*;
use helix_view::ai::{AiProviderType, AiProxy};
use serde_json::json;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

pub(crate) async fn server(
    status: u16,
    response: String,
) -> (String, tokio::task::JoinHandle<(String, Value)>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!(
        "http://{}/custom/chat/completions",
        listener.local_addr().unwrap()
    );
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = Vec::new();
        let (end, length) = loop {
            let mut buf = [0; 4096];
            let n = socket.read(&mut buf).await.unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buf[..n]);
            if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&bytes[..end]);
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|s| s.trim().parse::<usize>().unwrap())
                    })
                    .unwrap();
                break (end + 4, length);
            }
        };
        while bytes.len() < end + length {
            let mut buf = [0; 4096];
            let n = socket.read(&mut buf).await.unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buf[..n]);
        }
        let headers = String::from_utf8(bytes[..end].to_vec()).unwrap();
        let body = serde_json::from_slice(&bytes[end..end + length]).unwrap();
        let wire = format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len());
        let _ = socket.write_all(wire.as_bytes()).await;
        (headers, body)
    });
    (endpoint, task)
}

fn provider(endpoint: String, kind: AiProviderType) -> AiProviderConfig {
    AiProviderConfig {
        endpoint,
        model: "test-model".into(),
        kind,
        proxy: AiProxy::None,
        ..Default::default()
    }
}

#[tokio::test]
async fn ai_transport_local_thinking_has_a_finite_budget_and_separate_reasoning() {
    let (endpoint, capture) = server(200, "{}".into()).await;
    let mut model = provider(endpoint, AiProviderType::LlamaCpp);
    model.thinking = true;
    model.thinking_budget = Some(2048);
    model.reasoning_effort = Some("medium".into());
    post(&model, &json!({"messages":[],"max_tokens":3072}), 5000)
        .await
        .unwrap();
    let (_, body) = capture.await.unwrap();
    assert_eq!(body["chat_template_kwargs"]["enable_thinking"], true);
    assert_eq!(body["reasoning_effort"], "medium");
    assert_eq!(body["reasoning_budget_tokens"], 2048);
    assert_eq!(body["reasoning_format"], "deepseek");
    assert_eq!(body["max_tokens"], 3072);
}

#[tokio::test]
async fn ai_transport_adapts_deepseek_generic_and_local_parameters() {
    for kind in [
        AiProviderType::Deepseek,
        AiProviderType::OpenaiCompatible,
        AiProviderType::LlamaCpp,
    ] {
        let (endpoint, capture) = server(200, "{}".into()).await;
        let model = provider(endpoint, kind);
        post(&model, &json!({"model":"test-model","messages":[],"enable_thinking":true,"chat_template_kwargs":{"enable_thinking":true}}), 5000).await.unwrap();
        let (headers, body) = capture.await.unwrap();
        assert!(headers.starts_with("POST /custom/chat/completions "));
        match kind {
            AiProviderType::Deepseek => {
                assert_eq!(body["thinking"]["type"], "disabled");
                assert!(body.get("enable_thinking").is_none());
                assert!(body.get("chat_template_kwargs").is_none());
            }
            AiProviderType::OpenaiCompatible => {
                assert!(body.get("thinking").is_none());
                assert!(body.get("enable_thinking").is_none());
                assert!(body.get("chat_template_kwargs").is_none());
            }
            AiProviderType::LlamaCpp => {
                assert_eq!(body["enable_thinking"], false);
                assert_eq!(body["chat_template_kwargs"]["enable_thinking"], false);
            }
        }
    }
}

#[tokio::test]
async fn ai_transport_reports_actionable_status_without_response_body() {
    for (status, expected) in [
        (401, "authentication"),
        (402, "balance"),
        (429, "rate limit"),
        (503, "unavailable"),
    ] {
        let (endpoint, capture) = server(status, "SECRET_RESPONSE_SOURCE".into()).await;
        let error = post(
            &provider(endpoint, AiProviderType::Deepseek),
            &json!({}),
            5000,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(error.contains(expected), "{error}");
        assert!(error.contains(&status.to_string()));
        assert!(!error.contains("SECRET"));
        capture.await.unwrap();
    }
}

#[tokio::test]
async fn ai_transport_missing_key_fails_before_contacting_provider() {
    let mut model = provider(
        "http://127.0.0.1:1/chat/completions".into(),
        AiProviderType::Deepseek,
    );
    model.api_key_env = Some("HELIX_TEST_AI_KEY_UNSET_9B3E049A".into());
    let error = post(&model, &json!({}), 5000)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("environment variable"));
}

#[tokio::test]
async fn ai_transport_preserves_auth_json_mode_and_context_without_logging_credentials() {
    let variable = "HELIX_TEST_AI_KEY_7BA72C70";
    std::env::set_var(variable, "synthetic-test-token");
    let (endpoint, capture) = server(200, "{}".into()).await;
    let mut model = provider(endpoint, AiProviderType::Deepseek);
    model.api_key_env = Some(variable.into());
    let input = json!({"messages":[{"role":"user","content":"json source context"}],"response_format":{"type":"json_object"},"max_tokens":128,"stream":false});
    post(&model, &input, 5000).await.unwrap();
    let (headers, body) = capture.await.unwrap();
    std::env::remove_var(variable);
    assert!(headers
        .to_ascii_lowercase()
        .contains("authorization: bearer synthetic-test-token"));
    assert_eq!(body["messages"], input["messages"]);
    assert_eq!(body["response_format"], input["response_format"]);
    assert_eq!(body["max_tokens"], 128);
    assert_eq!(body["stream"], false);
}

#[tokio::test]
async fn ai_transport_bounds_response_and_rejects_invalid_json() {
    for (response, expected) in [
        ("x".repeat(262145), "size limit"),
        ("invalid json".into(), "Invalid AI response JSON"),
    ] {
        let (endpoint, capture) = server(200, response).await;
        let error = post(
            &provider(endpoint, AiProviderType::Deepseek),
            &json!({}),
            5000,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(error.contains(expected), "{error}");
        capture.await.unwrap();
    }
}

#[tokio::test]
async fn ai_transport_system_proxy_is_used_when_configured() {
    // Proxy discovery is cached by the HTTP library. Use a child process to avoid
    // mutating proxy environment variables shared with parallel tests.
    if std::env::var_os("HELIX_TEST_AI_PROXY_CHILD_BA82").is_some() {
        let mut model = provider(
            "http://proxy-test.invalid/custom/chat/completions".into(),
            AiProviderType::Deepseek,
        );
        model.proxy = AiProxy::System;
        post(&model, &json!({}), 5000).await.unwrap();
        return;
    }
    let (endpoint, capture) = server(200, "{}".into()).await;
    let proxy = endpoint.trim_end_matches("/custom/chat/completions");
    let mut child = tokio::process::Command::new(std::env::current_exe().unwrap());
    child
        .args([
            "--exact",
            "handlers::ai::tests::ai_transport_system_proxy_is_used_when_configured",
        ])
        .env("HELIX_TEST_AI_PROXY_CHILD_BA82", "1")
        .env("HTTP_PROXY", proxy)
        .env("http_proxy", proxy)
        .env("ALL_PROXY", proxy)
        .env("all_proxy", proxy)
        .env("NO_PROXY", "")
        .env("no_proxy", "")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    child.creation_flags(0x0800_0000);
    let status = tokio::time::timeout(Duration::from_secs(10), child.status())
        .await
        .unwrap()
        .unwrap();
    assert!(
        status.success(),
        "proxy request must succeed in isolated process"
    );
    let (headers, _) = capture.await.unwrap();
    assert!(headers.starts_with("POST http://proxy-test.invalid/custom/chat/completions "));
}
