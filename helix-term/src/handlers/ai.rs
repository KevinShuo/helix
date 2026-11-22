//! Shared bounded OpenAI-compatible HTTP transport.
use anyhow::{bail, Context as _};
use helix_view::ai::{AiProviderConfig, AiProviderType, AiProxy};
use serde_json::{json, Value};
use std::{sync::OnceLock, time::Duration};

#[derive(Debug)]
struct InputLimitError {
    status: u16,
}

impl std::fmt::Display for InputLimitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "AI endpoint returned HTTP {}: request exceeds provider input limits",
            self.status
        )
    }
}

impl std::error::Error for InputLimitError {}

/// Only an authoritative automatic-provider input error permits smaller batches.
pub(crate) fn can_reduce_batch(error: &anyhow::Error) -> bool {
    error.is::<InputLimitError>()
}

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
            if config.thinking {
                if let Some(effort) = &config.reasoning_effort {
                    parameters.insert("reasoning_effort".into(), json!(effort));
                }
            }
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
    let automatic = config.automatic_budget();
    let output_ceiling = config.budget_output_tokens(u32::MAX);
    if automatic {
        parameters.insert("max_tokens".into(), json!(output_ceiling));
    }
    let token = if let Some(name) = config.api_key_env.as_deref().filter(|s| !s.is_empty()) {
        Some(
            std::env::var(name)
                .ok()
                .filter(|s| !s.trim().is_empty())
                .context("AI API key environment variable is unset or empty")?,
        )
    } else {
        None
    };
    // Retries share one finite deadline, including response-body reads.
    let deadline =
        tokio::time::Instant::now() + Duration::from_millis(config.budget_timeout_ms(timeout_ms));
    let mut attempts = 0;
    let mut retried_context = false;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            bail!("AI request timed out");
        }
        let mut request = client.post(url.clone()).json(&body).timeout(remaining);
        if let Some(token) = &token {
            request = request.bearer_auth(token);
        }
        attempts += 1;
        let response = request.send().await.map_err(|err| {
            if err.is_timeout() {
                anyhow::anyhow!("AI request timed out")
            } else {
                anyhow::anyhow!("Could not connect to AI endpoint")
            }
        })?;
        let status = response.status().as_u16();
        if !response.status().is_success() {
            if automatic && status == 400 {
                let bytes = response_bytes(response, config.budget_response_bytes()).await?;
                if context_reservation_error(&bytes) {
                    if !retried_context && attempts < 3 {
                        retried_context = true;
                        body["max_tokens"] = json!(128);
                        continue;
                    }
                    return Err(InputLimitError { status }.into());
                }
            }
            if automatic && status == 413 {
                return Err(InputLimitError { status }.into());
            }
            let reason = match status {
                401 | 403 => "authentication failed; check api-key-env and API permissions",
                402 => "insufficient balance; check the provider account",
                429 => "rate limit exceeded; retry later",
                500..=599 => "service unavailable; retry later",
                _ => "request rejected; check the provider, model and parameters",
            };
            bail!("AI endpoint returned HTTP {status}: {reason}");
        }
        let bytes = response_bytes(response, config.budget_response_bytes()).await?;
        let response: Value = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow::anyhow!("Invalid AI response JSON"))?;
        if automatic
            && attempts < 3
            && response["choices"][0]["finish_reason"].as_str() == Some("length")
        {
            if let Some(prompt_tokens) = response["usage"]["prompt_tokens"].as_u64() {
                let next = (config.budget_context() as u64)
                    .saturating_sub(prompt_tokens)
                    .saturating_sub(256)
                    .min(output_ceiling as u64);
                if next > body["max_tokens"].as_u64().unwrap_or(0) {
                    body["max_tokens"] = json!(next);
                    continue;
                }
            }
        }
        return Ok(response);
    }
}

pub(crate) fn interactive_busy(editor: &helix_view::Editor) -> bool {
    super::ai_chat::busy(editor) || super::ai_generate::active(editor)
}

fn interactive_output(model: &AiProviderConfig) -> u32 {
    model.budget_output_tokens(
        (model.budget_context() as u32 / 4)
            .min(2048)
            .saturating_add(model.reasoning_tokens()),
    )
}

pub(super) const MESSAGE_OVERHEAD: usize = 16;

pub(super) fn message_bytes(messages: &[Value]) -> usize {
    messages
        .iter()
        .map(|m| m["content"].as_str().map_or(0, str::len) + MESSAGE_OVERHEAD)
        .sum()
}

pub(super) fn interactive_input_fits(model: &AiProviderConfig, input: usize) -> bool {
    model.budget_fits(input, interactive_output(model), 128)
}

pub(super) fn interactive_fits(model: &AiProviderConfig, messages: &[Value]) -> bool {
    interactive_input_fits(model, message_bytes(messages))
}

/// Execute an explicit user request through the same model lease and HTTP transport
/// as documentation and tests. Preserve code indentation for the insertion preview.
pub(crate) async fn interactive(
    model: &AiProviderConfig,
    messages: Vec<Value>,
) -> anyhow::Result<String> {
    anyhow::ensure!(
        interactive_fits(model, &messages),
        "内容超出模型上下文，请缩小选区、缩短需求或选择更大上下文的模型"
    );
    let body = json!({"model": model.model, "messages": messages, "max_tokens": interactive_output(model),
        "stream": false, "temperature": 0.2});
    super::ai_completion::server::with_model(model, async {
        super::ai_completion::server::wait_ready(model).await?;
        let response = post(model, &body, 120_000).await?;
        let choice = &response["choices"][0];
        anyhow::ensure!(
            matches!(choice["finish_reason"].as_str(), None | Some("stop")),
            "模型未返回完整结果，请缩小请求后重试"
        );
        let message = &choice["message"];
        anyhow::ensure!(
            message["refusal"].is_null()
                && message["function_call"].is_null()
                && message["tool_calls"].as_array().is_none_or(Vec::is_empty),
            "模型未返回可显示的文本"
        );
        let content = message["content"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .context("模型返回了空内容")?;
        anyhow::ensure!(content.len() <= 128 * 1024, "回答过长，请缩小问题范围");
        Ok(content.to_owned())
    })
    .await
}

// Inspect only structured provider errors, never include their contents in an error.
fn context_reservation_error(bytes: &[u8]) -> bool {
    let Ok(error) = serde_json::from_slice::<Value>(bytes) else {
        return false;
    };
    let Some(message) = error["error"]["message"].as_str() else {
        return false;
    };
    let message = message.to_ascii_lowercase();
    message.contains("context")
        && ["exceed", "maximum", "length"]
            .iter()
            .any(|word| message.contains(word))
}

async fn response_bytes(mut response: reqwest::Response, limit: usize) -> anyhow::Result<Vec<u8>> {
    if response.content_length().is_some_and(|n| n > limit as u64) {
        bail!("AI response exceeded size limit");
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|err| {
        if err.is_timeout() {
            anyhow::anyhow!("AI request timed out")
        } else {
            anyhow::anyhow!("Could not read AI response")
        }
    })? {
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            bail!("AI response exceeded size limit");
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[cfg(test)]
pub(crate) mod tests;
