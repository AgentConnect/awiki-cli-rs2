use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeTokenScope {
    pub agent_did: String,
    pub runtime_profile_id: String,
    pub run_id: String,
    pub allowed_methods: Vec<RpcMethod>,
    pub allowed_recipients: Option<Vec<String>>,
    pub allowed_message_security: Option<Vec<String>>,
    pub expires_at_ms: i64,
    pub single_use: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RpcMethod {
    RpcPing,
    TaskStatus,
    TaskFinish,
    MsgSend,
    SendAttachment,
    ArtifactCreated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RpcMethodLevel {
    Read,
    Status,
    Message,
    Admin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedRuntimeToken {
    pub token_id: String,
    pub token: RuntimeRpcToken,
    pub scope: RuntimeTokenScope,
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeRpcToken(String);

pub const ACTIVE_HANDLE_LOOKUP_RECIPIENT_SCOPE: &str = "@active_handle_lookup";
pub const ANY_DIRECT_RECIPIENT_SCOPE: &str = "@any_direct";
pub const ANY_GROUP_RECIPIENT_SCOPE: &str = "@any_group";

impl RuntimeRpcToken {
    pub fn generate() -> Self {
        let mut secret = [0_u8; 32];
        rand::thread_rng().fill_bytes(&mut secret);
        Self(format!("rtok_{}", URL_SAFE_NO_PAD.encode(secret)))
    }

    pub fn parse(input: impl Into<String>) -> Result<Self> {
        let value = input.into();
        if !value.starts_with("rtok_") {
            bail!("runtime RPC token must start with rtok_");
        }
        if value.len() < 32 {
            bail!("runtime RPC token is too short");
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn token_id(&self) -> String {
        let digest = Sha256::digest(self.0.as_bytes());
        format!("rtokid_{}", URL_SAFE_NO_PAD.encode(&digest[..16]))
    }

    pub fn secret_hash(&self) -> String {
        let digest = Sha256::digest(self.0.as_bytes());
        URL_SAFE_NO_PAD.encode(digest)
    }
}

impl fmt::Debug for RuntimeRpcToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RuntimeRpcToken(<redacted>)")
    }
}

impl fmt::Display for RuntimeRpcToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted-runtime-rpc-token>")
    }
}

impl RpcMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RpcPing => "rpc.ping",
            Self::TaskStatus => "task.status",
            Self::TaskFinish => "task.finish",
            Self::MsgSend => "msg.send",
            Self::SendAttachment => "attachment.send",
            Self::ArtifactCreated => "artifact.created",
        }
    }

    pub fn parse(input: &str) -> Result<Self> {
        match input {
            "rpc.ping" => Ok(Self::RpcPing),
            "task.status" => Ok(Self::TaskStatus),
            "task.finish" => Ok(Self::TaskFinish),
            "msg.send" => Ok(Self::MsgSend),
            "attachment.send" => Ok(Self::SendAttachment),
            "artifact.created" => Ok(Self::ArtifactCreated),
            other => bail!("unsupported RPC method: {other}"),
        }
    }

    pub fn level(&self) -> RpcMethodLevel {
        match self {
            Self::RpcPing => RpcMethodLevel::Read,
            Self::TaskStatus | Self::TaskFinish => RpcMethodLevel::Status,
            Self::MsgSend | Self::SendAttachment | Self::ArtifactCreated => RpcMethodLevel::Message,
        }
    }
}

impl RuntimeTokenScope {
    pub fn new(
        agent_did: impl Into<String>,
        runtime_profile_id: impl Into<String>,
        run_id: impl Into<String>,
        allowed_methods: Vec<RpcMethod>,
        allowed_recipients: Option<Vec<String>>,
        ttl: Duration,
    ) -> Result<Self> {
        let now = current_time_millis()?;
        let expires_at_ms = now
            .checked_add(ttl.as_millis() as i64)
            .ok_or_else(|| anyhow::anyhow!("runtime RPC token expiry overflow"))?;
        let scope = Self {
            agent_did: agent_did.into(),
            runtime_profile_id: runtime_profile_id.into(),
            run_id: run_id.into(),
            allowed_methods,
            allowed_recipients,
            allowed_message_security: None,
            expires_at_ms,
            single_use: false,
        };
        scope.validate()?;
        Ok(scope)
    }

    pub fn validate(&self) -> Result<()> {
        if self.agent_did.trim().is_empty() {
            bail!("agent_did must not be empty");
        }
        if self.runtime_profile_id.trim().is_empty() {
            bail!("runtime_profile_id must not be empty");
        }
        if self.run_id.trim().is_empty() {
            bail!("run_id must not be empty");
        }
        if self.allowed_methods.is_empty() {
            bail!("allowed_methods must not be empty");
        }
        if let Some(recipients) = self.allowed_recipients.as_ref() {
            if recipients
                .iter()
                .any(|recipient| recipient.trim().is_empty())
            {
                bail!("allowed_recipients must not contain blank entries");
            }
        }
        if let Some(security_modes) = self.allowed_message_security.as_ref() {
            if security_modes
                .iter()
                .any(|security| security.trim().is_empty())
            {
                bail!("allowed_message_security must not contain blank entries");
            }
        }
        Ok(())
    }

    pub fn allows_method(&self, method: &RpcMethod) -> bool {
        self.allowed_methods.iter().any(|allowed| allowed == method)
    }

    pub fn allows_recipient(&self, recipient: Option<&str>) -> bool {
        let Some(recipient) = recipient else {
            return self.allows_recipient_candidates(std::iter::empty::<&str>());
        };
        self.allows_recipient_candidates([recipient])
    }

    pub fn allows_recipient_candidates<'a>(
        &self,
        recipients: impl IntoIterator<Item = &'a str>,
    ) -> bool {
        let Some(allowed) = self.allowed_recipients.as_ref() else {
            return true;
        };
        let recipients = recipients
            .into_iter()
            .filter_map(|recipient| {
                let recipient = recipient.trim();
                (!recipient.is_empty()).then_some(recipient)
            })
            .collect::<Vec<_>>();

        if allowed.iter().any(|known| {
            normalize_recipient_for_scope(known) == ACTIVE_HANDLE_LOOKUP_RECIPIENT_SCOPE
        }) && recipients
            .iter()
            .any(|recipient| normalized_recipient_is_handle(recipient))
            && recipients
                .iter()
                .any(|recipient| recipient.trim().starts_with("did:"))
        {
            return true;
        }

        if allowed
            .iter()
            .any(|known| normalize_recipient_for_scope(known) == ANY_DIRECT_RECIPIENT_SCOPE)
            && recipients
                .iter()
                .any(|recipient| normalized_recipient_is_direct(recipient))
        {
            return true;
        }

        if allowed
            .iter()
            .any(|known| normalize_recipient_for_scope(known) == ANY_GROUP_RECIPIENT_SCOPE)
            && recipients
                .iter()
                .any(|recipient| normalized_recipient_is_group(recipient))
        {
            return true;
        }

        recipients.iter().any(|recipient| {
            let candidate = normalize_recipient_for_scope(recipient);
            allowed
                .iter()
                .any(|known| normalize_recipient_for_scope(known) == candidate)
        })
    }

    pub fn allows_message_security(&self, security: Option<&str>) -> bool {
        let Some(allowed) = self.allowed_message_security.as_ref() else {
            return true;
        };
        let Some(security) = security else {
            return false;
        };
        let security = security.trim();
        allowed.iter().any(|known| known.trim() == security)
    }
}

fn normalize_recipient_for_scope(input: &str) -> String {
    let value = input.trim();
    if value.starts_with("did:") {
        value.to_string()
    } else if value == ACTIVE_HANDLE_LOOKUP_RECIPIENT_SCOPE
        || value == ANY_DIRECT_RECIPIENT_SCOPE
        || value == ANY_GROUP_RECIPIENT_SCOPE
    {
        value.to_string()
    } else if value.starts_with('@') {
        value.to_ascii_lowercase()
    } else {
        format!("@{}", value.to_ascii_lowercase())
    }
}

fn normalized_recipient_is_handle(input: &str) -> bool {
    let value = input.trim();
    !value.is_empty() && !value.starts_with("did:")
}

fn normalized_recipient_is_direct(input: &str) -> bool {
    let value = input.trim().to_ascii_lowercase();
    !value.is_empty() && !normalized_recipient_is_group(&value)
}

fn normalized_recipient_is_group(input: &str) -> bool {
    let value = input.trim().to_ascii_lowercase();
    value.starts_with("did:group:")
        || (value.starts_with("did:wba:") && value.contains(":groups:"))
        || value.starts_with("group:")
        || value.starts_with("grp_")
        || value.starts_with("group_")
}

pub fn issue_runtime_token(scope: RuntimeTokenScope) -> Result<IssuedRuntimeToken> {
    scope.validate()?;
    let token = RuntimeRpcToken::generate();
    let token_id = token.token_id();
    Ok(IssuedRuntimeToken {
        token_id,
        token,
        scope,
    })
}

pub fn current_time_millis() -> Result<i64> {
    let duration = SystemTime::now().duration_since(UNIX_EPOCH)?;
    Ok(duration.as_millis() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_token_debug_and_display_are_redacted() {
        let token = RuntimeRpcToken::generate();

        assert!(!format!("{token:?}").contains(token.as_str()));
        assert!(!format!("{token}").contains(token.as_str()));
        assert_ne!(token.secret_hash(), token.as_str());
        assert_ne!(token.token_id(), token.as_str());
    }

    #[test]
    fn scope_enforces_method_and_recipient() {
        let scope = RuntimeTokenScope::new(
            "did:agent:test",
            "profile_1",
            "run_1",
            vec![RpcMethod::TaskStatus, RpcMethod::MsgSend],
            Some(vec!["@alice".to_string()]),
            Duration::from_secs(60),
        )
        .unwrap();

        assert!(scope.allows_method(&RpcMethod::TaskStatus));
        assert!(!scope.allows_method(&RpcMethod::TaskFinish));
        assert!(scope.allows_recipient(Some("@alice")));
        assert!(scope.allows_recipient(Some("alice")));
        assert!(!scope.allows_recipient(Some("@bob")));
    }

    #[test]
    fn scope_enforces_recipient_candidates_and_security() {
        let mut scope = RuntimeTokenScope::new(
            "did:agent:test",
            "profile_1",
            "run_1",
            vec![RpcMethod::MsgSend],
            Some(vec!["@bob".to_string(), "did:human:alice".to_string()]),
            Duration::from_secs(60),
        )
        .unwrap();
        scope.allowed_message_security = Some(vec!["default_plain".to_string()]);

        assert!(scope.allows_recipient_candidates(["@unknown", "did:human:alice"]));
        assert!(scope.allows_recipient_candidates(["bob", "did:human:bob"]));
        assert!(!scope.allows_recipient_candidates(["@mallory", "did:human:mallory"]));
        assert!(scope.allows_message_security(Some("default_plain")));
        assert!(!scope.allows_message_security(Some("direct_e2ee")));

        let handle_lookup_scope = RuntimeTokenScope::new(
            "did:agent:test",
            "profile_1",
            "run_2",
            vec![RpcMethod::MsgSend],
            Some(vec![ACTIVE_HANDLE_LOOKUP_RECIPIENT_SCOPE.to_string()]),
            Duration::from_secs(60),
        )
        .unwrap();
        assert!(handle_lookup_scope.allows_recipient_candidates(["alice", "did:human:alice"]));
        assert!(!handle_lookup_scope.allows_recipient(Some("did:human:alice")));
        assert!(!handle_lookup_scope.allows_recipient(Some("alice")));

        let direct_scope = RuntimeTokenScope::new(
            "did:agent:test",
            "profile_1",
            "run_3",
            vec![RpcMethod::MsgSend],
            Some(vec![ANY_DIRECT_RECIPIENT_SCOPE.to_string()]),
            Duration::from_secs(60),
        )
        .unwrap();
        assert!(direct_scope.allows_recipient(Some("alice")));
        assert!(direct_scope.allows_recipient(Some("did:human:alice")));
        assert!(!direct_scope.allows_recipient(Some("did:group:team")));
        assert!(!direct_scope.allows_recipient(Some("did:wba:awiki.ai:groups:demo:e1_group")));

        let group_scope = RuntimeTokenScope::new(
            "did:agent:test",
            "profile_1",
            "run_4",
            vec![RpcMethod::MsgSend],
            Some(vec![ANY_GROUP_RECIPIENT_SCOPE.to_string()]),
            Duration::from_secs(60),
        )
        .unwrap();
        assert!(group_scope.allows_recipient(Some("did:group:team")));
        assert!(group_scope.allows_recipient(Some("did:wba:awiki.ai:groups:demo:e1_group")));
        assert!(group_scope.allows_recipient(Some("group:team")));
        assert!(!group_scope.allows_recipient(Some("did:human:alice")));
    }
}
