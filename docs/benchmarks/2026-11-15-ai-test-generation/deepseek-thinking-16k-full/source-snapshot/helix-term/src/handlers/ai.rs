//! Shared bounded OpenAI-compatible HTTP transport.
use anyhow::{bail, Context as _};
use helix_view::ai::{AiProviderConfig, AiProviderType, AiProxy};
use serde_json::{json, Value};
use std::{sync::OnceLock, time::Duration};

pub(super) fn client(proxy: AiProxy) -> anyhow::Result<&'static reqwest::Client> {
    static SYSTEM: OnceLock<Result<reqwest::Client, reqwest::Error>> = OnceLock::new();
    static DIRECT: OnceLock<Result<reqwest::Client, reqwest::Error>> = OnceLock::new();
    let cache = match proxy {
        AiProxy::System => &SYSTEM,
        AiProxy::None => &DIRECT,
    };
    cache
        .get_or_init(|| {
            let builder = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none());
            let builder = if proxy == AiProxy::None {
                builder.no_proxy()
            } else {
                builder
            };
            builder.build()
        })
        .as_ref()
        .map_err(|_| anyhow::anyhow!("Could not create AI client"))
}

pub(crate) async fn post(
    config: &AiProviderConfig,
    body: &Value,
    timeout_ms: u64,
) -> anyhow::Result<Value> {
    let url = reqwest::Url::parse(&config.endpoint)
        .map_err(|_| anyhow::anyhow!("Invalid AI endpoint"))?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        bail!("AI endpoint must be an HTTP(S) URL without embedded credentials");
    }
    let proxy = if config.model_path.is_some() {
        AiProxy::None
    } else {
        config.proxy
    };
    let client = client(proxy)?;
    let mut body = body.clone();
    let parameters = body
        .as_object_mut()
        .context("AI request must be a JSON object")?;
    parameters.remove("enable_thinking");
    parameters.remove("chat_template_kwargs");
    parameters.remove("thinking");
    match config.kind {
        AiProviderType::Deepseek => {
            parameters.insert(
                "thinking".into(),
                json!({"type": if config.thinking { "enabled" } else { "disabled" }}),
            );
        }
        AiProviderType::LlamaCpp => {
            parameters.insert("enable_thinking".into(), json!(config.thinking));
            parameters.insert(
                "chat_template_kwargs".into(),
                json!({"enable_thinking":config.thinking}),
            );
            if config.thinking {
                parameters.insert("reasoning_format".into(), json!("deepseek"));
                if let Some(effort) = &config.reasoning_effort {
                    parameters.insert("reasoning_effort".into(), json!(effort));
                }
                if let Some(budget) = config.thinking_budget {
                    parameters.insert("reasoning_budget_tokens".into(), json!(budget.min(8192)));
                }
            }
        }
        AiProviderType::OpenaiCompatible => {}
    }
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
        let status = response.status().as_u16();
        let reason = match status {
            401 | 403 => "authentication failed; check api-key-env and API permissions",
            402 => "insufficient balance; check the provider account",
            429 => "rate limit exceeded; retry later",
            500..=599 => "service unavailable; retry later",
            _ => "request rejected; check the provider, model and parameters",
        };
        bail!("AI endpoint returned HTTP {status}: {reason}");
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
    let response: Value =
        serde_json::from_slice(&bytes).map_err(|_| anyhow::anyhow!("Invalid AI response JSON"))?;
    Ok(response)
}

#[cfg(test)]
mod tests;
