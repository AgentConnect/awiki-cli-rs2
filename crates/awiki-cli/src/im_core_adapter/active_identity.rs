use crate::config::Resolved;
use crate::identity::types::StoredIdentity;
use crate::identity::Manager;
use crate::im_core_adapter::message_result::MessageAdapterError;

pub(crate) fn require_active_identity(
    resolved: &Resolved,
    manager: &Manager,
    requested: &str,
) -> Result<StoredIdentity, MessageAdapterError> {
    let identity_name = if requested.trim().is_empty() {
        if resolved.active_identity.trim().is_empty() {
            manager.current().map_err(identity_error)?.identity_name
        } else {
            resolved.active_identity.clone()
        }
    } else {
        requested.trim().to_string()
    };
    let record = manager.load(&identity_name).map_err(identity_error)?;
    let user_state = crate::identity::store::evaluate_user_state(&record.user_id, &record.handle);
    if !user_state.ready_for_messaging {
        return Err(MessageAdapterError::IdentityRequired(format!(
            "identity {} requires user registration before messaging",
            record.identity_name
        )));
    }
    Ok(record)
}

fn identity_error(error: crate::identity::IdentityError) -> MessageAdapterError {
    MessageAdapterError::Identity(error.into())
}
