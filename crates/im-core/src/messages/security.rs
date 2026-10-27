use super::{MessageSecurityMode, MessageTarget};

/// Resolve the immutable group policy before persisting or uploading content.
/// Direct intent is never inferred from capability availability.
pub(crate) fn resolve(
    client: &crate::core::ImClient,
    target: &MessageTarget,
    requested: MessageSecurityMode,
) -> crate::ImResult<MessageSecurityMode> {
    let MessageTarget::Group(group) = target else {
        return Ok(requested);
    };
    preflight(client, &requested)?;
    #[cfg(feature = "group-e2ee")]
    if let Some(authoritative) =
        crate::internal::group_e2ee::send_state::prepared_snapshot(client, group)?
    {
        return group_mode(
            requested,
            crate::groups::authoritative_group_e2ee_classification(group.as_str(), &authoritative)?,
        );
    }
    #[cfg(feature = "group-e2ee")]
    if crate::internal::group_e2ee::send_state::has_observation(client, group) {
        // This authenticated observation inventory contains encrypted groups
        // only. It selects the immutable route, never grants send readiness.
        return group_mode(requested, true);
    }
    // A locally known immutable encrypted policy can only select the stronger
    // route. Never use a cached plain policy to downgrade a remote encrypted group.
    if crate::internal::group_runtime::cache::cached_group_snapshot(client, group.as_str())?
        .as_ref()
        .is_some_and(crate::internal::group_runtime::cache::group_snapshot_uses_e2ee)
    {
        return group_mode(requested, true);
    }
    let authoritative = client.groups().get_authoritative_group(group.clone())?;
    group_mode(
        requested,
        crate::groups::authoritative_group_e2ee_classification(group.as_str(), &authoritative)?,
    )
}

pub(crate) async fn resolve_async(
    client: &crate::core::ImClient,
    target: &MessageTarget,
    requested: MessageSecurityMode,
) -> crate::ImResult<MessageSecurityMode> {
    let MessageTarget::Group(group) = target else {
        return Ok(requested);
    };
    preflight(client, &requested)?;
    #[cfg(feature = "group-e2ee")]
    if let Some(authoritative) =
        crate::internal::group_e2ee::send_state::prepared_snapshot(client, group)?
    {
        return group_mode(
            requested,
            crate::groups::authoritative_group_e2ee_classification(group.as_str(), &authoritative)?,
        );
    }
    #[cfg(feature = "group-e2ee")]
    if crate::internal::group_e2ee::send_state::has_observation(client, group) {
        return group_mode(requested, true);
    }
    if crate::internal::group_runtime::cache::cached_group_snapshot_async(client, group.as_str())
        .await?
        .as_ref()
        .is_some_and(crate::internal::group_runtime::cache::group_snapshot_uses_e2ee)
    {
        return group_mode(requested, true);
    }
    let authoritative = client
        .groups()
        .get_authoritative_group_async(group.clone())
        .await?;
    group_mode(
        requested,
        crate::groups::authoritative_group_e2ee_classification(group.as_str(), &authoritative)?,
    )
}

/// A valid frozen operation already fixes its security route. This also permits
/// projection repair after the user left the group, without a new remote read.
pub(crate) fn resumes_group_operation(
    client: &crate::ImClient,
    request: &super::SendMessageRequest,
) -> crate::ImResult<bool> {
    if !matches!(&request.target, MessageTarget::Group(_)) {
        return Ok(false);
    }
    #[cfg(feature = "group-e2ee")]
    {
        if !client.core_inner().group_e2ee_v2_enabled()
            || matches!(
                crate::internal::identity_device_state::active_authorization_for_client(client),
                Err(crate::ImError::PermissionDenied)
            )
        {
            return Ok(false);
        }
        if matches!(&request.target, MessageTarget::Group(_))
            && matches!(
                request.security,
                MessageSecurityMode::DefaultPlain
                    | MessageSecurityMode::E2eeRequired
                    | MessageSecurityMode::GroupE2ee
            )
            && request.client_message_id.is_some()
            && request
                .delivery
                .idempotency_key
                .as_deref()
                .is_some_and(|key| !key.trim().is_empty())
        {
            preflight(client, &MessageSecurityMode::GroupE2ee)?;
            let mut frozen = request.clone();
            frozen.security = MessageSecurityMode::GroupE2ee;
            let journal =
                crate::internal::group_e2ee::send_journal::SendJournal::new(client, &frozen)?;
            return Ok(journal.load(client)?.is_some());
        }
    }
    let _ = (client, request);
    Ok(false)
}

fn group_mode(
    requested: MessageSecurityMode,
    encrypted: bool,
) -> crate::ImResult<MessageSecurityMode> {
    use MessageSecurityMode::*;
    match (requested, encrypted) {
        (DefaultPlain, true) | (E2eeRequired | GroupE2ee, true) => Ok(GroupE2ee),
        (DefaultPlain | Plain, false) => Ok(Plain),
        _ => Err(crate::ImError::InvalidInput {
            field: Some("security".to_owned()),
            message: "message security conflicts with the immutable group policy".to_owned(),
        }),
    }
}

#[cfg(test)]
#[path = "security_tests.rs"]
mod tests;

fn preflight(
    client: &crate::core::ImClient,
    requested: &MessageSecurityMode,
) -> crate::ImResult<()> {
    if matches!(
        requested,
        MessageSecurityMode::E2eeRequired | MessageSecurityMode::GroupE2ee
    ) {
        #[cfg(not(feature = "group-e2ee"))]
        return Err(crate::ImError::unsupported("group-e2ee"));
        #[cfg(feature = "group-e2ee")]
        {
            if !client.core_inner().group_e2ee_v2_enabled() {
                return Err(crate::ImError::unsupported("group-e2ee-v2"));
            }
            crate::internal::identity_device_state::active_authorization_for_client(client)?;
        }
    }
    Ok(())
}
