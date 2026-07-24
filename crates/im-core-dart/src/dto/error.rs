#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartDeviceRevokeOutcomeCategory {
    CancelledBeforeSubmit,
    RejectedBeforeCommit,
    OutcomeUnknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartImError {
    pub code: String,
    pub message: String,
    pub field: Option<String>,
    pub status_code: Option<u16>,
    pub capability: Option<String>,
    pub service_code: Option<String>,
    pub service_data_json: Option<String>,
    pub device_revoke_outcome_category: Option<DartDeviceRevokeOutcomeCategory>,
}

impl DartImError {
    pub fn invalid_input(field: Option<String>, message: impl Into<String>) -> Self {
        Self {
            code: "invalid_input".to_string(),
            message: message.into(),
            field,
            status_code: None,
            capability: None,
            service_code: None,
            service_data_json: None,
            device_revoke_outcome_category: None,
        }
    }

    pub fn unsupported(capability: impl Into<String>) -> Self {
        let capability = capability.into();
        Self {
            code: "unsupported_capability".to_string(),
            message: format!("unsupported capability: {capability}"),
            field: None,
            status_code: None,
            capability: Some(capability),
            service_code: None,
            service_data_json: None,
            device_revoke_outcome_category: None,
        }
    }

    pub fn object_closed(object: impl Into<String>) -> Self {
        let object = object.into();
        Self {
            code: "object_closed".to_string(),
            message: format!("{object} has been disposed"),
            field: None,
            status_code: None,
            capability: None,
            service_code: None,
            service_data_json: None,
            device_revoke_outcome_category: None,
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            code: "internal_error".to_string(),
            message: message.into(),
            field: None,
            status_code: None,
            capability: None,
            service_code: None,
            service_data_json: None,
            device_revoke_outcome_category: None,
        }
    }
}

impl From<im_core::ImError> for DartImError {
    fn from(value: im_core::ImError) -> Self {
        match value {
            im_core::ImError::InvalidInput { field, message } => {
                Self::invalid_input(field, message)
            }
            im_core::ImError::IdentityRequired => Self::simple("identity_required", value),
            im_core::ImError::IdentityNotFound { selector } => Self::simple(
                "identity_not_found",
                format!("identity not found for selector {selector}"),
            ),
            im_core::ImError::DefaultIdentityMissing => {
                Self::simple("default_identity_missing", value)
            }
            im_core::ImError::IdentityNotReady { identity, missing } => Self::simple(
                "identity_not_ready",
                format!("identity {identity} is not ready: {}", missing.join(", ")),
            ),
            im_core::ImError::IdentityVault { failure } => Self::simple(
                failure.code(),
                format!("identity vault failure: {}", failure.code()),
            ),
            im_core::ImError::AuthRequired => Self::simple("auth_required", value),
            im_core::ImError::SessionExpired => Self::simple("session_expired", value),
            im_core::ImError::PermissionDenied => Self::simple("permission_denied", value),
            im_core::ImError::PeerNotFound { peer } => {
                Self::simple("peer_not_found", format!("peer not found: {peer}"))
            }
            im_core::ImError::GroupNotFound { group } => {
                Self::simple("group_not_found", format!("group not found: {group}"))
            }
            im_core::ImError::CursorInvalid => {
                Self::group_inventory("group.local_cursor_invalid", false)
            }
            im_core::ImError::CursorStale => {
                Self::group_inventory("group.local_cursor_stale", true)
            }
            im_core::ImError::InventoryIncomplete => {
                Self::group_inventory("group.local_inventory_incomplete", true)
            }
            im_core::ImError::InventoryTooLarge => {
                Self::group_inventory("group.local_inventory_too_large", false)
            }
            im_core::ImError::DeviceRevokeOutcome { category } => {
                let category = match category {
                    im_core::DeviceRevokeOutcomeCategory::CancelledBeforeSubmit => {
                        DartDeviceRevokeOutcomeCategory::CancelledBeforeSubmit
                    }
                    im_core::DeviceRevokeOutcomeCategory::RejectedBeforeCommit => {
                        DartDeviceRevokeOutcomeCategory::RejectedBeforeCommit
                    }
                    im_core::DeviceRevokeOutcomeCategory::OutcomeUnknown => {
                        DartDeviceRevokeOutcomeCategory::OutcomeUnknown
                    }
                };
                Self {
                    code: "device_revoke_outcome".to_owned(),
                    message: "device revoke did not return a confirmed result".to_owned(),
                    field: None,
                    status_code: None,
                    capability: None,
                    service_code: None,
                    service_data_json: None,
                    device_revoke_outcome_category: Some(category),
                }
            }
            im_core::ImError::MessageNotFound { message_id } => Self::simple(
                "message_not_found",
                format!("message not found: {message_id}"),
            ),
            im_core::ImError::TransportUnavailable { detail } => Self::simple(
                "transport_unavailable",
                format!("transport unavailable: {detail}"),
            ),
            im_core::ImError::UnsupportedCapability { capability } => Self::unsupported(capability),
            im_core::ImError::LocalStateUnavailable { detail } => Self::simple(
                "local_state_unavailable",
                format!("local state unavailable: {detail}"),
            ),
            im_core::ImError::LocalStateUpgradeRequired {
                from_version,
                target_version,
            } => Self::simple(
                "local_state_upgrade_required",
                format!(
                    "local state upgrade required: schema {from_version} -> {target_version}"
                ),
            ),
            im_core::ImError::LocalStateUpgradeInProgress => {
                Self::simple("local_state_upgrade_in_progress", value)
            }
            im_core::ImError::LocalStateUpgradeFailed { phase, code } => Self::simple(
                "local_state_upgrade_failed",
                format!("local state upgrade failed during {phase}: {code}"),
            ),
            im_core::ImError::IdentityUnresolved { detail } => Self::simple(
                "identity_unresolved",
                format!("identity unresolved: {detail}"),
            ),
            im_core::ImError::IdentityBindingConflict { detail } => Self::simple(
                "identity_binding_conflict",
                format!("identity binding conflict: {detail}"),
            ),
            im_core::ImError::ConversationAliasConflict {
                alias,
                existing_target,
                requested_target,
            } => Self::simple(
                "conversation_alias_conflict",
                format!(
                    "conversation alias conflict for {alias}: existing target {existing_target}, requested target {requested_target}"
                ),
            ),
            im_core::ImError::MessageWireIdentityConflict { message_id } => Self::simple(
                "message_wire_identity_conflict",
                format!("message wire identity conflict: {message_id}"),
            ),
            im_core::ImError::CanonicalGroupIdentityMissing { group } => Self::simple(
                "canonical_group_identity_missing",
                format!("canonical group identity missing: {group}"),
            ),
            im_core::ImError::LocalProjectionUnavailable { detail } => Self::simple(
                "local_projection_unavailable",
                format!("local projection unavailable: {detail}"),
            ),
            im_core::ImError::PathUnavailable { path_kind, detail } => Self::simple(
                "path_unavailable",
                format!("{path_kind} path unavailable: {detail}"),
            ),
            im_core::ImError::CredentialFileUnreadable { path_kind, detail } => Self::simple(
                "credential_file_unreadable",
                format!("{path_kind} credential file unreadable: {detail}"),
            ),
            im_core::ImError::Service {
                status_code,
                code,
                message,
                data,
            } => Self {
                code: "service_error".to_string(),
                message,
                field: None,
                status_code,
                capability: None,
                service_code: code,
                service_data_json: data.map(|value| value.to_string()),
                device_revoke_outcome_category: None,
            },
            im_core::ImError::Serialization { detail } => Self::simple(
                "serialization_error",
                format!("serialization error: {detail}"),
            ),
            im_core::ImError::Io { detail } => {
                Self::simple("io_error", format!("io error: {detail}"))
            }
            im_core::ImError::Internal { message } => Self::simple("internal_error", message),
        }
    }
}

impl DartImError {
    fn group_inventory(service_code: &str, retryable: bool) -> Self {
        Self {
            code: "service_error".to_owned(),
            message: "group member inventory request failed".to_owned(),
            field: None,
            status_code: None,
            capability: None,
            service_code: Some(service_code.to_owned()),
            service_data_json: Some(format!(r#"{{"retryable":{retryable}}}"#)),
            device_revoke_outcome_category: None,
        }
    }

    fn simple(code: &str, message: impl ToString) -> Self {
        Self {
            code: code.to_string(),
            message: message.to_string(),
            field: None,
            status_code: None,
            capability: None,
            service_code: None,
            service_data_json: None,
            device_revoke_outcome_category: None,
        }
    }
}
