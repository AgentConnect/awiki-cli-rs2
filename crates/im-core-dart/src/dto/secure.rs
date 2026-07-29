use crate::dto::message::DartMessageTarget;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartDirectSecureState {
    Ready,
    Preparing,
    WaitingForPeer,
    NeedsRepair,
    Unavailable,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartDirectSecureStatus {
    pub peer: String,
    pub resolved_peer: Option<String>,
    pub state: DartDirectSecureState,
    pub can_send_secure: bool,
    pub pending_outbox_count: u32,
    pub problem: Option<DartSecureProblem>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartDirectSecurePrepareResult {
    pub peer: String,
    pub state: DartDirectSecureState,
    pub can_send_secure: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartDirectSecureRepairResult {
    pub peer: String,
    pub state: DartDirectSecureState,
    pub repaired: bool,
    pub problem: Option<DartSecureProblem>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartGroupSecureState {
    Ready,
    Syncing,
    NeedsRepair,
    WaitingForMembershipUpdate,
    MissingLocalState,
    Unavailable,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartGroupSecureStatus {
    pub group: String,
    pub state: DartGroupSecureState,
    pub can_send_secure: bool,
    pub local_readiness: DartGroupSecureLocalReadiness,
    pub pending_work: DartGroupSecurePendingWork,
    pub problem: Option<DartSecureProblem>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartGroupSecureLocalReadiness {
    pub has_local_state: bool,
    pub has_active_membership: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartGroupSecurePendingWork {
    pub pending_notices: u32,
    pub pending_commits: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartGroupSecurePrepareResult {
    pub group: String,
    pub state: DartGroupSecureState,
    pub can_send_secure: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartGroupSecureRepairResult {
    pub group: String,
    pub state: DartGroupSecureState,
    pub repaired: bool,
    pub added_devices: u32,
    pub removed_devices: u32,
    pub remaining_devices: u32,
    pub problem: Option<DartSecureProblem>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartSecureOutboxEntry {
    pub id: String,
    pub target: DartMessageTarget,
    pub message_kind: String,
    pub status: DartSecureOutboxStatus,
    pub attempt_count: u32,
    pub last_error: Option<DartSecureProblem>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartSecureOutboxStatus {
    Queued,
    Sending,
    Failed,
    Sent,
    Dropped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartSecureOutboxResult {
    pub id: String,
    pub status: DartSecureOutboxStatus,
    pub delivery: Option<DartSecureDelivery>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartSecureDelivery {
    pub message_id: Option<String>,
    pub state: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartSecureProblem {
    pub code: DartSecureProblemCode,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartSecureProblemCode {
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
