//! Shared bounded OpenAI-compatible HTTP transport.
use anyhow::{bail, Context as _};
use helix_view::ai_completion::AiCompletionConfig;
use serde_json::Value;
use std::time::Duration;

pub(crate) async fn post(
    config: &AiCompletionConfig,
    body: &Value,
    timeout_ms: u64,
) -> anyhow::Result<Value> {
    let url = reqwest::Url::parse(&config.endpoint).context("Invalid AI endpoint")?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        bail!("AI endpoint must be an HTTP(S) URL without embedded credentials");
    }
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .context("Could not create AI client")?;
    let mut request = client
        .post(url)
        .json(&body)
        .timeout(Duration::from_millis(timeout_ms.clamp(100, 120_000)));
    if let Some(name) = config.api_key_env.as_deref().filter(|s| !s.is_empty()) {
        let token = std::env::var(name)
            .ok()
            .filter(|s| !s.trim().is_empty())
            .context("AI API key environment variable is unset or empty")?;
        request = request.bearer_auth(token);
    }
    let mut response = request.send().await.map_err(|err| {
        if err.is_timeout() {
            anyhow::anyhow!("AI request timed out")
        } else {
            anyhow::anyhow!("Could not connect to AI endpoint")
        }
    })?;
    if !response.status().is_success() {
        bail!("AI endpoint returned HTTP {}", response.status().as_u16());
    }
    const MAX_RESPONSE: usize = 262_144;
    if response
        .content_length()
        .is_some_and(|n| n > MAX_RESPONSE as u64)
    {
        bail!("AI response exceeded size limit");
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow::anyhow!("Could not read AI response"))?
    {
        if bytes.len() + chunk.len() > MAX_RESPONSE {
            bail!("AI response exceeded size limit");
        }
        bytes.extend_from_slice(&chunk);
    }
    let response: Value = serde_json::from_slice(&bytes).context("Invalid AI response JSON")?;
    Ok(response)
}
