#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartGroupSummary {
    pub id: Option<String>,
    pub did: String,
    pub name: Option<String>,
    pub membership_status: Option<String>,
    pub member_count: Option<u32>,
    pub last_message_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartGroupSnapshot {
    pub id: Option<String>,
    pub did: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub my_role: Option<String>,
    pub membership_status: Option<String>,
    pub member_count: Option<u32>,
    pub last_message_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartGroupMember {
    pub did: Option<String>,
    pub handle: Option<String>,
    pub role: Option<String>,
    pub status: Option<String>,
    pub joined_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartCreateGroupRequest {
    pub name: String,
    pub description: Option<String>,
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
pub struct DartGroupReadResult {
    pub group: Option<DartGroupSnapshot>,
    pub groups: Vec<DartGroupSummary>,
    pub members: Vec<DartGroupMember>,
    pub messages: crate::dto::message::DartMessagePage,
    pub total: Option<u32>,
    pub source: Option<String>,
    pub warnings: Vec<String>,
}
