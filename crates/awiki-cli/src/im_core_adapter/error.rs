use crate::output::ExitError;

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
        im_core::ImError::LocalStateUnavailable { detail } => ExitError::new(
            "local_state_unavailable",
            5,
            format!("{context}: local state unavailable: {detail}"),
            "Check the workspace database path and permissions.",
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
        im_core::ImError::Service { message, .. } => ExitError::new(
            "service_error",
            5,
            format!("{context}: service error: {message}"),
            "Check the remote service response and retry if appropriate.",
        ),
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

pub fn map_identity_boundary_error(err: ExitError) -> ExitError {
    if err.detail.code == "identity_required"
        && err.detail.message.contains("default identity is missing")
    {
        return crate::app::identity_exit(
            crate::legacy_identity::IdentityError::NoDefaultIdentity(
                "identity not found: no active identity is configured".to_string(),
            ),
        );
    }
    err
}
