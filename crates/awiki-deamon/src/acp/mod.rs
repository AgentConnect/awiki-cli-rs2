//! ACP v1 transport and durable runtime control, independent of the legacy drivers.
pub mod attachments;
pub(crate) mod background;
pub mod client;
pub(crate) mod components;
pub(crate) mod hermes_profile;
pub mod host;
pub mod mcp_stdio;
pub mod model_refresh;
mod models;
pub mod operations;
mod question_tool;
mod questions;
pub mod session_configuration;
pub mod store;
pub mod task_records;
mod tools;
#[cfg(test)]
mod tools_tests;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

pub const PLUGIN_ID: &str = "acp";
pub const SUPPORTED_DRIVERS: [&str; 7] = [
    "hermes",
    "codex",
    "claude-code",
    "opencode",
    "gemini",
    "kimi",
    "deepseek-harness",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Brand {
    Hermes,
    Codex,
    ClaudeCode,
    OpenCode,
    Gemini,
    Kimi,
    DeepseekHarness,
}

impl Brand {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "hermes" => Ok(Self::Hermes),
            "codex" | "codex-cli" => Ok(Self::Codex),
            "claude-code" => Ok(Self::ClaudeCode),
            "opencode" | "open-code" => Ok(Self::OpenCode),
            "gemini" | "gemini-cli" => Ok(Self::Gemini),
            "kimi" | "kimi-code" => Ok(Self::Kimi),
            "deepseek-harness" | "dsh" => Ok(Self::DeepseekHarness),
            _ => bail!("unsupported_acp_client"),
        }
    }
    pub fn id(self) -> &'static str {
        match self {
            Self::Hermes => "hermes",
            Self::Codex => "codex",
            Self::ClaudeCode => "claude-code",
            Self::OpenCode => "opencode",
            Self::Gemini => "gemini",
            Self::Kimi => "kimi",
            Self::DeepseekHarness => "deepseek-harness",
        }
    }
    pub fn command(self) -> &'static str {
        match self {
            Self::Hermes => "hermes",
            Self::Codex => "codex",
            Self::ClaudeCode => "claude",
            Self::OpenCode => "opencode",
            Self::Gemini => "gemini",
            Self::Kimi => "kimi",
            Self::DeepseekHarness => "dsh",
        }
    }
    pub fn args(self) -> &'static [&'static str] {
        match self {
            Self::Hermes | Self::OpenCode | Self::Kimi => &["acp"],
            Self::Codex | Self::ClaudeCode => &[],
            Self::Gemini => &["--experimental-acp"],
            Self::DeepseekHarness => &["--profile", "acp"],
        }
    }
}
