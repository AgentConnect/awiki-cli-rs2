use crate::cli_output::ExitError;

const PUBLIC_SERVICE_CODE_MAX_LEN: usize = 96;
const PUBLIC_SERVICE_CODE_NAMESPACES: &[&str] = &[
    "anp",
    "attachment",
    "awiki",
    "client",
    "device",
    "direct",
    "group",
    "identity",
    "inbox",
    "read_state",
    "sync",
];

pub fn map_im_error(err: im_core::ImError, context: &'static str) -> ExitError {
    match err {
        im_core::ImError::InvalidInput { message, .. } => ExitError::new(
            "invalid_argument",
            2,
            format!("{context}: {message}"),
            "Check the command arguments and try again.",
        ),
        im_core::ImError::IdentityRequired => ExitError::new(
            "identity_required",
            2,
            format!("{context}: identity is required."),
            "Pass --identity or run `awiki-cli id use <identity>`.",
        ),
        im_core::ImError::DefaultIdentityMissing => ExitError::new(
            "identity_required",
            2,
            format!("{context}: default identity is missing."),
            "Register an identity or run `awiki-cli id use <identity>` first.",
        ),
        im_core::ImError::IdentityNotFound { selector } => ExitError::new(
            "not_found",
            5,
            format!("{context}: identity not found: {selector}"),
            "Check --identity or run `awiki-cli id list`.",
        ),
        im_core::ImError::IdentityNotReady { identity, missing } => ExitError::new(
            "identity_not_ready",
            3,
            format!("{context}: identity {identity} is not ready: {}", missing.join(", ")),
            "Complete identity setup before using IM commands.",
        ),
        im_core::ImError::IdentityVault { failure } => ExitError::new(
            failure.code(),
            3,
            format!("{context}: identity vault verification failed."),
            "Check the configured vault root key and workspace/device context; do not replace an existing vault key.",
        ),
        im_core::ImError::AuthRequired | im_core::ImError::SessionExpired => ExitError::new(
            "auth_required",
            3,
            format!("{context}: authentication is required."),
            "Use an identity with valid DID key material, or run `awiki-cli id refresh-token` and try again.",
        ),
        im_core::ImError::PermissionDenied => ExitError::new(
            "permission_denied",
            4,
            format!("{context}: permission denied."),
            "Check identity permissions and service access.",
        ),
        im_core::ImError::PeerNotFound { peer } => ExitError::new(
            "not_found",
            5,
            format!("{context}: peer not found: {peer}"),
            "Check the peer handle or DID.",
        ),
        im_core::ImError::GroupNotFound { group } => ExitError::new(
            "not_found",
            5,
            format!("{context}: group not found: {group}"),
            "Check the group DID or id.",
        ),
        im_core::ImError::CursorInvalid => ExitError::new(
            "invalid_argument",
            2,
            format!("{context}: group page cursor is invalid."),
            "Use the opaque cursor returned by the previous page.",
        ),
        im_core::ImError::CursorStale | im_core::ImError::InventoryIncomplete => {
            let mut mapped = ExitError::new(
                "temporarily_unavailable",
                5,
                format!("{context}: the group member inventory changed or is incomplete."),
                "Retry the operation from the first page.",
            );
            mapped.detail.retryable = true;
            mapped
        }
        im_core::ImError::InventoryTooLarge => ExitError::new(
            "resource_limit",
            5,
            format!("{context}: the group member inventory exceeds its allowed limit."),
            "Check the authoritative group member policy before retrying.",
        ),
        im_core::ImError::DeviceRevokeOutcome { category } => {
            let (code, exit, message, hint) = match category {
                im_core::DeviceRevokeOutcomeCategory::CancelledBeforeSubmit => (
                    "cancelled_before_submit",
                    2,
                    "device revoke was cancelled before submission.",
                    "Confirm user presence before retrying.",
                ),
                im_core::DeviceRevokeOutcomeCategory::RejectedBeforeCommit => (
                    "rejected_before_commit",
                    4,
                    "device revoke was rejected before commit.",
                    "Refresh the device Registry and review the target before retrying.",
                ),
                im_core::DeviceRevokeOutcomeCategory::OutcomeUnknown => (
                    "outcome_unknown",
                    5,
                    "device revoke outcome is unknown.",
                    "Refresh the authoritative device Registry before deciding whether to retry.",
                ),
            };
            ExitError::new(code, exit, format!("{context}: {message}"), hint)
        }
        im_core::ImError::MessageNotFound { message_id } => ExitError::new(
            "not_found",
            5,
            format!("{context}: message not found: {message_id}"),
            "Check the message id.",
        ),
        im_core::ImError::UnsupportedCapability { capability } => ExitError::new(
            "unsupported_capability",
            2,
            format!("{context}: {capability} is not supported in the IM Core Phase 1 adapter."),
            "This capability is outside the Phase 1 SDK migration scope; use the existing legacy command path where available.",
        ),
        im_core::ImError::TransportUnavailable { detail } => ExitError::new(
            "transport_unavailable",
            5,
            format!("{context}: transport unavailable: {detail}"),
            "Check the service endpoint, runtime mode, and network connectivity.",
        ),
        im_core::ImError::LocalStateUnavailable { detail } if detail.contains("secret vault") => {
            ExitError::new(
                "vault_root_key_required",
                3,
                format!("{context}: {detail}"),
                crate::m_core_cli_adapter::vault::ROOT_KEY_HINT,
            )
        }
        im_core::ImError::LocalStateUnavailable { detail } => ExitError::new(
            "local_state_unavailable",
            5,
            format!("{context}: local state unavailable: {detail}"),
            "Check the workspace database path and permissions.",
        ),
        im_core::ImError::LocalStateUpgradeRequired {
            from_version,
            target_version,
        } => ExitError::new(
            "local_state_upgrade_required",
            5,
            format!(
                "{context}: local state upgrade required: schema {from_version} -> {target_version}."
            ),
            "Run the matching AWiki upgrade flow before retrying; do not open this state with an older binary.",
        ),
        im_core::ImError::LocalStateUpgradeInProgress => ExitError::new(
            "local_state_upgrade_in_progress",
            5,
            format!("{context}: local state upgrade is already in progress."),
            "Wait for the current upgrade process to finish, then retry.",
        ),
        im_core::ImError::LocalStateUpgradeFailed { phase, code } => ExitError::new(
            "local_state_upgrade_failed",
            5,
            format!("{context}: local state upgrade failed during {phase}: {code}."),
            "Keep the verified backup and retry with the matching AWiki recovery flow.",
        ),
        im_core::ImError::IdentityUnresolved { .. } => ExitError::new(
            "identity_unresolved",
            5,
            format!("{context}: identity could not be resolved to a canonical Persona."),
            "Refresh the authoritative Handle binding and retry.",
        ),
        im_core::ImError::IdentityBindingConflict { .. } => ExitError::new(
            "identity_binding_conflict",
            5,
            format!("{context}: authoritative identity bindings conflict."),
            "Resolve the Handle binding conflict before retrying.",
        ),
        im_core::ImError::ConversationAliasConflict { .. } => ExitError::new(
            "conversation_alias_conflict",
            5,
            format!("{context}: conversation alias has conflicting canonical targets."),
            "Run the canonical conversation diagnostics before retrying.",
        ),
        im_core::ImError::MessageWireIdentityConflict { .. } => ExitError::new(
            "message_wire_identity_conflict",
            5,
            format!("{context}: message wire identity conflicts with persisted state."),
            "Stop processing this state and run the canonical conversation diagnostics.",
        ),
        im_core::ImError::CanonicalGroupIdentityMissing { .. } => ExitError::new(
            "canonical_group_identity_missing",
            5,
            format!("{context}: canonical group identity is unavailable."),
            "Refresh the authoritative Group state before retrying.",
        ),
        im_core::ImError::LocalProjectionUnavailable { .. } => ExitError::new(
            "local_projection_unavailable",
            5,
            format!("{context}: canonical local projection is unavailable."),
            "Repair the local projection from authoritative Core state and retry.",
        ),
        im_core::ImError::PathUnavailable { path_kind, detail } => ExitError::new(
            "path_unavailable",
            5,
            format!("{context}: {path_kind} path unavailable: {detail}"),
            "Check workspace path configuration and permissions.",
        ),
        im_core::ImError::CredentialFileUnreadable { path_kind, detail } => ExitError::new(
            "credential_unreadable",
            5,
            format!("{context}: {path_kind} credential file unreadable: {detail}"),
            "Check identity file permissions.",
        ),
        im_core::ImError::Service {
            code: Some(code), ..
        } if code == "group.local_cursor_invalid" => {
            service_group_page_error(
                context,
                "invalid_argument",
                2,
                &code,
                "The group page cursor is invalid.",
            )
        }
        im_core::ImError::Service {
            code: Some(code), ..
        } if matches!(
            code.as_str(),
            "group.local_cursor_stale" | "group.local_inventory_incomplete"
        ) => service_group_page_error(
            context,
            "temporarily_unavailable",
            5,
            &code,
            "Retry the operation from the first page.",
        ),
        im_core::ImError::Service {
            code: Some(code), ..
        } if code == "group.local_inventory_too_large" => service_group_page_error(
            context,
            "resource_limit",
            5,
            &code,
            "The authoritative group inventory exceeds its allowed limit.",
        ),
        im_core::ImError::Service {
            status_code: Some(401),
            ..
        } => ExitError::new(
            "auth_required",
            3,
            format!("{context}: authentication is required."),
            "Refresh the selected identity credentials and try again.",
        ),
        im_core::ImError::Service {
            status_code: Some(403),
            ..
        } => ExitError::new(
            "permission_denied",
            4,
            format!("{context}: permission denied."),
            "Check identity permissions and service access.",
        ),
        im_core::ImError::Service { code, .. } => {
            let mut mapped = ExitError::new(
                "service_error",
                5,
                format!("{context}: remote service request failed."),
                "Retry if appropriate or inspect the stable service code.",
            );
            if let Some(service_code) = code.as_deref().filter(|code| is_public_service_code(code))
            {
                mapped.detail.details = serde_json::json!({"service_code": service_code});
            }
            mapped
        }
        im_core::ImError::Serialization { detail }
            if detail.contains(im_core::vault::IM_CORE_VAULT_ROOT_KEY_ENV) =>
        {
            ExitError::new(
                "vault_root_key_invalid",
                2,
                format!("{context}: {detail}"),
                crate::m_core_cli_adapter::vault::ROOT_KEY_HINT,
            )
        }
        im_core::ImError::Serialization { detail } => ExitError::new(
            "serialization_error",
            1,
            format!("{context}: serialization error: {detail}"),
            "Report this issue with the command output.",
        ),
        im_core::ImError::Io { detail } => ExitError::new(
            "io_error",
            1,
            format!("{context}: io error: {detail}"),
            "Check local filesystem access.",
        ),
        im_core::ImError::Internal { message } => ExitError::new(
            "internal",
            1,
            format!("{context}: internal error: {message}"),
            "Report this issue with the command output.",
        ),
    }
}

fn service_group_page_error(
    context: &'static str,
    category: &'static str,
    exit_code: i32,
    service_code: &str,
    hint: &'static str,
) -> ExitError {
    let mut mapped = ExitError::new(
        category,
        exit_code,
        format!("{context}: remote group inventory request failed."),
        hint,
    );
    mapped.detail.details = serde_json::json!({"service_code": service_code});
    mapped.detail.retryable = matches!(
        service_code,
        "group.local_cursor_stale" | "group.local_inventory_incomplete"
    );
    mapped
}

pub(crate) fn is_public_service_code(code: &str) -> bool {
    if code.is_empty() || code.len() > PUBLIC_SERVICE_CODE_MAX_LEN || !code.is_ascii() {
        return false;
    }
    if !code
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte))
    {
        return false;
    }

    let Some((namespace, suffix)) = code.split_once('.') else {
        return false;
    };
    PUBLIC_SERVICE_CODE_NAMESPACES.contains(&namespace)
        && !suffix.is_empty()
        && code.split('.').all(|segment| !segment.is_empty())
}

pub fn map_identity_boundary_error(err: ExitError) -> ExitError {
    if err.detail.code == "identity_required"
        && err.detail.message.contains("default identity is missing")
    {
        return ExitError::new(
            "not_found",
            5,
            "identity not found: no active identity is configured",
            "Run `awiki-cli id list` to inspect available identities.",
        );
    }
    err
}
