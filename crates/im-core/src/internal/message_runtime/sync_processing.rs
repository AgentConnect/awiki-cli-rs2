//! Business execution of one already received ordinary event.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::internal::local_state::{sync_inbox, sync_v2};
use crate::internal::transport::AsyncRpcTransport;
use crate::messages::{MessageProcessingStatus, MessageProcessingUpdate};

pub(crate) async fn process_ordinary<R: AsyncRpcTransport>(
    client: &crate::core::ImClient,
    claim: &sync_inbox::InputClaim,
    directory: &mut R,
) -> crate::ImResult<MessageProcessingUpdate> {
    let db = client.core_inner().local_state_db().await?;
    let checked = claim.clone();
    db.run_local(move |connection| sync_inbox::require_claim(connection, &checked, now()))
        .await?;
    let binding = client.active_sync_account_binding().await?;
    if binding.owner_identity_id != claim.owner_identity_id
        || binding.current_did != claim.owner_did
        || binding.account_id != claim.account_id
        || binding.protocol_device_id != claim.device_id
        || binding.device_auth_generation != claim.auth_generation
    {
        return Err(sync_inbox::error(
            "SYNC_INPUT_SUPERSEDED",
            "processing client no longer matches its claimed binding",
        ));
    }
    let event: crate::internal::wire::sync_v2::SyncEventV2 =
        serde_json::from_value(claim.payload.get("event").cloned().ok_or_else(|| {
            sync_inbox::error("SYNC_INPUT_INVALID", "ordinary envelope is missing")
        })?)
        .map_err(|_| {
            sync_inbox::error("SYNC_INPUT_INVALID", "ordinary envelope cannot be decoded")
        })?;
    let hydrated = claim
        .payload
        .get("hydrated")
        .filter(|value| !value.is_null());
    let notification = if event.event_type == "system.notification" {
        Some(
            super::sync_v2::prepare_system_notification(
                client,
                &binding,
                &event,
                hydrated.ok_or_else(|| {
                    sync_inbox::error("SYNC_INPUT_INVALID", "notification body is missing")
                })?,
                directory,
            )
            .await?,
        )
    } else {
        None
    };
    let mut public_messages = BTreeMap::new();
    let prepared = super::sync_v2::reduce_event(
        client,
        &event,
        hydrated,
        notification.clone(),
        &mut public_messages,
    )?;
    let dids = super::sync_v2::direct_peer_dids_from_events(std::slice::from_ref(&prepared));
    let mut warnings = Vec::new();
    super::sync_v2::resolve_unresolved_peers(
        client,
        &db,
        &binding,
        directory,
        dids.clone(),
        &mut warnings,
    )
    .await?;
    let unresolved = db
        .filter_unresolved_peer_dids(claim.owner_identity_id.clone(), dids)
        .await?;
    if !unresolved.is_empty() {
        return Err(sync_inbox::error(
            "sync.peer_resolution_pending",
            "message awaits its verified peer identity",
        ));
    }
    // Resolve canonical identities again after a newly verified Persona was
    // persisted. Both the public result and transaction use that same identity.
    public_messages.clear();
    let prepared =
        super::sync_v2::reduce_event(client, &event, hydrated, notification, &mut public_messages)?;
    let input = sync_v2::DeltaApplyInputV2 {
        owner_identity_id: claim.owner_identity_id.clone(),
        expected_run_generation: None,
        owner_did: claim.owner_did.clone(),
        account_id: claim.account_id.clone(),
        protocol_device_id: claim.device_id.clone(),
        device_auth_generation: claim.auth_generation.clone(),
        stream_epoch: claim.lane_epoch.clone(),
        next_scan_seq: claim.position.clone(),
        server_time: claim
            .payload
            .get("server_time")
            .and_then(Value::as_str)
            .unwrap_or(&event.occurred_at)
            .to_owned(),
        events: vec![prepared],
    };
    let committed_claim = claim.clone();
    let outcome = db
        .run_local(move |connection| {
            sync_inbox::apply_ordinary_claim(connection, &committed_claim, input, now())
        })
        .await?;
    // This is a post-commit local terminal marker, never a receive dependency.
    // Its authoritative Group state remains durable if a marker needs repair.
    let _ = super::sync_v2::apply_p4_terminal_events(client, std::slice::from_ref(&event));
    for notification in outcome.committed_system_notifications {
        client.emit_committed_system_notification(notification);
    }
    super::sync::emit_committed_sync_invalidation(client, &outcome.invalidation);
    let mut incoming = Vec::new();
    if claim.payload.get("source").and_then(Value::as_str) != Some("snapshot") {
        for event_id in &outcome.projected_message_event_ids {
            if let Some(message) = public_messages.get(event_id) {
                if message.direction == crate::messages::MessageDirection::Incoming {
                    incoming.push(crate::messages::CommittedIncomingMessage {
                        event_id: event_id.clone(),
                        logical_message_id: message.id.as_str().to_owned(),
                        source: "live_delta".to_owned(),
                        direction: message.direction.clone(),
                        message: message.clone(),
                    });
                }
            }
        }
    }
    Ok(MessageProcessingUpdate {
        event_id: claim.event_id.clone(),
        status: MessageProcessingStatus::Applied,
        changed_conversation_ids: outcome.invalidation.conversation_ids,
        committed_incoming_messages: incoming,
        error_code: None,
    })
}

pub(crate) fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

#[cfg(feature = "group-e2ee")]
pub(crate) async fn process_p6(
    client: &crate::core::ImClient,
    claim: &sync_inbox::InputClaim,
) -> crate::ImResult<MessageProcessingUpdate> {
    if !client.core_inner().group_e2ee_v2_enabled() {
        return Err(crate::ImError::unsupported("group-e2ee-v2"));
    }
    enum Prepared {
        Message(super::read::PreparedP6Incoming),
        Notice(crate::internal::group_e2ee::v2_notice::PreparedP6Notice),
    }
    let prepared = match claim.event_type.as_str() {
        "p6.delivery.created" => Prepared::Message(
            super::read::prepare_p6_v2_incoming_message(client, &claim.payload).await?,
        ),
        "p6.control.notice" => Prepared::Notice(
            crate::internal::group_e2ee::v2_notice::prepare_for_client_async(
                client,
                &claim.payload,
            )
            .await?,
        ),
        _ => {
            return Err(sync_inbox::error(
                "SYNC_INPUT_INVALID",
                "unsupported P6 input",
            ))
        }
    };
    let cleanup_runtime = match &prepared {
        Prepared::Message(message) => Some(message.runtime.clone()),
        _ => None,
    };
    let db = client.core_inner().local_state_db().await?;
    let committed_claim = claim.clone();
    let committing_client = client.clone();
    let changed_conversations = db.run_local(move |connection| {
        let transaction = rusqlite::Transaction::new_unchecked(connection, rusqlite::TransactionBehavior::Immediate)
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        sync_inbox::require_claim(&transaction, &committed_claim, now())?;
        let mut conversations = Vec::new();
        match prepared {
            Prepared::Message(message) => {
                let mut projected = super::read::project_prepared_p6_incoming(&committing_client, message, true)?;
                let manifest = super::read::attachment_manifest_cache_record(&committing_client, &projected);
                super::read::redact_attachment_manifests_for_public_projection(std::slice::from_mut(&mut projected));
                let message = super::read::message_from_value(&committing_client, &projected, None)?
                    .ok_or_else(|| sync_inbox::error("SYNC_P6_APPLICATION_INCOMPLETE", "P6 did not produce one message"))?;
                let records = super::read::remote_projection_records(&committing_client, &[message], &super::read::DirectP5ProjectionProvenance::default())?;
                for record in records {
                    let record = crate::internal::local_state::inbound_resolution_backlog::canonicalize_inbound_message(&transaction, record)?;
                    conversations.push(record.conversation_id.clone());
                    crate::internal::local_state::messages::upsert_message(&transaction, &record)?;
                }
                if let Some(manifest) = manifest {
                    crate::internal::local_state::attachment_manifest_cache::upsert_attachment_manifest_cache(&transaction, &manifest)?;
                }
            }
            Prepared::Notice(notice) => { notice.apply(true)?; }
        }
        sync_inbox::complete_claim(&transaction, &committed_claim, now())?;
        transaction.commit().map_err(crate::internal::local_state::local_state_unavailable)?;
        Ok(conversations)
    }).await?;
    if let (Some(runtime), Some(message_id), Some(group_did)) = (
        cleanup_runtime,
        claim
            .payload
            .pointer("/meta/message_id")
            .and_then(Value::as_str),
        claim.group_did.as_deref(),
    ) {
        let _ = runtime
            .with_nonblocking_operations()
            .forget_received_decryption(message_id, group_did);
    }
    client.emit_committed_local_message_projection("sync_p6_processed");
    Ok(MessageProcessingUpdate {
        event_id: claim.event_id.clone(),
        status: MessageProcessingStatus::Applied,
        changed_conversation_ids: changed_conversations,
        committed_incoming_messages: Vec::new(),
        error_code: None,
    })
}

#[cfg(not(feature = "group-e2ee"))]
pub(crate) async fn process_p6(
    _client: &crate::core::ImClient,
    _claim: &sync_inbox::InputClaim,
) -> crate::ImResult<MessageProcessingUpdate> {
    Err(crate::ImError::unsupported("group-e2ee"))
}

pub(crate) fn failure_code(error: &crate::ImError) -> String {
    match error {
        crate::ImError::MessageWireIdentityConflict { .. } => {
            "message_wire_identity_conflict".into()
        }
        crate::ImError::Service {
            code: Some(code), ..
        } => code.clone(),
        crate::ImError::TransportUnavailable { .. } => {
            "sync.processing_transport_unavailable".into()
        }
        crate::ImError::LocalStateUnavailable { .. } => {
            "sync.processing_storage_unavailable".into()
        }
        crate::ImError::PeerNotFound { .. } => "sync.peer_resolution_pending".into(),
        crate::ImError::GroupNotFound { .. } => "sync.group_state_pending".into(),
        crate::ImError::AuthRequired | crate::ImError::SessionExpired => {
            "sync.processing_auth_required".into()
        }
        crate::ImError::UnsupportedCapability { .. } => "sync.processing_upgrade_required".into(),
        _ => "sync.processing_blocked".into(),
    }
}

pub(crate) fn retry_at(error: &crate::ImError, attempts: i64) -> Option<i64> {
    let code = failure_code(error);
    (matches!(
        error,
        crate::ImError::TransportUnavailable { .. }
            | crate::ImError::LocalStateUnavailable { .. }
            | crate::ImError::PeerNotFound { .. }
            | crate::ImError::GroupNotFound { .. }
            | crate::ImError::AuthRequired
            | crate::ImError::SessionExpired
    ) || matches!(
        code.as_str(),
        "sync.peer_resolution_pending"
            | "sync.group_state_pending"
            | "group.e2ee.epoch_conflict"
            | "sync.processing_timeout"
    ))
    .then(|| now().saturating_add((1_i64 << attempts.clamp(0, 6)).min(60)))
}
