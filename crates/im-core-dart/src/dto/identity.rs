#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DartIdentitySelector {
    Default,
    Id { id: String },
    Did { did: String },
    Handle { handle: String },
    LocalAlias { alias: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartIdentitySummary {
    pub id: String,
    pub did: String,
    pub handle: Option<String>,
    pub display_name: Option<String>,
    pub local_alias: Option<String>,
    pub device_id: Option<String>,
    pub is_default: bool,
    pub ready_for_auth: bool,
    pub ready_for_messaging: bool,
    pub missing: Vec<String>,
}
