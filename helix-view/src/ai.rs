//! Provider configuration shared by automatic and explicit AI features.
use std::{collections::BTreeMap, path::PathBuf};

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};

use crate::{ai_completion::AiCompletionConfig, editor::Config};

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct AiConfig {
    pub default_provider: Option<String>,
    pub providers: BTreeMap<String, AiProviderConfig>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AiProviderType {
    LlamaCpp,
    Deepseek,
    #[default]
    OpenaiCompatible,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AiProxy {
    #[default]
    System,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct AiProviderConfig {
    #[serde(rename = "type")]
    pub kind: AiProviderType,
    pub endpoint: String,
    pub model: String,
    pub api_key_env: Option<String>,
    pub context_size: u32,
    pub thinking: bool,
    pub thinking_budget: Option<u32>,
    pub reasoning_effort: Option<String>,
    /// Runtime marker: local explicit-generation overrides acquire a temporary lease.
    #[serde(skip)]
    pub on_demand: bool,
    pub proxy: AiProxy,
    pub model_path: Option<PathBuf>,
    pub server_path: PathBuf,
    pub gpu_layers: u32,
    pub startup_timeout_ms: u64,
}

impl Default for AiProviderConfig {
    fn default() -> Self {
        Self {
            kind: AiProviderType::OpenaiCompatible,
            endpoint: String::new(),
            model: String::new(),
            api_key_env: None,
            context_size: 4096,
            thinking: false,
            thinking_budget: None,
            reasoning_effort: None,
            on_demand: false,
            proxy: AiProxy::System,
            model_path: None,
            server_path: "llama-server".into(),
            gpu_layers: 99,
            startup_timeout_ms: 120_000,
        }
    }
}

impl AiProviderConfig {
    pub fn from_legacy(config: &AiCompletionConfig) -> Self {
        Self {
            kind: AiProviderType::LlamaCpp,
            endpoint: config.endpoint.clone(),
            model: config.model.clone(),
            api_key_env: config.api_key_env.clone(),
            context_size: config.context_size,
            model_path: config.model_path.clone(),
            server_path: config.server_path.clone(),
            gpu_layers: config.gpu_layers,
            startup_timeout_ms: config.startup_timeout_ms,
            proxy: AiProxy::None,
            ..Self::default()
        }
    }

    fn validate(&self) -> Result<()> {
        ensure!(
            !self.endpoint.trim().is_empty(),
            "AI provider endpoint is empty"
        );
        ensure!(!self.model.trim().is_empty(), "AI provider model is empty");
        ensure!(
            self.context_size >= 512,
            "AI provider context-size must be at least 512"
        );
        ensure!(
            self.model_path.is_none() || self.kind == AiProviderType::LlamaCpp,
            "AI model-path is only supported for type = 'llama-cpp'"
        );
        if let Some(budget) = self.thinking_budget {
            ensure!(
                self.kind == AiProviderType::LlamaCpp && (1..=8192).contains(&budget),
                "thinking-budget requires llama-cpp and must be between 1 and 8192"
            );
        }
        if let Some(effort) = &self.reasoning_effort {
            ensure!(
                ["low", "medium", "high", "xhigh", "max"].contains(&effort.as_str()),
                "Unsupported reasoning-effort"
            );
        }
        Ok(())
    }

    pub fn reasoning_tokens(&self) -> u32 {
        if self.kind == AiProviderType::LlamaCpp && self.thinking {
            self.thinking_budget.unwrap_or(0).min(8192)
        } else {
            0
        }
    }

    fn listener_identity(&self) -> Option<(String, u16)> {
        let url = url::Url::parse(&self.endpoint).ok()?;
        Some((url.host_str()?.to_owned(), url.port_or_known_default()?))
    }
}

#[derive(Debug, Clone, Copy)]
pub enum AiFeature {
    Completion,
    Doc,
    Commit,
    Test,
}

impl Config {
    pub fn ai_provider(&self, feature: AiFeature) -> Result<AiProviderConfig> {
        let selected = match feature {
            AiFeature::Completion => self.ai_completion.provider.as_ref(),
            AiFeature::Doc => self.ai_doc.provider.as_ref(),
            AiFeature::Commit => self.ai_commit.provider.as_ref(),
            AiFeature::Test => self.ai_test.provider.as_ref(),
        }
        .or(self.ai.default_provider.as_ref());
        let mut provider = if let Some(name) = selected {
            let provider = self
                .ai
                .providers
                .get(name)
                .with_context(|| format!("Unknown AI provider '{name}'"))?;
            provider
                .validate()
                .map_err(|err| anyhow::anyhow!("Invalid AI provider '{name}': {err}"))?;
            provider.clone()
        } else {
            AiProviderConfig::from_legacy(&self.ai_completion)
        };
        let explicit = match feature {
            AiFeature::Completion => self.ai_completion.provider.is_some(),
            AiFeature::Doc => self.ai_doc.provider.is_some(),
            AiFeature::Commit => self.ai_commit.provider.is_some(),
            AiFeature::Test => self.ai_test.provider.is_some(),
        };
        if !explicit && provider.kind == AiProviderType::LlamaCpp {
            if let Some(local) = self.local_generation_provider(feature)? {
                if local != &provider {
                    provider = local.clone();
                    provider.on_demand = true;
                    if provider.thinking {
                        provider.thinking_budget.get_or_insert(2048);
                        provider
                            .reasoning_effort
                            .get_or_insert_with(|| "medium".into());
                    }
                }
            }
        }
        Ok(provider)
    }

    fn local_generation_provider(&self, feature: AiFeature) -> Result<Option<&AiProviderConfig>> {
        let name = match feature {
            AiFeature::Completion => None,
            AiFeature::Doc => self.ai_doc.local_provider.as_ref(),
            AiFeature::Commit => self.ai_commit.local_provider.as_ref(),
            AiFeature::Test => self.ai_test.local_provider.as_ref(),
        };
        let Some(name) = name else {
            return Ok(None);
        };
        let provider = self
            .ai
            .providers
            .get(name)
            .with_context(|| format!("Unknown local AI generation provider '{name}'"))?;
        provider.validate()?;
        ensure!(
            provider.kind == AiProviderType::LlamaCpp && provider.model_path.is_some(),
            "AI local-provider must configure a managed llama-cpp model-path"
        );
        Ok(Some(provider))
    }

    pub fn active_ai_providers(&self) -> Result<Vec<AiProviderConfig>> {
        let mut providers: Vec<AiProviderConfig> = Vec::new();
        for (feature, enabled) in [
            (AiFeature::Completion, self.ai_completion.enabled),
            (AiFeature::Doc, self.ai_doc.enabled),
            (AiFeature::Commit, self.ai_commit.enabled),
            (AiFeature::Test, self.ai_test.enabled),
        ] {
            if !enabled {
                continue;
            }
            let provider = self.ai_provider(feature)?;
            if providers.contains(&provider) {
                continue;
            }
            if providers.iter().any(|other| {
                (provider.model_path.is_some() || other.model_path.is_some())
                    && provider.listener_identity().is_some()
                    && provider.listener_identity() == other.listener_identity()
            }) {
                bail!("Active local AI providers must use distinct endpoints");
            }
            providers.push(provider);
        }
        Ok(providers)
    }

    pub fn validate_ai(&self) -> Result<()> {
        self.local_generation_provider(AiFeature::Doc)?;
        self.local_generation_provider(AiFeature::Commit)?;
        self.local_generation_provider(AiFeature::Test)?;
        for (name, provider) in &self.ai.providers {
            provider
                .validate()
                .map_err(|err| anyhow::anyhow!("Invalid AI provider '{name}': {err}"))?;
        }
        if let Some(name) = &self.ai.default_provider {
            ensure!(
                self.ai.providers.contains_key(name),
                "Unknown default AI provider '{name}'"
            );
        }
        for feature in [
            AiFeature::Completion,
            AiFeature::Doc,
            AiFeature::Commit,
            AiFeature::Test,
        ] {
            self.ai_provider(feature)?;
        }
        self.active_ai_providers()?;
        Ok(())
    }
}
