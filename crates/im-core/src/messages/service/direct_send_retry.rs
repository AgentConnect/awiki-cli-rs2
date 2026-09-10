//! Durable request identity for the target-first plain Direct compatibility API.
use super::ResolvedSendRequest;
#[cfg(any(feature = "blocking", test))]
use crate::internal::local_state::messages::direct_send_intent;
use crate::internal::local_state::messages::MessageRecord;

#[cfg(any(feature = "blocking", test))]
pub(super) fn prepare(
    client: &crate::core::ImClient,
    resolved: &mut ResolvedSendRequest,
) -> crate::ImResult<Option<String>> {
    if resolved.request.delegated_signing.is_some() {
        return Ok(None);
    }
    let record = pending_record(client, resolved)?;
    let mut connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    let intent = direct_send_intent::prepare(&mut connection, record)?;
    client.emit_committed_local_message_projection("local_send_pending");
    resolved.target_did = Some(intent.target_did);
    Ok(Some(intent.created_at))
}

#[cfg(not(any(feature = "blocking", test)))]
pub(super) fn prepare(
    _client: &crate::core::ImClient,
    _resolved: &mut ResolvedSendRequest,
) -> crate::ImResult<Option<String>> {
    Err(crate::ImError::unsupported("sync-message-projection"))
}

pub(super) async fn prepare_async(
    client: &crate::core::ImClient,
    resolved: &mut ResolvedSendRequest,
) -> crate::ImResult<Option<String>> {
    // Delegated messages have a distinct logical sender and retain their existing runtime.
    if resolved.request.delegated_signing.is_some() {
        return Ok(None);
    }
    let record = pending_record(client, resolved)?;
    let intent = client
        .core_inner()
        .local_state_db()
        .await?
        .prepare_direct_send(record)
        .await?;
    client.emit_committed_local_message_projection("local_send_pending");
    resolved.target_did = Some(intent.target_did);
    Ok(Some(intent.created_at))
}

fn pending_record(
    client: &crate::core::ImClient,
    resolved: &mut ResolvedSendRequest,
) -> crate::ImResult<MessageRecord> {
    normalize_identity(client.current_identity().id.as_str(), &mut resolved.request)?;
    let request = &resolved.request;
    let projection = crate::internal::message_runtime::local_projection::send_projection_result(
        client,
        &request.target,
        &request.body,
        request
            .client_message_id
            .as_ref()
            .expect("normalized message id"),
        request.delivery.idempotency_key.as_deref(),
        crate::messages::DeliveryState::StoredLocally,
        resolved.target_did.as_deref(),
        resolved.direct_handle(),
        resolved.peer_scope.as_ref(),
    )?;
    crate::internal::message_runtime::local_projection::send_projection_record(
        client,
        &request.target,
        &request.body,
        &projection,
        resolved.target_did.as_deref(),
        resolved.direct_handle(),
        resolved.peer_scope.as_ref(),
        Some(&crate::internal::wire::common::now_rfc3339()),
    )
}

fn normalize_identity(
    owner: &str,
    request: &mut crate::messages::SendMessageRequest,
) -> crate::ImResult<()> {
    use sha2::{Digest, Sha256};
    request.delivery.idempotency_key = request
        .delivery
        .idempotency_key
        .take()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    if request.client_message_id.is_none() {
        let suffix = match request.delivery.idempotency_key.as_deref() {
            Some(operation) => {
                let mut hash = Sha256::new();
                hash.update(b"awiki-direct-message-v1\0");
                hash.update((owner.len() as u64).to_be_bytes());
                hash.update(owner.as_bytes());
                hash.update(operation.as_bytes());
                format!("{:x}", hash.finalize())
            }
            None => crate::internal::wire::common::generate_operation_id(),
        };
        request.client_message_id = Some(crate::ids::MessageId::parse(format!("msg-{suffix}"))?);
    }
    if request.delivery.idempotency_key.is_none() {
        request.delivery.idempotency_key = Some(format!(
            "op-{}",
            request
                .client_message_id
                .as_ref()
                .expect("normalized message id")
                .as_str()
        ));
    }
    Ok(())
}
