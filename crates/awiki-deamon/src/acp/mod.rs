//! ACP v1 transport and durable runtime control, independent of the legacy drivers.
pub mod attachments;
pub mod client;
pub mod host;
mod models;
pub mod model_refresh;
mod tools;
#[cfg(test)]
mod tools_tests;
pub mod operations;
pub mod session_configuration;
mod question_tool;
mod questions;
pub mod store;
pub mod task_records;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

pub const PLUGIN_ID: &str = "acp";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Brand {
    OpenCode,
    Gemini,
    Kimi,
    DeepseekHarness,
}

impl Brand {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "opencode" | "open-code" => Ok(Self::OpenCode),
            "gemini" | "gemini-cli" => Ok(Self::Gemini),
            "kimi" | "kimi-code" => Ok(Self::Kimi),
            "deepseek-harness" | "dsh" => Ok(Self::DeepseekHarness),
            _ => bail!("unsupported_acp_client"),
        }
    }
    pub fn id(self) -> &'static str {
        match self {
            Self::OpenCode => "opencode",
            Self::Gemini => "gemini",
            Self::Kimi => "kimi",
            Self::DeepseekHarness => "deepseek-harness",
        }
    }
    pub fn command(self) -> &'static str {
        match self {
            Self::OpenCode => "opencode",
            Self::Gemini => "gemini",
            Self::Kimi => "kimi",
            Self::DeepseekHarness => "dsh",
        }
    }
    pub fn args(self) -> &'static [&'static str] {
        match self {
            Self::OpenCode | Self::Kimi => &["acp"],
            Self::Gemini => &["--experimental-acp"],
            Self::DeepseekHarness => &["--profile", "acp"],
        }
    }
}
