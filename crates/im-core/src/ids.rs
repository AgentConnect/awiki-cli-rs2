use serde::{Deserialize, Serialize};

use base64::Engine;
use rand::RngCore;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IdentityId(String);

/// Public cryptographic endpoint identifier carried by ANP device-aware Profiles.
///
/// This identifier is independent from the local vault context identifier and
/// must never be derived from a hardware serial number or local storage scope.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProtocolDeviceId(String);

/// Local-only identifier binding encrypted SecretVault records to one runtime context.
///
/// It is not an ANP `device_id` and must never be serialized into a DID Document
/// or cross-domain message.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VaultContextDeviceId(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Did(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Handle(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PeerRef(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GroupRef(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageId(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ThreadId(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Cursor(String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<Cursor>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageLimit(pub u32);

impl IdentityId {
    pub fn parse(input: impl AsRef<str>) -> crate::ImResult<Self> {
        parse_non_empty("identity_id", input).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl ProtocolDeviceId {
    pub fn parse(input: impl AsRef<str>) -> crate::ImResult<Self> {
        let value = parse_non_empty("protocol_device_id", input)?;
        if value == "default" {
            return Err(crate::ImError::invalid_input(
                Some("protocol_device_id".to_string()),
                "protocol_device_id must not use the reserved legacy value default",
            ));
        }
        Ok(Self(value))
    }

    pub fn generate() -> crate::ImResult<Self> {
        let mut random = [0_u8; 16];
        rand::rngs::OsRng
            .try_fill_bytes(&mut random)
            .map_err(|_| crate::ImError::Internal {
                message: "secure protocol device id generation failed".to_owned(),
            })?;
        let value = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random);
        Ok(Self(format!("dev-{value}")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl VaultContextDeviceId {
    pub fn parse(input: impl AsRef<str>) -> crate::ImResult<Self> {
        parse_non_empty("vault_context_device_id", input).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Did {
    pub fn parse(input: impl AsRef<str>) -> crate::ImResult<Self> {
        let value = parse_non_empty("did", input)?;
        if !value.starts_with("did:") {
            return Err(crate::ImError::invalid_input(
                Some("did".to_string()),
                "DID must start with did:",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Handle {
    pub fn parse(input: impl AsRef<str>, default_domain: &str) -> crate::ImResult<Self> {
        let value = parse_non_empty("handle", input)?;
        let normalized = if value.contains('@') || value.contains('.') || default_domain.is_empty()
        {
            value
        } else {
            format!("{value}.{default_domain}")
        };
        Ok(Self(
            normalized.trim_start_matches('@').to_ascii_lowercase(),
        ))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl PeerRef {
    pub fn parse(input: impl AsRef<str>, default_domain: &str) -> crate::ImResult<Self> {
        let value = parse_non_empty("peer", input)?;
        if value.starts_with("did:") {
            return Ok(Self(value));
        }
        Ok(Self(
            Handle::parse(value, default_domain)?.as_str().to_string(),
        ))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl GroupRef {
    pub fn parse(input: impl AsRef<str>) -> crate::ImResult<Self> {
        parse_non_empty("group", input).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl MessageId {
    pub fn parse(input: impl AsRef<str>) -> crate::ImResult<Self> {
        parse_non_empty("message_id", input).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl ThreadId {
    pub fn parse(input: impl AsRef<str>) -> crate::ImResult<Self> {
        parse_non_empty("thread_id", input).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Cursor {
    pub fn parse(input: impl AsRef<str>) -> crate::ImResult<Self> {
        parse_non_empty("cursor", input).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl PageLimit {
    pub fn new(value: u32) -> crate::ImResult<Self> {
        if value == 0 {
            return Err(crate::ImError::invalid_input(
                Some("limit".to_string()),
                "limit must be greater than zero",
            ));
        }
        Ok(Self(value.min(100)))
    }
}

fn parse_non_empty(field: &'static str, input: impl AsRef<str>) -> crate::ImResult<String> {
    let value = input.as_ref().trim();
    if value.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some(field.to_string()),
            format!("{field} must not be empty"),
        ));
    }
    Ok(value.to_string())
}
