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
    let authoritative = client
        .groups()
        .get_authoritative_group_async(group.clone())
        .await?;
    group_mode(
        requested,
        crate::groups::authoritative_group_e2ee_classification(group.as_str(), &authoritative)?,
    )
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
