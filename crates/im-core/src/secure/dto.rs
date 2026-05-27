use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SecureOutboxId(String);

impl SecureOutboxId {
    pub fn parse(input: impl AsRef<str>) -> crate::ImResult<Self> {
        let value = input.as_ref().trim();
        if value.is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("outbox_id".to_owned()),
                "outbox_id must not be empty",
            ));
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectSecureStatus {
    pub peer: crate::ids::PeerRef,
    pub resolved_peer: Option<crate::ids::Did>,
    pub state: DirectSecureState,
    pub can_send_secure: bool,
    pub pending_outbox_count: u32,
    pub problem: Option<SecureProblem>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DirectSecureState {
    Ready,
    Preparing,
    WaitingForPeer,
    NeedsRepair,
    Unavailable,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectSecurePrepareResult {
    pub peer: crate::ids::PeerRef,
    pub state: DirectSecureState,
    pub can_send_secure: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectSecureRepairResult {
    pub peer: crate::ids::PeerRef,
    pub state: DirectSecureState,
    pub repaired: bool,
    pub problem: Option<SecureProblem>,
    pub prepared_local_send_state: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupSecureStatus {
    pub group: crate::ids::GroupRef,
    pub state: GroupSecureState,
    pub can_send_secure: bool,
    pub local_readiness: GroupSecureLocalReadiness,
    pub pending_work: GroupSecurePendingWork,
    pub problem: Option<SecureProblem>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GroupSecureState {
    Ready,
    Syncing,
    NeedsRepair,
    WaitingForMembershipUpdate,
    MissingLocalState,
    Unavailable,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupSecureLocalReadiness {
    pub has_local_state: bool,
    pub has_active_membership: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GroupSecurePendingWork {
    pub pending_notices: u32,
    pub pending_commits: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupSecurePrepareResult {
    pub group: crate::ids::GroupRef,
    pub state: GroupSecureState,
    pub can_send_secure: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupSecureRepairResult {
    pub group: crate::ids::GroupRef,
    pub state: GroupSecureState,
    pub repaired: bool,
    pub problem: Option<SecureProblem>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecureOutboxEntry {
    pub id: SecureOutboxId,
    pub target: crate::messages::MessageTarget,
    pub message_kind: String,
    pub status: SecureOutboxStatus,
    pub attempt_count: u32,
    pub last_error: Option<SecureProblem>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SecureOutboxStatus {
    Queued,
    Sending,
    Failed,
    Sent,
    Dropped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecureOutboxResult {
    pub id: SecureOutboxId,
    pub status: SecureOutboxStatus,
    pub delivery: Option<SecureDelivery>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecureDelivery {
    pub message_id: Option<crate::ids::MessageId>,
    pub state: crate::messages::DeliveryState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecureProblem {
    pub code: SecureProblemCode,
    pub message: String,
    pub retryable: bool,
}

impl SecureProblem {
    pub(crate) fn peer_not_found() -> Self {
        Self {
            code: SecureProblemCode::PeerNotFound,
            message: "peer did is not available in local directory cache".to_owned(),
            retryable: true,
        }
    }

    pub(crate) fn peer_keys_unavailable(message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code: SecureProblemCode::PeerKeysUnavailable,
            message: message.into(),
            retryable,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SecureProblemCode {
    IdentityNotReady,
    PeerNotFound,
    PeerKeysUnavailable,
    SessionNeedsRepair,
    GroupStateUnavailable,
    LocalStateUnavailable,
    TransportUnavailable,
    Unsupported,
    Unknown,
}
