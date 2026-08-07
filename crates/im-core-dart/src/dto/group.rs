#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartGroupSummary {
    pub conversation_id: String,
    pub id: Option<String>,
    pub did: String,
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub avatar_uri: Option<String>,
    pub my_role: Option<String>,
    pub membership_status: Option<String>,
    pub member_count: Option<u32>,
    pub last_message_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartGroupSnapshot {
    pub conversation_id: String,
    pub id: Option<String>,
    pub did: String,
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub avatar_uri: Option<String>,
    pub my_role: Option<String>,
    pub membership_status: Option<String>,
    pub member_count: Option<u32>,
    pub last_message_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartGroupMember {
    pub membership_id: Option<String>,
    pub peer_persona_id: Option<String>,
    pub did: Option<String>,
    pub credential_did: Option<String>,
    pub handle: Option<String>,
    pub role: Option<String>,
    pub status: Option<String>,
    pub joined_at: Option<String>,
    pub subject_type: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartGroupIdentityMode {
    Handle,
    DidOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartCreateGroupRequest {
    pub name: String,
    pub identity_mode: DartGroupIdentityMode,
    pub identity_handle: Option<String>,
    pub description: Option<String>,
    pub avatar_uri: Option<String>,
    pub discoverability: Option<String>,
    pub admission_mode: Option<String>,
    pub message_security_profile: Option<String>,
    pub e2ee: bool,
    pub slug: Option<String>,
    pub goal: Option<String>,
    pub rules: Option<String>,
    pub message_prompt: Option<String>,
    pub doc_url: Option<String>,
    pub attachments_allowed: Option<bool>,
    pub max_members: Option<String>,
    pub member_max_messages: Option<i64>,
    pub member_max_total_chars: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartJoinGroupRequest {
    pub group_did: String,
    pub identity_mode: DartGroupIdentityMode,
    pub identity_handle: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartGroupRebindRecoveryItem {
    pub group_did: String,
    pub layer: String,
    pub phase: String,
    pub blocked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartGroupRebindRecoverySummary {
    pub processed: u32,
    pub completed: u32,
    pub pending: u32,
    pub blocked: u32,
    pub send_paused_group_dids: Vec<String>,
    pub items: Vec<DartGroupRebindRecoveryItem>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartGroupReadResult {
    pub group: Option<DartGroupSnapshot>,
    pub groups: Vec<DartGroupSummary>,
    pub members: Vec<DartGroupMember>,
    pub messages: crate::dto::message::DartMessagePage,
    pub total: Option<u32>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
    pub page_group_did: Option<String>,
    pub group_state_version: Option<String>,
    pub source: Option<String>,
    pub warnings: Vec<String>,
}
