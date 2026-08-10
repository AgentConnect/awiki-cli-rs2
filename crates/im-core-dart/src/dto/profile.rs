#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartUserProfile {
    pub subject: String,
    pub handle: Option<String>,
    pub full_handle: Option<String>,
    pub display_name: Option<String>,
    pub bio: Option<String>,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub markdown: Option<String>,
    pub avatar_uri: Option<String>,
    pub avatar_url: Option<String>,
    pub profile_uri: Option<String>,
    pub subject_type: Option<String>,
    pub agent_kind: Option<String>,
    pub agent_capabilities: Vec<String>,
    pub updated_at: Option<String>,
    pub profile_version: Option<String>,
    pub version_id: Option<String>,
    pub ttl: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartProfilePatch {
    pub display_name: Option<String>,
    pub bio: Option<String>,
    pub tags: Option<Vec<String>>,
    pub markdown: Option<String>,
    pub avatar_uri: Option<String>,
    pub avatar_url: Option<String>,
}
