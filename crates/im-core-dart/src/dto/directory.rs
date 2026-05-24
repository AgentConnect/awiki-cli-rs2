#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DartIdentitySubject {
    Did { did: String },
    Handle { handle: String },
    Any { value: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartDirectoryResolution {
    pub input: String,
    pub did: String,
    pub handle: Option<String>,
    pub profile: Option<crate::dto::profile::DartUserProfile>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartRelationStatus {
    pub peer: String,
    pub relationship: Option<String>,
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartRelationshipListItem {
    pub did: String,
    pub handle: Option<String>,
    pub display_name: Option<String>,
    pub relationship: String,
    pub created_at: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartRelationshipPage {
    pub items: Vec<DartRelationshipListItem>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}
