//! High-level intents for the existing tenant listener. Only the listener's
//! Core encrypts; once an intent is submitted there is no local-writer fallback.
use super::*;
use crate::host_runtime::bridge::{self, BridgeCallError, BridgeRequest};
use im_core::prelude::{
    AttachmentInput, AttachmentSendRequest, MessageId, MessageTarget, SendMessageRequest,
};

fn available(resolved: &Resolved) -> bool {
    crate::host_runtime::resolve(resolved).mode == bridge::MODE_WEBSOCKET
        && bridge::bridge_endpoint_available(&bridge::resolved_bridge_endpoint(resolved))
}

fn ids(message: &mut Option<MessageId>, operation: &mut Option<String>) -> Result<(), ExitError> {
    if message.is_none() {
        *message = Some(
            MessageId::parse(format!(
                "msg-{}",
                im_core::realtime::wire::generate_operation_id()
            ))
            .map_err(|error| internal_anyhow(error.into()))?,
        );
    }
    if operation
        .as_deref()
        .is_none_or(|value| value.trim().is_empty())
    {
        *operation = Some(format!(
            "op-{}",
            message.as_ref().expect("assigned id").as_str()
        ));
    }
    Ok(())
}

pub(super) fn message(
    resolved: &Resolved,
    identity: &str,
    request: &SendMessageRequest,
) -> Result<Option<CommandResult>, ExitError> {
    if !available(resolved) {
        return Ok(None);
    }
    let mut request = request.clone();
    ids(
        &mut request.client_message_id,
        &mut request.delivery.idempotency_key,
    )?;
    let recovery = json!({"client_message_id":request.client_message_id,"idempotency_key":request.delivery.idempotency_key});
    invoke(
        resolved,
        identity,
        "local.send_message",
        Map::from_iter([("request".into(), json!(request))]),
        recovery,
    )
}

pub(super) async fn message_async(
    resolved: &Resolved,
    identity: &str,
    request: &SendMessageRequest,
) -> Result<Option<CommandResult>, ExitError> {
    let (resolved, identity, request) = (resolved.clone(), identity.to_owned(), request.clone());
    tokio::task::spawn_blocking(move || message(&resolved, &identity, &request))
        .await
        .map_err(|error| internal_anyhow(error.into()))?
}

pub(super) fn attachment(
    resolved: &Resolved,
    identity: &str,
    target: &MessageTarget,
    request: &AttachmentSendRequest,
    message: Option<MessageId>,
) -> Result<Option<CommandResult>, ExitError> {
    if !available(resolved) {
        return Ok(None);
    }
    let mut request = request.clone();
    let mut message = message;
    ids(&mut message, &mut request.delivery.idempotency_key)?;
    if let AttachmentInput::LocalFile(path) = &mut request.input {
        if path.is_relative() {
            *path = std::env::current_dir()
                .map_err(|error| internal_anyhow(error.into()))?
                .join(&*path);
        }
    }
    let recovery =
        json!({"client_message_id":message,"idempotency_key":request.delivery.idempotency_key});
    invoke(
        resolved,
        identity,
        "local.send_attachment",
        json!({
            "target":target, "request":request, "client_message_id":message,
        })
        .as_object()
        .expect("attachment object")
        .clone(),
        recovery,
    )
}

pub(super) async fn attachment_async(
    resolved: &Resolved,
    identity: &str,
    target: &MessageTarget,
    request: &AttachmentSendRequest,
    message: Option<MessageId>,
) -> Result<Option<CommandResult>, ExitError> {
    let (resolved, identity, target, request) = (
        resolved.clone(),
        identity.to_owned(),
        target.clone(),
        request.clone(),
    );
    tokio::task::spawn_blocking(move || {
        attachment(&resolved, &identity, &target, &request, message)
    })
    .await
    .map_err(|error| internal_anyhow(error.into()))?
}

fn invoke(
    resolved: &Resolved,
    identity: &str,
    method: &str,
    params: Map<String, Value>,
    recovery: Value,
) -> Result<Option<CommandResult>, ExitError> {
    invoke_with(
        &bridge::send_workspace_binding(resolved),
        &bridge_identity_name(resolved, identity),
        method,
        params,
        recovery,
        |request| bridge::call_local_bridge(request, resolved),
    )
}

fn invoke_with(
    binding: &str,
    identity: &str,
    method: &str,
    mut params: Map<String, Value>,
    recovery: Value,
    mut call: impl FnMut(BridgeRequest) -> anyhow::Result<Map<String, Value>>,
) -> Result<Option<CommandResult>, ExitError> {
    // Read-only negotiation precedes any business submission. A legacy or
    // unavailable listener can still use the original standalone path.
    let capabilities = match call(BridgeRequest {
        method: "local.capabilities".into(),
        params: Map::new(),
        identity_name: identity.into(),
    }) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    if capabilities.get("schema_version").and_then(Value::as_u64) != Some(1)
        || !capabilities
            .get("methods")
            .and_then(Value::as_array)
            .is_some_and(|methods| methods.iter().any(|item| item.as_str() == Some(method)))
    {
        return Ok(None);
    }
    if capabilities
        .get("workspace_binding")
        .and_then(Value::as_str)
        != Some(binding)
    {
        return Err(ExitError::new(
            "identity_binding_conflict",
            5,
            "The listener belongs to a different tenant or local state.",
            "Select the listener for the current workspace.",
        ));
    }
    params.insert("workspace_binding".into(), Value::String(binding.into()));
    let request = BridgeRequest {
        method: method.into(),
        params,
        identity_name: identity.into(),
    };
    let mut outcome = call(request.clone());
    if outcome.as_ref().err().is_some_and(|error| {
        error
            .downcast_ref::<BridgeCallError>()
            .is_some_and(|error| {
                matches!(error.phase.as_str(), "bridge_read" | "bridge_write")
                    && !error.cause.is_empty()
            })
    }) {
        // One bounded repeat of the exact high-level intent. P5/P6 recovery in
        // the same Core owns acceptance lookup and frozen ciphertext reuse.
        outcome = call(request);
    }
    let mut result = outcome.map_err(|error| {
        let mut mapped = listener_local_command_error(method, &error);
        mapped.detail.retryable = true;
        mapped.detail.details = recovery.clone();
        mapped.detail.hint = "Retry the same recipient and content using the original client_message_id and idempotency_key in details; do not start a new send with new IDs.".into();
        mapped
    })?;
    if let Some(error) = result.remove("command_error") {
        let mut error = decode_command_error(error)?;
        if error.detail.details.is_null() {
            error.detail.details = recovery;
        } else if let Some(details) = error.detail.details.as_object_mut() {
            details.insert("original_send".into(), recovery);
        }
        return Err(error);
    }
    parse_listener_local_result(result).map(Some)
}

fn decode_command_error(value: Value) -> Result<ExitError, ExitError> {
    #[derive(serde::Deserialize)]
    struct RemoteError {
        exit_code: i32,
        error: Detail,
    }
    #[derive(serde::Deserialize)]
    struct Detail {
        code: String,
        message: String,
        #[serde(default)]
        hint: String,
        retryable: bool,
        #[serde(default)]
        details: Value,
    }
    let remote: RemoteError =
        serde_json::from_value(value).map_err(|error| internal_anyhow(error.into()))?;
    let mut error = ExitError::new(
        &remote.error.code,
        remote.exit_code,
        remote.error.message,
        remote.error.hint,
    );
    error.detail.retryable = remote.error.retryable;
    error.detail.details = remote.error.details;
    Ok(error)
}

#[cfg(test)]
#[path = "listener_send_tests.rs"]
mod tests;
