//! Typed options for the managed llama.cpp process, independent of HTTP budgets.
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AiCacheType {
    #[default]
    #[serde(rename = "f16")]
    F16,
    #[serde(rename = "q8_0")]
    Q8,
    #[serde(rename = "q4_0")]
    Q4,
}

impl AiCacheType {
    pub fn argument(self) -> &'static str {
        match self {
            Self::F16 => "f16",
            Self::Q8 => "q8_0",
            Self::Q4 => "q4_0",
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AiSpeculativeDecoding {
    #[default]
    None,
    Mtp,
    NgramSimple,
    NgramMod,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AiModelLoadMode {
    #[default]
    Auto,
    Mmap,
    None,
    Dio,
}

impl AiModelLoadMode {
    pub fn argument(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Mmap => "mmap",
            Self::None => "none",
            Self::Dio => "dio",
        }
    }
}

impl AiSpeculativeDecoding {
    pub fn argument(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Mtp => "draft-mtp",
            Self::NgramSimple => "ngram-simple",
            Self::NgramMod => "ngram-mod",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct AiRuntimeConfig {
    pub cache_type_k: AiCacheType,
    pub cache_type_v: AiCacheType,
    pub speculative_decoding: AiSpeculativeDecoding,
    pub backend_sampling: bool,
    pub load_mode: AiModelLoadMode,
    pub draft_tokens: u32,
    pub threads: Option<u32>,
    pub threads_batch: Option<u32>,
    pub batch_size: Option<u32>,
    pub ubatch_size: Option<u32>,
}

impl Default for AiRuntimeConfig {
    fn default() -> Self {
        Self {
            cache_type_k: AiCacheType::F16,
            cache_type_v: AiCacheType::F16,
            speculative_decoding: AiSpeculativeDecoding::None,
            backend_sampling: false,
            load_mode: AiModelLoadMode::Auto,
            draft_tokens: 3,
            threads: None,
            threads_batch: None,
            batch_size: None,
            ubatch_size: None,
        }
    }
}

impl AiRuntimeConfig {
    pub(super) fn validate(&self) -> Result<()> {
        ensure!(
            (1..=64).contains(&self.draft_tokens),
            "runtime.draft-tokens must be between 1 and 64"
        );
        for (name, count) in [
            ("threads", self.threads),
            ("threads-batch", self.threads_batch),
        ] {
            ensure!(
                count.is_none_or(|n| (1..=1024).contains(&n)),
                "runtime.{name} must be between 1 and 1024"
            );
        }
        for (name, count) in [
            ("batch-size", self.batch_size),
            ("ubatch-size", self.ubatch_size),
        ] {
            ensure!(
                count.is_none_or(|n| (1..=65536).contains(&n)),
                "runtime.{name} must be between 1 and 65536"
            );
        }
        ensure!(
            self.ubatch_size.unwrap_or(512) <= self.batch_size.unwrap_or(2048),
            "runtime.ubatch-size must not exceed batch-size"
        );
        Ok(())
    }
}
