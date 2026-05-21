use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryResolution {
    pub input: String,
    pub did: crate::ids::Did,
    pub handle: Option<crate::ids::Handle>,
    pub profile: Option<crate::identity::Profile>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandleLookupResult {
    pub handle: crate::ids::Handle,
    pub did: crate::ids::Did,
    pub domain: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contact {
    pub did: crate::ids::Did,
    pub handle: Option<crate::ids::Handle>,
    pub display_name: Option<String>,
    pub relationship: Option<String>,
    pub followed: bool,
    pub messaged: bool,
    pub note: Option<String>,
    pub last_seen_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationStatus {
    pub peer: crate::ids::PeerRef,
    pub did: Option<crate::ids::Did>,
    pub is_contact: bool,
    pub followed: bool,
    pub messaged: bool,
    pub relationship: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveContactRequest {
    pub peer: crate::ids::PeerRef,
    pub did: Option<crate::ids::Did>,
    pub handle: Option<crate::ids::Handle>,
    pub display_name: Option<String>,
    pub relationship: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ContactListQuery {
    pub limit: Option<crate::ids::PageLimit>,
}
