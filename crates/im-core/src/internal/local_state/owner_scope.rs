use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnerScope {
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) device_id: Option<String>,
    pub(crate) credential_name: Option<String>,
}

impl OwnerScope {
    pub(crate) fn new(
        owner_identity_id: impl Into<String>,
        owner_did: impl Into<String>,
    ) -> crate::ImResult<Self> {
        Ok(Self {
            owner_identity_id: Self::require_identity_id(owner_identity_id)?,
            owner_did: require_non_empty("owner_did", owner_did.into())?,
            device_id: None,
            credential_name: None,
        })
    }

    pub(crate) fn for_client(client: &crate::core::ImClient) -> crate::ImResult<Self> {
        Self::for_identity(client.current_identity())
    }

    pub(crate) fn for_identity(
        identity: &crate::identity::IdentitySummary,
    ) -> crate::ImResult<Self> {
        let mut scope = Self::new(identity.id.as_str(), identity.did.as_str())?;
        scope.device_id = optional_trimmed(identity.device_id.clone());
        scope.credential_name = optional_trimmed(identity.local_alias.clone());
        Ok(scope)
    }

    pub(crate) fn require_identity_id(value: impl Into<String>) -> crate::ImResult<String> {
        require_non_empty("owner_identity_id", value.into())
    }

    pub(crate) fn with_device_id(mut self, device_id: impl Into<String>) -> Self {
        self.device_id = optional_trimmed(Some(device_id.into()));
        self
    }

    pub(crate) fn with_credential_name(mut self, credential_name: impl Into<String>) -> Self {
        self.credential_name = optional_trimmed(Some(credential_name.into()));
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DirectPeerScope {
    pub(crate) user_id: String,
    pub(crate) full_handle: String,
}

impl DirectPeerScope {
    pub(crate) fn new(
        user_id: impl Into<String>,
        full_handle: impl Into<String>,
    ) -> crate::ImResult<Self> {
        Ok(Self {
            user_id: require_non_empty("peer_user_id", user_id.into())?,
            full_handle: normalize_full_handle(full_handle.into())?,
        })
    }
}

pub(crate) fn direct_conversation_id(peer_did: &str) -> String {
    let peer_did = peer_did.trim();
    if peer_did.is_empty() {
        "dm:unknown".to_owned()
    } else {
        format!("dm:{peer_did}")
    }
}

pub(crate) fn direct_conversation_id_for_peer_scope(scope: &DirectPeerScope) -> String {
    let input = format!("user:{}\nhandle:{}", scope.user_id, scope.full_handle);
    format!("dm:peer-scope:v1:{}", sha256_hex(input.as_bytes()))
}

pub(crate) fn direct_conversation_id_from_thread_alias(
    thread_id: &str,
    owner_did: &str,
) -> Option<String> {
    let alias = thread_id.trim().strip_prefix("dm:")?.trim();
    let owner_did = owner_did.trim();
    if alias.is_empty() {
        return Some(direct_conversation_id(""));
    }
    if owner_did.is_empty() {
        return Some(direct_conversation_id(alias));
    }
    if let Some(peer) = alias
        .strip_prefix(owner_did)
        .and_then(|rest| rest.strip_prefix(':'))
        .filter(|peer| !peer.trim().is_empty())
    {
        return Some(direct_conversation_id(peer));
    }
    if let Some(peer) = alias
        .strip_suffix(owner_did)
        .and_then(|rest| rest.strip_suffix(':'))
        .filter(|peer| !peer.trim().is_empty())
    {
        return Some(direct_conversation_id(peer));
    }
    Some(direct_conversation_id(alias))
}

pub(crate) fn group_conversation_id(group_id_or_did: &str) -> String {
    let group_id_or_did = group_id_or_did.trim();
    if group_id_or_did.is_empty() {
        "group:unknown".to_owned()
    } else {
        format!("group:{group_id_or_did}")
    }
}

pub(crate) fn mail_conversation_id(source: &str) -> String {
    let source = source.trim();
    if source.is_empty() {
        "mail:unknown".to_owned()
    } else {
        format!("mail:{source}")
    }
}

fn require_non_empty(field: &'static str, value: String) -> crate::ImResult<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            format!("{field} must not be empty"),
        ));
    }
    Ok(value.to_owned())
}

fn normalize_full_handle(value: String) -> crate::ImResult<String> {
    let value = require_non_empty("peer_full_handle", value)?;
    Ok(value.to_ascii_lowercase())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    encoded
}

fn optional_trimmed(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_scope_rejects_empty_identity_id() {
        let err = OwnerScope::new("  ", "did:example:alice").unwrap_err();

        assert!(matches!(
            err,
            crate::ImError::InvalidInput {
                field: Some(ref field),
                ..
            } if field == "owner_identity_id"
        ));
    }

    #[test]
    fn owner_scope_trims_metadata_without_using_it_as_owner_fallback() {
        let scope = OwnerScope::new(" alice-id ", " did:example:alice ")
            .unwrap()
            .with_device_id(" device-a ")
            .with_credential_name(" alice ");

        assert_eq!(scope.owner_identity_id, "alice-id");
        assert_eq!(scope.owner_did, "did:example:alice");
        assert_eq!(scope.device_id.as_deref(), Some("device-a"));
        assert_eq!(scope.credential_name.as_deref(), Some("alice"));
    }

    #[test]
    fn conversation_ids_are_stable_without_local_owner_did() {
        assert_eq!(
            direct_conversation_id(" did:example:bob "),
            "dm:did:example:bob"
        );
        assert_eq!(
            group_conversation_id(" did:example:group "),
            "group:did:example:group"
        );
        assert_eq!(mail_conversation_id(" inbox "), "mail:inbox");
    }

    #[test]
    fn direct_aliases_drop_local_owner_did() {
        assert_eq!(
            direct_conversation_id_from_thread_alias(
                "dm:did:example:alice:did:example:bob",
                "did:example:alice",
            ),
            Some("dm:did:example:bob".to_owned())
        );
        assert_eq!(
            direct_conversation_id_from_thread_alias(
                "dm:did:example:bob:did:example:alice",
                "did:example:alice",
            ),
            Some("dm:did:example:bob".to_owned())
        );
        assert_eq!(
            direct_conversation_id_from_thread_alias("dm:did:example:bob", "did:example:alice"),
            Some("dm:did:example:bob".to_owned())
        );
    }

    #[test]
    fn direct_peer_scope_conversation_ids_ignore_did_rotation() {
        let scope = DirectPeerScope::new("user-1", " Alice.AnPClaw.com ").expect("valid scope");
        let same_scope = DirectPeerScope::new("user-1", "alice.anpclaw.com").expect("valid scope");

        assert_eq!(
            direct_conversation_id_for_peer_scope(&scope),
            direct_conversation_id_for_peer_scope(&same_scope)
        );
        assert!(direct_conversation_id_for_peer_scope(&scope).starts_with("dm:peer-scope:v1:"));
    }

    #[test]
    fn direct_peer_scope_separates_handle_reuse_between_users() {
        let old_owner = DirectPeerScope::new("user-1", "alice.anpclaw.com").expect("valid scope");
        let new_owner = DirectPeerScope::new("user-2", "alice.anpclaw.com").expect("valid scope");

        assert_ne!(
            direct_conversation_id_for_peer_scope(&old_owner),
            direct_conversation_id_for_peer_scope(&new_owner)
        );
    }
}
