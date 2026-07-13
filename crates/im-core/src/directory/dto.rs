use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryResolution {
    pub input: String,
    pub did: crate::ids::Did,
    pub handle: Option<crate::ids::Handle>,
    pub conversation_id: String,
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
    pub user_id: String,
    pub domain: Option<String>,
    pub status: Option<String>,
    pub binding_generation: Option<String>,
    pub profile: Option<crate::identity::Profile>,
    pub warnings: Vec<String>,
}

impl HandleLookupResult {
    pub fn direct_conversation_id(&self) -> String {
        crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(
            &crate::internal::local_state::owner_scope::DirectPeerScope::new(
                self.user_id.clone(),
                self.handle.as_str(),
            )
            .expect("validated handle lookup must define a direct peer scope"),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contact {
    pub did: crate::ids::Did,
    pub handle: Option<crate::ids::Handle>,
    pub display_name: Option<String>,
    pub avatar_uri: Option<String>,
    pub avatar_url: Option<String>,
    pub profile_uri: Option<String>,
    pub subject_type: Option<String>,
    pub relationship: Option<String>,
    pub followed: bool,
    pub messaged: bool,
    pub note: Option<String>,
    pub last_seen_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayProfile {
    pub did: Option<crate::ids::Did>,
    pub handle: Option<crate::ids::Handle>,
    pub display_name: Option<String>,
    pub avatar_uri: Option<String>,
    pub avatar_url: Option<String>,
    pub profile_uri: Option<String>,
    pub subject_type: Option<String>,
    pub cache_hit: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DisplayProfileBatchRequest {
    pub peers: Vec<crate::ids::PeerRef>,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(did: &str) -> HandleLookupResult {
        HandleLookupResult {
            handle: crate::ids::Handle::parse("Alice.Awiki.Info", "").unwrap(),
            did: crate::ids::Did::parse(did).unwrap(),
            user_id: "user-alice".to_owned(),
            domain: Some("awiki.info".to_owned()),
            status: Some("active".to_owned()),
            binding_generation: Some("2".to_owned()),
            profile: None,
            warnings: Vec::new(),
        }
    }

    #[test]
    fn handle_lookup_exposes_peer_scope_conversation_id_stable_across_did_rotation() {
        let first = lookup("did:example:alice:old");
        let rotated = lookup("did:example:alice:new");

        assert_eq!(
            first.direct_conversation_id(),
            rotated.direct_conversation_id()
        );
        assert!(first
            .direct_conversation_id()
            .starts_with("dm:peer-scope:v1:"));
    }
}
