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
pub enum IdentitySubject {
    Did(crate::ids::Did),
    Handle(crate::ids::Handle),
    Any(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicProfile {
    pub subject: IdentitySubject,
    pub did: crate::ids::Did,
    pub handle: Option<crate::ids::Handle>,
    pub profile: crate::identity::Profile,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FollowRequest {
    pub peer: crate::ids::PeerRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnfollowRequest {
    pub peer: crate::ids::PeerRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FollowResult {
    pub peer: crate::ids::PeerRef,
    pub did: crate::ids::Did,
    pub is_friend: bool,
    pub relation: RelationshipStatus,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnfollowResult {
    pub peer: crate::ids::PeerRef,
    pub did: crate::ids::Did,
    pub ok: bool,
    pub relation: RelationshipStatus,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationshipStatus {
    pub peer: crate::ids::PeerRef,
    pub did: crate::ids::Did,
    pub is_following: bool,
    pub is_follower: bool,
    pub is_friend: bool,
    pub is_blocked: bool,
    pub is_blocked_by: bool,
    pub is_contact: bool,
    pub messaged: bool,
    pub relationship: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RelationshipListQuery {
    pub limit: Option<crate::ids::PageLimit>,
    pub offset: Option<u32>,
    pub hydrate_profiles: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationshipListItem {
    pub did: Option<crate::ids::Did>,
    pub handle: Option<crate::ids::Handle>,
    pub profile: Option<crate::identity::Profile>,
    pub created_at: Option<String>,
    pub warnings: Vec<String>,
}
