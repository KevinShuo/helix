//! Provider configuration shared by automatic and explicit AI features.
mod budget;
mod local_models;
mod runtime;
pub use runtime::{AiCacheType, AiModelLoadMode, AiRuntimeConfig, AiSpeculativeDecoding};

use std::{collections::BTreeMap, path::PathBuf};

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};

use crate::{ai_completion::AiCompletionConfig, editor::Config};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct AiConfig {
    pub enabled: bool,
    pub min_local_vram_gib: u32,
    pub thinking: BTreeMap<AiFeature, AiThinking>,
    /// Feature defaults for the DeepSeek route. Session overrides take precedence.
    pub deepseek_thinking: BTreeMap<AiFeature, AiThinking>,
    pub default_provider: Option<String>,
    /// Preferred managed model for inherited features when this host can run it.
    pub auto_local_provider: Option<String>,
    /// Desktop default for manual generation. Inline completion is independent.
    pub local_generation_provider: Option<String>,
    pub local_model_directories: Vec<PathBuf>,
    #[serde(serialize_with = "configured_providers")]
    pub providers: BTreeMap<String, AiProviderConfig>,
    #[serde(skip)]
    pub(crate) catalog_loaded: bool,
    #[serde(skip)]
    automatic_generation: Option<String>,
    /// Computed when applying configuration, never read from a workspace file.
    #[serde(skip)]
    pub(crate) deepseek_only: bool,
    #[serde(skip)]
    laptop: bool,
    #[serde(skip)]
    dedicated_vram: u64,
    #[serde(skip)]
    statusline_model: String,
}

// :set-option round-trips the editor configuration through serde. Runtime
// profiles must be rediscovered so their lease and hardware metadata survive.
fn configured_providers<S: serde::Serializer>(
    providers: &BTreeMap<String, AiProviderConfig>,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    serializer.collect_map(
        providers
            .iter()
            .filter(|(name, p)| !(name.starts_with("gguf-") && p.on_demand)),
    )
}

impl AiConfig {
    pub fn statusline(&self) -> &str {
        if !self.enabled {
            "AI 关"
        } else if self.statusline_model.is_empty() {
            "AI 开 · 无启用功能"
        } else {
            &self.statusline_model
        }
    }
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_local_vram_gib: 8,
            thinking: BTreeMap::new(),
            deepseek_thinking: BTreeMap::new(),
            default_provider: None,
            auto_local_provider: None,
            local_generation_provider: None,
            local_model_directories: Vec::new(),
            providers: BTreeMap::new(),
            catalog_loaded: false,
            automatic_generation: None,
            deepseek_only: false,
            laptop: false,
            dedicated_vram: 0,
            statusline_model: String::new(),
        }
    }
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

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AiBudgetMode {
    #[default]
    Auto,
    Configured,
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
    pub budget_mode: AiBudgetMode,
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
    pub runtime: AiRuntimeConfig,
}

impl Default for AiProviderConfig {
    fn default() -> Self {
        Self {
            kind: AiProviderType::OpenaiCompatible,
            endpoint: String::new(),
            model: String::new(),
            api_key_env: None,
            context_size: 4096,
            budget_mode: AiBudgetMode::Auto,
            thinking: false,
            thinking_budget: None,
            reasoning_effort: None,
            on_demand: false,
            proxy: AiProxy::System,
            model_path: None,
            server_path: "llama-server".into(),
            gpu_layers: 99,
            startup_timeout_ms: 120_000,
            runtime: AiRuntimeConfig::default(),
        }
    }
}

impl AiProviderConfig {
    pub fn default_deepseek() -> Self {
        Self {
            kind: AiProviderType::Deepseek,
            endpoint: "https://api.deepseek.com/chat/completions".into(),
            model: "deepseek-flash".into(),
            api_key_env: Some("DEEPSEEK_API_KEY".into()),
            context_size: 32768,
            ..Self::default()
        }
    }

    pub fn is_local_completion_model(&self) -> bool {
        self.kind == AiProviderType::LlamaCpp
            && self.model_path.is_some()
            && url::Url::parse(&self.endpoint).is_ok_and(|url| {
                url.scheme() == "http"
                    && url.host_str() == Some("127.0.0.1")
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.path() == "/v1/chat/completions"
                    && url.query().is_none()
                    && url.fragment().is_none()
            })
    }

    pub fn fits_dedicated_vram(&self, bytes: u64) -> bool {
        self.fitting_weight_size(bytes).is_some()
    }

    /// A declared parameter count in a GGUF filename or alias can identify a
    /// small completion model even when it is in a different model directory.
    fn compact_generation_model(&self) -> bool {
        fn compact_label(label: &str) -> Option<bool> {
            static COUNT: std::sync::LazyLock<helix_core::regex::Regex> =
                std::sync::LazyLock::new(|| {
                    helix_core::regex::Regex::new(
                        r"(?i)(?:^|[^a-z0-9])([0-9]+(?:\.[0-9]+)?)([bm])(?:$|[^a-z0-9])",
                    )
                    .unwrap()
                });
            let count = COUNT.captures(label)?;
            let limit = if count[2].eq_ignore_ascii_case("b") {
                3.0
            } else {
                3000.0
            };
            Some(count[1].parse::<f32>().ok()? <= limit)
        }
        compact_label(&self.model)
            .or_else(|| compact_label(self.model_path.as_ref()?.file_stem()?.to_str()?))
            .unwrap_or(false)
    }

    fn fitting_weight_size(&self, bytes: u64) -> Option<u64> {
        if !self.is_local_completion_model() {
            return None;
        }
        let size =
            local_models::weight_bytes(&helix_stdx::path::expand(self.model_path.as_ref()?))?;
        (size.saturating_add(1 << 30) <= bytes
            && helix_stdx::env::binary_exists(
                helix_stdx::path::expand(&self.server_path).as_os_str(),
            ))
        .then_some(size)
    }

    fn is_local(&self) -> bool {
        let url = url::Url::parse(&self.endpoint).ok();
        let loopback = match url.as_ref().and_then(url::Url::host) {
            Some(url::Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            None => false,
        };
        self.model_path.is_some() || loopback
    }

    pub fn route_label(&self) -> &'static str {
        if self.kind == AiProviderType::Deepseek {
            "DeepSeek"
        } else if self.is_local() {
            "本地"
        } else if self.kind == AiProviderType::LlamaCpp {
            "远程 llama.cpp"
        } else {
            "OpenAI 兼容"
        }
    }

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

    /// Validate provider and runtime settings before starting a managed model.
    pub fn validate(&self) -> Result<()> {
        self.runtime.validate()?;
        ensure!(
            self.kind == AiProviderType::LlamaCpp || self.runtime == AiRuntimeConfig::default(),
            "AI runtime settings require type = 'llama-cpp'"
        );
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiThinking {
    Off,
    Low,
    High,
    Max,
}

impl AiThinking {
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "关闭",
            Self::Low => "低",
            Self::High => "高",
            Self::Max => "最高",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AiFeature {
    Completion,
    Doc,
    Commit,
    Test,
    Merge,
    Explain,
    Generate,
}

impl AiFeature {
    fn is_interactive(self) -> bool {
        matches!(self, Self::Explain | Self::Generate)
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct InteractiveConfig {
    pub enabled: bool,
    pub provider: Option<String>,
    pub local_provider: Option<String>,
}

impl Config {
    pub fn load_local_model_catalog(&mut self) {
        if self.ai.enabled && !self.ai.catalog_loaded {
            self.discover_local_models();
            self.ai.catalog_loaded = true;
        }
    }

    pub fn is_generation_choice(&self, name: &str) -> bool {
        self.ai.providers.get(name).is_some_and(|p| {
            p.is_local_completion_model()
                && !(self.ai_completion.local_only
                    && self.ai_completion.provider.as_deref() == Some(name)
                    && p.gpu_layers == 0)
        })
    }

    fn automatic_local_models(&self) -> bool {
        self.ai.auto_local_provider.is_some() || !self.ai.local_model_directories.is_empty()
    }

    pub fn ai_features(&self) -> [(AiFeature, bool); 7] {
        [
            (AiFeature::Completion, self.ai_completion.enabled),
            (AiFeature::Doc, self.ai_doc.enabled),
            (AiFeature::Commit, self.ai_commit.enabled),
            (AiFeature::Test, self.ai_test.enabled),
            (AiFeature::Merge, self.ai_merge.enabled),
            (AiFeature::Explain, self.ai_explain.enabled),
            (AiFeature::Generate, self.ai_generate.enabled),
        ]
    }

    /// Compare one physical GPU's dedicated memory with the largest selected
    /// local model. On-demand models replace the resident model, so use a maximum.
    pub fn apply_ai_hardware(&mut self, dedicated_vram: impl FnOnce() -> u64) -> Result<()> {
        self.apply_ai_host(|| (dedicated_vram(), false))
    }

    pub fn apply_ai_host(&mut self, hardware: impl FnOnce() -> (u64, bool)) -> Result<()> {
        self.ai.deepseek_only = false;
        self.ai.laptop = false;
        self.ai.automatic_generation = None;
        self.ai_completion.unavailable = None;
        self.ai.statusline_model.clear();
        if !self.ai.enabled || !self.ai_features().iter().any(|(_, enabled)| *enabled) {
            return Ok(());
        }
        let (capacity, laptop) = hardware();
        self.ai.laptop = laptop;
        self.ai.dedicated_vram = capacity;
        if !laptop && self.ai.local_generation_provider.is_none() && self.automatic_local_models() {
            // A per-feature profile takes precedence. For the automatic alternative,
            // consider the available library, leaving dedicated inline weights alone.
            let inline_weights = [
                self.ai_completion.provider.as_ref(),
                self.ai.auto_local_provider.as_ref(),
            ]
            .map(|name| {
                name.and_then(|n| self.ai.providers.get(n))
                    .and_then(|p| p.model_path.as_ref())
            });
            self.ai.automatic_generation = self
                .ai
                .providers
                .iter()
                .filter(|(_, p)| {
                    !inline_weights.contains(&p.model_path.as_ref())
                        && !p.compact_generation_model()
                })
                .filter_map(|(name, p)| p.fitting_weight_size(capacity).map(|size| (size, name)))
                .max()
                .map(|(_, name)| name.clone());
        }
        let mut required = u64::from(self.ai.min_local_vram_gib) << 30;
        for (feature, enabled) in self.ai_features() {
            if !enabled || (feature == AiFeature::Completion && self.ai_completion.local_only) {
                continue;
            }
            if feature != AiFeature::Completion
                && (laptop
                    || self.automatic_local_models()
                    || (self.ai.local_generation_provider.is_some()
                        && self.feature_provider_name(feature).is_none()))
            {
                continue;
            }
            if let Some(path) = self.configured_ai_provider(feature)?.model_path {
                if let Some(size) = local_models::weight_bytes(&helix_stdx::path::expand(&path)) {
                    required = required.max(size.saturating_add(1 << 30));
                }
            }
        }
        self.ai.deepseek_only = capacity == 0 || capacity < required;
        if self.ai_completion.enabled
            && self.ai_completion.provider.is_none()
            && self.ai.auto_local_provider.is_some()
            && !self
                .routed_ai_provider(AiFeature::Completion)?
                .is_local_completion_model()
        {
            self.ai_completion.unavailable = Some("本地模型不可用，请检查模型包和可用内存".into());
        }
        let providers = self.active_ai_providers()?;
        if let Some(first) = providers.first() {
            self.ai.statusline_model = if providers.iter().all(|p| {
                p.kind == first.kind
                    && p.model == first.model
                    && p.route_label() == first.route_label()
            }) {
                format!("AI 开 · {} / {}", first.route_label(), first.model)
            } else if self.ai_completion.enabled && self.ai_completion.unavailable.is_none() {
                let inline = self.ai_provider(AiFeature::Completion)?;
                format!("AI 开 · 补全 {} / {}", inline.route_label(), inline.model)
            } else {
                "AI 开 · 多模型".into()
            };
        } else if self.ai_completion.unavailable.is_some() {
            self.ai.statusline_model = "AI 开 · 本地补全不可用".into();
        }
        log::info!("AI GPU routing: {capacity} bytes dedicated, {required} bytes required, DeepSeek only = {}", self.ai.deepseek_only);
        Ok(())
    }

    pub fn deepseek_only(&self) -> bool {
        self.ai.deepseek_only
    }

    pub fn laptop(&self) -> bool {
        self.ai.laptop
    }

    pub fn requires_deepseek(&self, feature: AiFeature) -> bool {
        if feature == AiFeature::Completion {
            self.ai.deepseek_only && !self.ai_completion.local_only
        } else {
            self.ai.laptop || self.ai.deepseek_only
        }
    }

    pub fn ai_feature_settings(&mut self, feature: AiFeature) -> (&mut bool, &mut Option<String>) {
        match feature {
            AiFeature::Completion => (
                &mut self.ai_completion.enabled,
                &mut self.ai_completion.provider,
            ),
            AiFeature::Doc => (&mut self.ai_doc.enabled, &mut self.ai_doc.provider),
            AiFeature::Commit => (&mut self.ai_commit.enabled, &mut self.ai_commit.provider),
            AiFeature::Test => (&mut self.ai_test.enabled, &mut self.ai_test.provider),
            AiFeature::Merge => (&mut self.ai_merge.enabled, &mut self.ai_merge.provider),
            AiFeature::Explain => (&mut self.ai_explain.enabled, &mut self.ai_explain.provider),
            AiFeature::Generate => (
                &mut self.ai_generate.enabled,
                &mut self.ai_generate.provider,
            ),
        }
    }

    pub fn check_ai_enabled(&self) -> Result<()> {
        ensure!(self.ai.enabled, "AI 总开关已关闭，可在 :ai 面板中开启");
        Ok(())
    }

    pub fn ai_provider(&self, feature: AiFeature) -> Result<AiProviderConfig> {
        self.check_ai_enabled()?;
        if feature == AiFeature::Completion {
            if let Some(reason) = &self.ai_completion.unavailable {
                bail!("{reason}");
            }
        }
        let mut provider = self.routed_ai_provider(feature)?;
        if feature == AiFeature::Completion
            && self.ai.auto_local_provider.is_some()
            && self.ai_completion.provider.is_none()
        {
            ensure!(
                provider.fits_dedicated_vram(self.ai.dedicated_vram),
                "当前电脑没有可用的本地补全模型"
            );
        }
        let defaults = if provider.kind == AiProviderType::Deepseek {
            self.ai.deepseek_thinking.get(&feature)
        } else {
            None
        };
        if let Some(level) = self.ai.thinking.get(&feature).or(defaults) {
            if provider.kind != AiProviderType::OpenaiCompatible {
                provider.thinking = *level != AiThinking::Off;
                provider.reasoning_effort = match level {
                    AiThinking::Off => None,
                    AiThinking::Low => Some("low".into()),
                    AiThinking::High => Some("high".into()),
                    AiThinking::Max => Some("max".into()),
                };
                if provider.kind == AiProviderType::LlamaCpp && provider.thinking {
                    provider.thinking_budget.get_or_insert(2048);
                }
            }
        }
        Ok(provider)
    }

    fn routed_ai_provider(&self, feature: AiFeature) -> Result<AiProviderConfig> {
        if feature == AiFeature::Completion && self.ai_completion.local_only {
            let provider = self.configured_ai_provider(feature)?;
            ensure!(
                provider.is_local_completion_model(),
                "仅本地行内补全需要选择本机托管的 llama.cpp 模型"
            );
            return Ok(provider);
        }
        if self.requires_deepseek(feature) {
            return Ok(self.deepseek_provider(feature));
        }
        if feature != AiFeature::Completion && self.feature_provider_name(feature).is_none() {
            if let Some(name) = &self.ai.local_generation_provider {
                return Ok(self
                    .ai
                    .providers
                    .get(name)
                    .filter(|p| p.fits_dedicated_vram(self.ai.dedicated_vram))
                    .map(|p| self.generation_route(p))
                    .unwrap_or_else(|| self.deepseek_provider(feature)));
            }
            if self.automatic_local_models() {
                if let Some(p) = self
                    .local_generation_provider(feature)?
                    .filter(|p| p.fits_dedicated_vram(self.ai.dedicated_vram))
                    .or_else(|| self.discovered_generation())
                {
                    return Ok(self.generation_route(p));
                }
                if feature.is_interactive() || self.ai.auto_local_provider.is_none() {
                    return Ok(self.deepseek_provider(feature));
                }
            }
        }
        if self.feature_provider_name(feature).is_none() {
            if let Some(name) = &self.ai.auto_local_provider {
                let ordinary = self
                    .ai
                    .providers
                    .get(name)
                    .with_context(|| format!("Unknown automatic local AI provider '{name}'"))?;
                if ordinary.fits_dedicated_vram(self.ai.dedicated_vram) {
                    return Ok(ordinary.clone());
                }
            }
        }
        let provider = self.configured_ai_provider(feature)?;
        if feature != AiFeature::Completion
            && self.feature_provider_name(feature).is_some()
            && provider.is_local_completion_model()
        {
            if self.automatic_local_models()
                && self.ai.dedicated_vram != 0
                && !provider.fits_dedicated_vram(self.ai.dedicated_vram)
            {
                return Ok(self.deepseek_provider(feature));
            }
            return Ok(self.generation_route(&provider));
        }
        if feature.is_interactive()
            && self.feature_provider_name(feature).is_none()
            && provider.kind == AiProviderType::LlamaCpp
            && self.ai.dedicated_vram != 0
            && !provider.fits_dedicated_vram(self.ai.dedicated_vram)
        {
            return Ok(self.deepseek_provider(feature));
        }
        Ok(provider)
    }

    fn discovered_generation(&self) -> Option<&AiProviderConfig> {
        let provider = self
            .ai
            .providers
            .get(self.ai.automatic_generation.as_ref()?)?;
        provider
            .fits_dedicated_vram(self.ai.dedicated_vram)
            .then_some(provider)
    }

    fn generation_route(&self, provider: &AiProviderConfig) -> AiProviderConfig {
        let inline = self
            .ai_completion
            .provider
            .as_ref()
            .or(self.ai.auto_local_provider.as_ref())
            .or(self.ai.default_provider.as_ref())
            .and_then(|name| self.ai.providers.get(name));
        let mut local = provider.clone();
        local.on_demand = !self.ai_completion.enabled || inline != Some(provider);
        if local.on_demand && local.thinking {
            local.thinking_budget.get_or_insert(2048);
        }
        local
    }

    fn deepseek_provider(&self, feature: AiFeature) -> AiProviderConfig {
        self.feature_provider_name(feature)
            .into_iter()
            .chain(self.ai.default_provider.as_ref())
            .filter_map(|name| self.ai.providers.get(name))
            .chain(self.ai.providers.get("deepseek"))
            .chain(self.ai.providers.values())
            .find(|provider| provider.kind == AiProviderType::Deepseek)
            .cloned()
            .unwrap_or_else(AiProviderConfig::default_deepseek)
    }

    pub fn feature_provider_name(&self, feature: AiFeature) -> Option<&String> {
        match feature {
            AiFeature::Completion => self.ai_completion.provider.as_ref(),
            AiFeature::Doc => self.ai_doc.provider.as_ref(),
            AiFeature::Commit => self.ai_commit.provider.as_ref(),
            AiFeature::Test => self.ai_test.provider.as_ref(),
            AiFeature::Merge => self.ai_merge.provider.as_ref(),
            AiFeature::Explain => self.ai_explain.provider.as_ref(),
            AiFeature::Generate => self.ai_generate.provider.as_ref(),
        }
    }

    fn configured_ai_provider(&self, feature: AiFeature) -> Result<AiProviderConfig> {
        let explicit = self.feature_provider_name(feature);
        let selected = explicit.or(self.ai.default_provider.as_ref());
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
        if feature == AiFeature::Completion {
            // Discovered profiles normally load on demand. An explicit inline
            // choice owns a resident lease, including when generation shares it.
            provider.on_demand = false;
            return Ok(provider);
        }
        let use_generation_model = provider.kind == AiProviderType::LlamaCpp
            || (feature.is_interactive()
                && provider.kind == AiProviderType::OpenaiCompatible
                && provider.is_local());
        if explicit.is_none() && use_generation_model {
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
            } else if feature.is_interactive() {
                return Ok(self.deepseek_provider(feature));
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
            AiFeature::Merge => self.ai_merge.local_provider.as_ref(),
            AiFeature::Explain | AiFeature::Generate => {
                let settings = if feature == AiFeature::Explain {
                    &self.ai_explain
                } else {
                    &self.ai_generate
                };
                settings
                    .local_provider
                    .as_ref()
                    .or(self.ai_doc.local_provider.as_ref())
                    .or(self.ai_test.local_provider.as_ref())
                    .or(self.ai_commit.local_provider.as_ref())
            }
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
        if !self.ai.enabled {
            return Ok(providers);
        }
        for (feature, enabled) in self.ai_features() {
            if !enabled
                || (feature == AiFeature::Completion && self.ai_completion.unavailable.is_some())
            {
                continue;
            }
            let provider = self.routed_ai_provider(feature)?;
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
        ensure!(
            self.ai_completion.min_gap_confidence <= 100,
            "ai-completion.min-gap-confidence must be between 0 and 100"
        );
        if !self.ai.enabled {
            return Ok(());
        }
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
        if let Some(name) = &self.ai.auto_local_provider {
            ensure!(
                self.ai
                    .providers
                    .get(name)
                    .is_some_and(AiProviderConfig::is_local_completion_model),
                "Automatic local AI provider '{name}' must be a managed loopback llama.cpp model"
            );
        }
        if let Some(name) = &self.ai.local_generation_provider {
            ensure!(
                self.is_generation_choice(name)
                    || (name.starts_with("gguf-") && !self.ai.providers.contains_key(name)),
                "Local generation provider '{name}' must be a managed local generation model"
            );
        }
        for (feature, _) in self.ai_features() {
            self.local_generation_provider(feature)?;
            self.configured_ai_provider(feature)?;
        }
        self.active_ai_providers()?;
        Ok(())
    }
}
