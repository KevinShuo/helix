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
async fn ai_transport_generous_timeout_survives_two_minutes_but_remains_bounded() {
    for (mode, configured_ms, elapsed_ms, should_survive) in [
        (0, 180_000, 121_000, true),
        (0, 50, 150, false),
        (0, u64::MAX, 601_000, false),
        (1, 5_000, 601_000, true),
        (1, u64::MAX, 7_201_000, false),
        (2, 180_000, 121_000, true),
        (2, 5_000, 6_000, false),
        (2, u64::MAX, 7_201_000, false),
        (3, 5_000, 6_000, false),
        (3, u64::MAX, 601_000, false),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!(
            "http://{}/v1/chat/completions",
            listener.local_addr().unwrap()
        );
        let (received, arrival) = tokio::sync::oneshot::channel();
        let (reply, release) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut buffer = [0; 4096];
                let n = socket.read(&mut buffer).await.unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buffer[..n]);
                if let Some(offset) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..offset]).to_ascii_lowercase();
                    let length: usize = headers
                        .lines()
                        .find_map(|s| s.strip_prefix("content-length:"))
                        .unwrap()
                        .trim()
                        .parse()
                        .unwrap();
                    if bytes.len() >= offset + 4 + length {
                        break;
                    }
                }
            }
            received.send(()).unwrap();
            let _ = release.await;
            let _ = socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
                .await;
        });
        let operation = tokio::spawn(async move {
            let model = match mode {
                1 => automatic_deepseek(endpoint),
                2 => configured_deepseek(endpoint),
                3 => provider(endpoint, AiProviderType::LlamaCpp),
                _ => provider(endpoint, AiProviderType::OpenaiCompatible),
            };
            post(&model, &json!({}), configured_ms).await
        });
        arrival.await.unwrap();
        tokio::time::pause();
        tokio::time::advance(Duration::from_millis(elapsed_ms)).await;
        tokio::time::sleep(Duration::from_millis(1)).await;
        let expired = operation.is_finished();
        tokio::time::resume();
        let _ = reply.send(());
        let result = operation.await.unwrap();
        server.await.unwrap();
        assert_eq!(
            !expired, should_survive,
            "configured={configured_ms}, elapsed={elapsed_ms}, result={result:?}"
        );
        assert_eq!(result.is_ok(), should_survive);
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

fn automatic_deepseek(endpoint: String) -> AiProviderConfig {
    let mut config = provider(endpoint, AiProviderType::Deepseek);
    config.model = "deepseek-flash".into();
    config
}

fn configured_deepseek(endpoint: String) -> AiProviderConfig {
    let mut value = serde_json::to_value(automatic_deepseek(endpoint)).unwrap();
    value["budget-mode"] = json!("configured");
    serde_json::from_value(value).unwrap()
}

#[tokio::test]
async fn ai_transport_known_deepseek_accepts_large_complete_response() {
    let content = "complete source ".repeat(24_000);
    let response =
        json!({"choices":[{"message":{"content":content},"finish_reason":"stop"}]}).to_string();
    assert!(response.len() > 262_144);
    let (endpoint, capture) = server(200, response.clone()).await;
    let result = post(
        &automatic_deepseek(endpoint),
        &json!({"max_tokens":128}),
        5000,
    )
    .await
    .unwrap();
    assert_eq!(result["choices"][0]["message"]["content"], content);
    assert_eq!(capture.await.unwrap().1["max_tokens"], 393_216);

    let (endpoint, capture) = server(200, response.clone()).await;
    let result = post(
        &configured_deepseek(endpoint),
        &json!({"max_tokens":128}),
        5000,
    )
    .await
    .unwrap();
    assert_eq!(result["choices"][0]["message"]["content"], content);
    assert_eq!(capture.await.unwrap().1["max_tokens"], 128);

    for kind in [
        AiProviderType::LlamaCpp,
        AiProviderType::Deepseek,
        AiProviderType::OpenaiCompatible,
    ] {
        let (endpoint, capture) = server(200, response.clone()).await;
        let error = post(&provider(endpoint, kind), &json!({}), 5000)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("size limit"));
        capture.await.unwrap();
    }
}

#[tokio::test]
async fn ai_transport_enforces_declared_and_chunked_provider_memory_limits() {
    for (automatic, chunked) in [(true, false), (true, true), (false, true)] {
        let limit = if automatic { 16 * 1024 * 1024 } else { 262_144 };
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/chat/completions", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            socket.read(&mut request).await.unwrap();
            if chunked {
                let _ = socket.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").await;
                let data = vec![b'x'; 16_384];
                for _ in 0..=limit / data.len() {
                    if socket.write_all(b"4000\r\n").await.is_err()
                        || socket.write_all(&data).await.is_err()
                        || socket.write_all(b"\r\n").await.is_err()
                    {
                        break;
                    }
                }
                let _ = socket.write_all(b"0\r\n\r\n").await;
            } else {
                let headers = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    limit + 1
                );
                let _ = socket.write_all(headers.as_bytes()).await;
            }
        });
        let model = if automatic {
            automatic_deepseek(endpoint)
        } else {
            provider(endpoint, AiProviderType::LlamaCpp)
        };
        let error = post(&model, &json!({}), 5000).await.unwrap_err();
        assert!(error.to_string().contains("size limit"), "{error}");
        task.await.unwrap();
    }
}

async fn sequence_server(
    responses: Vec<(u16, String)>,
) -> (String, tokio::task::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/chat/completions", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let mut captures = Vec::new();
        for (status, response) in responses {
            let Ok(Ok((mut socket, _))) =
                tokio::time::timeout(Duration::from_secs(1), listener.accept()).await
            else {
                break;
            };
            let mut bytes = Vec::new();
            let (end, length) = loop {
                let mut buf = [0; 4096];
                let n = socket.read(&mut buf).await.unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buf[..n]);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
                    let length: usize = headers
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:"))
                        .unwrap()
                        .trim()
                        .parse()
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
            captures.push(serde_json::from_slice(&bytes[end..end + length]).unwrap());
            let wire = format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len());
            let _ = socket.write_all(wire.as_bytes()).await;
        }
        captures
    });
    (endpoint, task)
}

#[tokio::test]
async fn ai_transport_retries_context_reservation_using_authoritative_usage() {
    for message in [
        "Maximum CONTEXT length exceeded: SECRET_SOURCE",
        "Requested output exceeds context maximum: SECRET_SOURCE",
    ] {
        let (endpoint, capture) = sequence_server(vec![
            (400, json!({"error":{"message":message}}).to_string()),
            (200, json!({"choices":[{"message":{"content":"partial"},"finish_reason":"length"}],"usage":{"prompt_tokens":700_000}}).to_string()),
            (200, json!({"choices":[{"message":{"content":"complete"},"finish_reason":"stop"}]}).to_string()),
        ]).await;
        let input = json!({"model":"deepseek-flash","messages":[{"role":"system","content":"all instructions"},{"role":"user","content":"COMPLETE_SECRET_SOURCE"}],"max_tokens":393_216,"stream":false});
        let result = post(&automatic_deepseek(endpoint), &input, 5000)
            .await
            .unwrap();
        assert_eq!(result["choices"][0]["message"]["content"], "complete");
        let requests = capture.await.unwrap();
        assert_eq!(requests.len(), 3);
        for (request, maximum) in requests.iter().zip([393_216, 128, 348_320]) {
            assert_eq!(request["max_tokens"], maximum);
            assert_eq!(request["messages"], input["messages"]);
            assert_eq!(request["stream"], false);
        }
    }
}

#[tokio::test]
async fn ai_transport_does_not_retry_unrelated_failures_or_guess_prompt_usage() {
    for (status, response, expected) in [
        (
            400,
            json!({"error":{"message":"SECRET_SOURCE invalid parameter"}}).to_string(),
            "HTTP 400",
        ),
        (
            400,
            "SECRET_SOURCE context maximum invalid JSON".into(),
            "HTTP 400",
        ),
        (
            401,
            json!({"error":{"message":"context maximum SECRET_SOURCE"}}).to_string(),
            "HTTP 401",
        ),
        (429, "SECRET_SOURCE".into(), "HTTP 429"),
        (503, "SECRET_SOURCE".into(), "HTTP 503"),
        (
            200,
            "SECRET_SOURCE invalid JSON".into(),
            "Invalid AI response JSON",
        ),
    ] {
        let (endpoint, capture) = server(status, response).await;
        let error = post(
            &automatic_deepseek(endpoint),
            &json!({"max_tokens":128}),
            5000,
        )
        .await
        .unwrap_err();
        let error = format!("{error:#}");
        assert!(error.contains(expected), "{error}");
        assert!(!error.contains("SECRET"));
        capture.await.unwrap();
    }
    for usage in [
        json!(null),
        json!({"prompt_tokens":"700000"}),
        json!({"prompt_tokens":1_048_576}),
    ] {
        let response = json!({"choices":[{"finish_reason":"length","message":{"content":"partial"}}],"usage":usage});
        let (endpoint, capture) = server(200, response.to_string()).await;
        assert_eq!(
            post(
                &automatic_deepseek(endpoint),
                &json!({"max_tokens":128}),
                5000
            )
            .await
            .unwrap(),
            response
        );
        capture.await.unwrap();
    }
}

#[tokio::test]
async fn ai_transport_manual_and_unknown_models_do_not_retry_context_errors() {
    for configured in [false, true] {
        let (endpoint, capture) = server(
            400,
            json!({"error":{"message":"context maximum exceeded SECRET_SOURCE"}}).to_string(),
        )
        .await;
        let model = if configured {
            configured_deepseek(endpoint)
        } else {
            provider(endpoint, AiProviderType::Deepseek)
        };
        let error = post(&model, &json!({"max_tokens":128}), 5000)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("HTTP 400"));
        assert!(!format!("{error:#}").contains("SECRET"));
        assert_eq!(capture.await.unwrap().1["max_tokens"], 128);
    }
}

#[tokio::test]
async fn ai_transport_stops_after_three_attempts_and_one_context_retry() {
    let context_error =
        json!({"error":{"message":"context length exceeded SECRET_SOURCE"}}).to_string();
    let length = |prompt_tokens| {
        json!({"choices":[{"finish_reason":"length","message":{"content":"partial"}}],"usage":{"prompt_tokens":prompt_tokens}}).to_string()
    };
    let (endpoint, capture) = sequence_server(vec![
        (400, context_error.clone()),
        (200, length(700_000)),
        (200, length(100)),
    ])
    .await;
    let response = post(
        &automatic_deepseek(endpoint),
        &json!({"messages":[],"max_tokens":393_216}),
        5000,
    )
    .await
    .unwrap();
    assert_eq!(response["choices"][0]["finish_reason"], "length");
    assert_eq!(capture.await.unwrap().len(), 3);

    let (endpoint, capture) =
        sequence_server(vec![(400, context_error.clone()), (400, context_error)]).await;
    let error = post(&automatic_deepseek(endpoint), &json!({}), 5000)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("HTTP 400"));
    assert!(!format!("{error:#}").contains("SECRET"));
    assert_eq!(capture.await.unwrap().len(), 2);
}

#[tokio::test]
async fn ai_transport_marks_only_authoritative_auto_input_errors_for_batch_reduction() {
    let context = json!({"error":{"message":"context maximum exceeded SECRET_SOURCE"}}).to_string();
    let (endpoint, capture) = sequence_server(vec![(400, context.clone()), (400, context)]).await;
    let error = post(&automatic_deepseek(endpoint), &json!({}), 5000)
        .await
        .unwrap_err();
    assert!(can_reduce_batch(&error));
    assert!(!format!("{error:#}").contains("SECRET"));
    assert!(can_reduce_batch(&error.context("Batch request failed")));
    assert_eq!(capture.await.unwrap().len(), 2);

    for (status, message, reducible) in [
        (413, "SECRET_SOURCE", true),
        (
            400,
            r#"{"error":{"message":"invalid parameter SECRET_SOURCE"}}"#,
            false,
        ),
        (400, "context maximum SECRET_SOURCE invalid JSON", false),
        (401, "SECRET_SOURCE", false),
        (429, "SECRET_SOURCE", false),
        (503, "SECRET_SOURCE", false),
    ] {
        let (endpoint, capture) = server(status, message.into()).await;
        let error = post(&automatic_deepseek(endpoint), &json!({}), 5000)
            .await
            .unwrap_err();
        assert_eq!(can_reduce_batch(&error), reducible, "HTTP {status}");
        assert!(!format!("{error:#}").contains("SECRET"));
        capture.await.unwrap();
    }

    // Configured and unknown providers keep the caller's legacy status policy.
    for configured in [false, true] {
        let (endpoint, capture) = server(413, "SECRET_SOURCE".into()).await;
        let model = if configured {
            configured_deepseek(endpoint)
        } else {
            provider(endpoint, AiProviderType::Deepseek)
        };
        let error = post(&model, &json!({}), 5000).await.unwrap_err();
        assert!(!can_reduce_batch(&error));
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
