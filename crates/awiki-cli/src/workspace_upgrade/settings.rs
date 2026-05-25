use crate::workspace_config;
use serde::Deserialize;
use std::fmt;
use std::fs;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NormalizedLegacySettings {
    pub service_base_url: String,
    pub did_domain: String,
    pub runtime_mode: String,
}

#[derive(Debug)]
pub enum LegacySettingsError {
    Read(std::io::Error),
    Parse(serde_json::Error),
    SplitServiceUrls {
        user_service_url: String,
        molt_message_url: String,
    },
}

impl fmt::Display for LegacySettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(err) => write!(f, "read legacy settings: {err}"),
            Self::Parse(err) => write!(f, "parse legacy settings: {err}"),
            Self::SplitServiceUrls {
                user_service_url,
                molt_message_url,
            } => write!(
                f,
                "legacy settings use different user_service_url ({user_service_url}) and molt_message_url ({molt_message_url}); automatic migration to one service_base_url is not supported"
            ),
        }
    }
}

impl std::error::Error for LegacySettingsError {}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct LegacySettingsFile {
    user_service_url: String,
    molt_message_url: String,
    did_domain: String,
    message_transport: LegacyMessageTransport,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct LegacyMessageTransport {
    receive_mode: String,
}

pub fn load_legacy_settings(path: &str) -> Result<NormalizedLegacySettings, LegacySettingsError> {
    let raw = fs::read(path).map_err(LegacySettingsError::Read)?;
    parse_legacy_settings(&raw)
}

pub fn parse_legacy_settings(raw: &[u8]) -> Result<NormalizedLegacySettings, LegacySettingsError> {
    let legacy: LegacySettingsFile =
        serde_json::from_slice(raw).map_err(LegacySettingsError::Parse)?;
    let runtime_mode = if legacy
        .message_transport
        .receive_mode
        .trim()
        .eq_ignore_ascii_case("websocket")
    {
        "websocket"
    } else {
        "http"
    }
    .to_string();
    let user_service_url = workspace_config::normalize_base_url(legacy.user_service_url.trim());
    let molt_message_url = workspace_config::normalize_base_url(legacy.molt_message_url.trim());
    if !user_service_url.is_empty()
        && !molt_message_url.is_empty()
        && user_service_url != molt_message_url
    {
        return Err(LegacySettingsError::SplitServiceUrls {
            user_service_url,
            molt_message_url,
        });
    }
    let service_base_url = if user_service_url.is_empty() {
        molt_message_url
    } else {
        user_service_url
    };
    Ok(NormalizedLegacySettings {
        service_base_url,
        did_domain: legacy.did_domain,
        runtime_mode,
    })
}
