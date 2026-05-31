use super::records::ContactRecord;
use crate::internal::local_state::owner_scope::OwnerScope;

pub(crate) fn record_follow_applied(
    client: &crate::core::ImClient,
    target_did: &crate::ids::Did,
    target_handle: Option<&crate::ids::Handle>,
) -> crate::ImResult<()> {
    record_relationship_projection(
        client,
        target_did,
        target_handle,
        "following",
        true,
        "followed",
    )
}

pub(crate) fn record_unfollow_applied(
    client: &crate::core::ImClient,
    target_did: &crate::ids::Did,
    target_handle: Option<&crate::ids::Handle>,
) -> crate::ImResult<()> {
    record_relationship_projection(
        client,
        target_did,
        target_handle,
        "none",
        false,
        "unfollowed",
    )
}

fn record_relationship_projection(
    client: &crate::core::ImClient,
    target_did: &crate::ids::Did,
    target_handle: Option<&crate::ids::Handle>,
    relationship: &str,
    followed: bool,
    event_type: &str,
) -> crate::ImResult<()> {
    let mut connection = super::open_writable(client)?;
    let scope = OwnerScope::for_client(client)?;
    let credential_name = credential_name(&scope);
    let handle = target_handle
        .map(|handle| handle.as_str().to_string())
        .unwrap_or_default();
    super::records::upsert_contact(
        &mut connection,
        ContactRecord {
            owner_identity_id: scope.owner_identity_id.clone(),
            owner_did: scope.owner_did.clone(),
            did: target_did.as_str().to_string(),
            handle: handle.clone(),
            relationship: relationship.to_string(),
            followed: Some(followed),
            source_type: "directory.relationship".to_string(),
            credential_name: credential_name.clone(),
            ..ContactRecord::default()
        },
    )?;
    super::records::append_relationship_event(
        &connection,
        super::records::RelationshipEventRecord {
            owner_identity_id: scope.owner_identity_id,
            owner_did: scope.owner_did,
            target_did: target_did.as_str().to_string(),
            target_handle: handle,
            event_type: event_type.to_string(),
            source_type: "directory.relationship".to_string(),
            status: "applied".to_string(),
            credential_name,
            ..super::records::RelationshipEventRecord::default()
        },
    )?;
    Ok(())
}

fn credential_name(scope: &OwnerScope) -> String {
    scope
        .credential_name
        .clone()
        .unwrap_or_else(|| scope.owner_identity_id.clone())
}
