//! Provider capability ceilings and finite budgets shared by every AI feature.

use super::{AiBudgetMode, AiProviderConfig, AiProviderType};

const DEEPSEEK_CONTEXT: usize = 1_048_576;
const DEEPSEEK_OUTPUT: u32 = 393_216;
const LARGE_BYTES: usize = 16 * 1024 * 1024;
const DEEPSEEK_TIMEOUT_MS: u64 = 7_200_000;

impl AiProviderConfig {
    fn known_deepseek_budget(&self) -> bool {
        self.kind == AiProviderType::Deepseek
            && matches!(
                self.model.as_str(),
                "deepseek-flash"
                    | "deepseek-v4-pro"
                    | "deepseek-v4-flash"
                    | "deepseek-v4-flash-vision-exp"
            )
    }

    /// Automatic limits require an explicitly recognized provider and model.
    pub fn automatic_budget(&self) -> bool {
        self.budget_mode == AiBudgetMode::Auto && self.known_deepseek_budget()
    }

    pub fn budget_context(&self) -> usize {
        if self.automatic_budget() {
            DEEPSEEK_CONTEXT
        } else {
            (self.context_size as usize).clamp(
                512,
                if self.known_deepseek_budget() {
                    DEEPSEEK_CONTEXT
                } else {
                    131_072
                },
            )
        }
    }

    pub fn budget_input_bytes(&self, configured: usize) -> usize {
        if self.automatic_budget() {
            LARGE_BYTES
        } else {
            configured
        }
    }

    pub fn budget_output_tokens(&self, configured: u32) -> u32 {
        if self.automatic_budget() {
            DEEPSEEK_OUTPUT
        } else {
            configured.min(if self.known_deepseek_budget() {
                DEEPSEEK_OUTPUT
            } else {
                16_384
            })
        }
    }

    pub fn budget_timeout_ms(&self, configured: u64) -> u64 {
        if self.automatic_budget() {
            DEEPSEEK_TIMEOUT_MS
        } else {
            configured.clamp(
                100,
                if self.known_deepseek_budget() {
                    DEEPSEEK_TIMEOUT_MS
                } else {
                    600_000
                },
            )
        }
    }

    pub fn budget_response_bytes(&self) -> usize {
        if self.known_deepseek_budget() {
            LARGE_BYTES
        } else {
            262_144
        }
    }

    pub fn budget_artifact_bytes(&self, configured: usize) -> usize {
        if self.known_deepseek_budget() {
            LARGE_BYTES
        } else {
            configured
        }
    }

    /// Auto DeepSeek uses a memory envelope, leaving exact token accounting to
    /// the API. Configured providers retain the conservative byte/token estimate.
    pub fn budget_fits(&self, input_bytes: usize, output_tokens: u32, overhead: usize) -> bool {
        if self.automatic_budget() {
            input_bytes.saturating_add(overhead) <= LARGE_BYTES && output_tokens <= DEEPSEEK_OUTPUT
        } else {
            input_bytes
                .saturating_add(output_tokens as usize)
                .saturating_add(overhead)
                <= self.budget_context()
        }
    }

    pub fn budget_file_limit(&self, configured: usize) -> usize {
        if self.automatic_budget() {
            4096
        } else {
            configured.min(1024)
        }
    }

    pub fn budget_batch_files(&self) -> usize {
        if self.automatic_budget() {
            128
        } else {
            4
        }
    }

    pub fn budget_request_limit(&self) -> usize {
        if self.automatic_budget() {
            256
        } else {
            64
        }
    }
}
