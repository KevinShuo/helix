use helix_view::ai::{AiProviderConfig, AiProviderType};
use serde_json::Value;
use std::{sync::OnceLock, time::Duration};
use tokio::sync::watch;

pub(super) type TokenSample = (String, Option<u64>);

pub(super) async fn track_tokens(
    model: &AiProviderConfig,
    stages: watch::Receiver<String>,
    samples: watch::Sender<TokenSample>,
) {
    let mut ticks = tokio::time::interval(Duration::from_secs(1));
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticks.tick().await;
        let stage = stages.borrow().clone();
        let generating = [
            "AI 分析批次",
            "AI 重试分析批次",
            "AI 汇总推荐",
            "AI 生成提交信息",
        ]
        .iter()
        .any(|prefix| stage.starts_with(prefix));
        let count = if generating {
            generated_tokens(model).await
        } else {
            None
        };
        // Discard a slow probe if the operation moved to a different batch.
        if *stages.borrow() != stage {
            continue;
        }
        let next = (stage, count);
        samples.send_if_modified(|sample| {
            if *sample == next {
                return false;
            }
            *sample = next;
            true
        });
    }
}

/// Read-only token telemetry for the application's single-slot local server.
pub(super) async fn generated_tokens(model: &AiProviderConfig) -> Option<u64> {
    if model.kind != AiProviderType::LlamaCpp || model.model_path.is_none() {
        return None;
    }
    let mut url = reqwest::Url::parse(&model.endpoint).ok()?;
    let local = url.host_str().is_some_and(|host| {
        host == "localhost"
            || host
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    if !local || url.scheme() != "http" || !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    url.set_path("/slots");
    url.set_query(None);
    url.set_fragment(None);
    static CLIENT: OnceLock<Option<reqwest::Client>> = OnceLock::new();
    let client = CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .ok()
        })
        .as_ref()?;
    // Telemetry failure must never fail or hold up the generation request.
    let mut response = client
        .get(url)
        .timeout(Duration::from_millis(500))
        .send()
        .await
        .ok()?;
    const LIMIT: usize = 64 * 1024;
    if !response.status().is_success()
        || response.content_length().is_some_and(|n| n > LIMIT as u64)
    {
        return None;
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if chunk.len() > LIMIT.saturating_sub(bytes.len()) {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    let slots: Value = serde_json::from_slice(&bytes).ok()?;
    let slots = slots.as_array()?;
    // Managed servers run --parallel 1. Never add other clients' slot counts.
    if slots.len() != 1
        || slots[0]["id"].as_u64() != Some(0)
        || slots[0]["is_processing"].as_bool() != Some(true)
    {
        return None;
    }
    let next = &slots[0]["next_token"];
    // Installed b11081 uses a one-element array; newer servers use an object.
    let next = if let Some(items) = next.as_array() {
        if items.len() != 1 {
            return None;
        }
        &items[0]
    } else {
        next
    };
    next["n_decoded"].as_u64()
}

#[cfg(test)]
mod tests {
    use super::*;
    use helix_view::ai::AiProviderType;
    use serde_json::json;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    #[tokio::test]
    async fn ai_commit_token_progress_reads_real_counts_and_rejects_idle_or_ambiguous_slots() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let model = AiProviderConfig {
            kind: AiProviderType::LlamaCpp,
            endpoint: format!(
                "http://{}/v1/chat/completions",
                listener.local_addr().unwrap()
            ),
            model_path: Some("fixture.gguf".into()),
            ..Default::default()
        };
        let server = tokio::spawn(async move {
            for body in [
                json!([{"id":0,"is_processing":true,"next_token":[{"n_decoded":12}]}]),
                json!([{"id":0,"is_processing":true,"next_token":{"n_decoded":37}}]),
                json!([{"id":0,"is_processing":false,"next_token":{"n_decoded":37}}]),
                json!([{"id":0,"is_processing":true,"next_token":{"n_decoded":99}}, {"id":1,"is_processing":true,"next_token":{"n_decoded":18}}]),
            ] {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = vec![0; 4096];
                let n = socket.read(&mut bytes).await.unwrap();
                let request = String::from_utf8_lossy(&bytes[..n]);
                assert!(request.starts_with("GET /slots HTTP/1.1\r\n"), "{request}");
                assert!(!request.to_ascii_lowercase().contains("authorization:"));
                let body = body.to_string();
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            }
        });
        assert_eq!(generated_tokens(&model).await, Some(12));
        assert_eq!(generated_tokens(&model).await, Some(37));
        assert_eq!(generated_tokens(&model).await, None);
        assert_eq!(generated_tokens(&model).await, None);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn ai_commit_token_progress_does_not_probe_unmanaged_or_online_providers() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = AiProviderConfig {
            endpoint: format!(
                "http://{}/v1/chat/completions",
                listener.local_addr().unwrap()
            ),
            ..Default::default()
        };
        for model in [
            base.clone(),
            AiProviderConfig {
                kind: AiProviderType::Deepseek,
                model_path: Some("fixture.gguf".into()),
                ..base.clone()
            },
            AiProviderConfig {
                endpoint: "https://example.com/v1/chat/completions".into(),
                model_path: Some("fixture.gguf".into()),
                ..base.clone()
            },
        ] {
            assert_eq!(generated_tokens(&model).await, None);
        }
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), listener.accept())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn ai_commit_token_progress_drops_stale_inflight_probe_when_batch_changes() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let model = AiProviderConfig {
            kind: AiProviderType::LlamaCpp,
            endpoint: format!(
                "http://{}/v1/chat/completions",
                listener.local_addr().unwrap()
            ),
            model_path: Some("fixture.gguf".into()),
            ..Default::default()
        };
        let (stages, stage_updates) = watch::channel("AI 汇总推荐".to_owned());
        let (samples, mut updates) = watch::channel((String::new(), None));
        let monitor =
            tokio::spawn(async move { track_tokens(&model, stage_updates, samples).await });
        let (mut socket, _) = tokio::time::timeout(Duration::from_secs(2), listener.accept())
            .await
            .unwrap()
            .unwrap();
        let mut bytes = [0; 4096];
        socket.read(&mut bytes).await.unwrap();
        stages.send_replace("AI 校验仓库状态并整理结果".into());
        let body = r#"[{"id":0,"is_processing":true,"next_token":{"n_decoded":999}}]"#;
        socket
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), updates.changed())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updates.borrow().0, "AI 校验仓库状态并整理结果");
        assert_eq!(
            updates.borrow().1,
            None,
            "must not attach previous request's count to the new phase"
        );
        monitor.abort();
        assert!(monitor.await.unwrap_err().is_cancelled());
        assert!(
            tokio::time::timeout(Duration::from_millis(30), listener.accept())
                .await
                .is_err(),
            "canceled monitor must not send more telemetry"
        );
    }
}
