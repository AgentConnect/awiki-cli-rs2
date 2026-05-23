use super::records::ContactRecord;

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
    let handle = target_handle
        .map(|handle| handle.as_str().to_string())
        .unwrap_or_default();
    super::records::upsert_contact(
        &mut connection,
        ContactRecord {
            owner_identity_id: client.current_identity().id.as_str().to_string(),
            owner_did: client.did().as_str().to_string(),
            did: target_did.as_str().to_string(),
            handle: handle.clone(),
            relationship: relationship.to_string(),
            followed: Some(followed),
            source_type: "directory.relationship".to_string(),
            credential_name: client.current_identity().id.as_str().to_string(),
            ..ContactRecord::default()
        },
    )?;
    super::records::append_relationship_event(
        &connection,
        super::records::RelationshipEventRecord {
            owner_identity_id: client.current_identity().id.as_str().to_string(),
            owner_did: client.did().as_str().to_string(),
            target_did: target_did.as_str().to_string(),
            target_handle: handle,
            event_type: event_type.to_string(),
            source_type: "directory.relationship".to_string(),
            status: "applied".to_string(),
            credential_name: client.current_identity().id.as_str().to_string(),
            ..super::records::RelationshipEventRecord::default()
        },
    )?;
    Ok(())
}
