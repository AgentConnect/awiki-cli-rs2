use serde_json::{json, Map, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QueuedSecureOutboxRow {
    pub(crate) outbox_id: String,
    pub(crate) peer_did: String,
    pub(crate) original_type: String,
    pub(crate) plaintext: String,
    pub(crate) created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SecureOutboxSendOutcome {
    Success {
        message_id: String,
        operation_id: String,
        delivery_state: String,
        accepted_at: String,
    },
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MarkSentOutcome {
    Success,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StoreMessageOutcome {
    Success,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SecureOutboxFlushRowOutcome {
    pub(crate) send: SecureOutboxSendOutcome,
    pub(crate) session_id: String,
    pub(crate) mark_sent: MarkSentOutcome,
    pub(crate) store_message: StoreMessageOutcome,
}

impl Default for SecureOutboxFlushRowOutcome {
    fn default() -> Self {
        Self {
            send: SecureOutboxSendOutcome::Success {
                message_id: String::new(),
                operation_id: String::new(),
                delivery_state: String::new(),
                accepted_at: String::new(),
            },
            session_id: String::new(),
            mark_sent: MarkSentOutcome::Success,
            store_message: StoreMessageOutcome::Success,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SecureOutboxFlushAction {
    SendText {
        outbox_id: String,
        target_did: String,
        plaintext: String,
    },
    SendJson {
        outbox_id: String,
        target_did: String,
        payload: Map<String, Value>,
    },
    SetOutboxFailure {
        outbox_id: String,
        error_code: String,
        retry_hint: String,
        metadata: String,
    },
    MarkOutboxSent {
        outbox_id: String,
        session_id: String,
        sent_msg_id: String,
        metadata: String,
    },
    StoreMessage {
        outbox_id: String,
        record: crate::internal::local_state::messages::MessageRecord,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SecureOutboxFlushPlan {
    pub(crate) actions: Vec<SecureOutboxFlushAction>,
    pub(crate) warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SecureOutboxSendRequest {
    pub(crate) outbox_id: String,
    pub(crate) target_did: String,
    pub(crate) original_type: String,
    pub(crate) plaintext: String,
    pub(crate) json_payload: Option<Map<String, Value>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SecureOutboxSendResult {
    pub(crate) send: SecureOutboxSendOutcome,
    pub(crate) session_id: String,
}

pub(crate) fn flush_queued_secure_outbox_with_sender(
    connection: &rusqlite::Connection,
    scope: &crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope,
    peer_filter_did: &str,
    mut sender: impl FnMut(SecureOutboxSendRequest) -> SecureOutboxSendResult,
) -> Vec<String> {
    let rows = match crate::internal::store::e2ee_outbox::list_e2ee_outbox(
        connection,
        scope,
        Some("queued"),
    ) {
        Ok(rows) => rows,
        Err(err) => {
            return compact_warnings(vec![format!("Failed to list secure outbox: {err}")]);
        }
    };
    let rows = rows
        .iter()
        .filter_map(queued_secure_outbox_row_from_record)
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return Vec::new();
    }
    let mut warnings = Vec::new();
    let mut rows = rows;
    rows.sort_by(|left, right| left.created_at.cmp(&right.created_at));
    let peer_filter = peer_filter_did.trim().to_owned();
    for row in rows {
        if !peer_filter.is_empty() && row.peer_did != peer_filter {
            continue;
        }
        let plan = flush_queued_secure_outbox_rows_plan(
            &scope.owner_identity_id,
            &scope.owner_did,
            &scope.credential_name,
            "",
            std::slice::from_ref(&row),
            |row| {
                let original_type = default_string(&row.original_type, "text");
                let json_payload = if original_type == "json" {
                    serde_json::from_str::<Map<String, Value>>(&row.plaintext).ok()
                } else {
                    None
                };
                let result = sender(SecureOutboxSendRequest {
                    outbox_id: row.outbox_id.clone(),
                    target_did: row.peer_did.clone(),
                    original_type,
                    plaintext: row.plaintext.clone(),
                    json_payload,
                });
                SecureOutboxFlushRowOutcome {
                    send: result.send,
                    session_id: result.session_id,
                    mark_sent: MarkSentOutcome::Success,
                    store_message: StoreMessageOutcome::Success,
                }
            },
        );
        warnings.extend(execute_secure_outbox_flush_plan(connection, scope, plan));
    }
    compact_warnings(warnings)
}

pub(crate) fn flush_queued_secure_outbox_rows_plan(
    owner_identity_id: &str,
    owner_did: &str,
    credential_name: &str,
    peer_filter_did: &str,
    rows: &[QueuedSecureOutboxRow],
    mut outcome_for_row: impl FnMut(&QueuedSecureOutboxRow) -> SecureOutboxFlushRowOutcome,
) -> SecureOutboxFlushPlan {
    let mut rows = rows.to_vec();
    rows.sort_by(|left, right| left.created_at.cmp(&right.created_at));

    let peer_filter = peer_filter_did.trim();
    let mut actions = Vec::new();
    let mut warnings = Vec::new();
    for row in rows {
        if !peer_filter.is_empty() && row.peer_did != peer_filter {
            continue;
        }
        let outbox_id = string_value(&row.outbox_id);
        let target_did = string_value(&row.peer_did);
        let original_type = default_string(&string_value(&row.original_type), "text");
        let plaintext = string_value(&row.plaintext);
        if outbox_id.is_empty() || target_did.is_empty() {
            continue;
        }

        match original_type.as_str() {
            "text" | "" => actions.push(SecureOutboxFlushAction::SendText {
                outbox_id: outbox_id.clone(),
                target_did: target_did.clone(),
                plaintext: plaintext.clone(),
            }),
            "json" => match serde_json::from_str::<Map<String, Value>>(&plaintext) {
                Ok(payload) => actions.push(SecureOutboxFlushAction::SendJson {
                    outbox_id: outbox_id.clone(),
                    target_did: target_did.clone(),
                    payload,
                }),
                Err(err) => {
                    actions.push(SecureOutboxFlushAction::SetOutboxFailure {
                        outbox_id: outbox_id.clone(),
                        error_code: "invalid_payload".to_owned(),
                        retry_hint: "drop".to_owned(),
                        metadata: metadata_string(json!({"detail": err.to_string()})),
                    });
                    warnings.push(format!(
                        "Failed to parse queued secure JSON payload {outbox_id}: {err}"
                    ));
                    continue;
                }
            },
            _ => {
                actions.push(SecureOutboxFlushAction::SetOutboxFailure {
                    outbox_id: outbox_id.clone(),
                    error_code: "unsupported_original_type".to_owned(),
                    retry_hint: "drop".to_owned(),
                    metadata: metadata_string(json!({"original_type": original_type})),
                });
                warnings.push(format!(
                    "Queued secure outbox {outbox_id} uses unsupported original_type={original_type}"
                ));
                continue;
            }
        }

        let outcome = outcome_for_row(&row);
        let (message_id, operation_id, delivery_state, accepted_at) = match outcome.send {
            SecureOutboxSendOutcome::Success {
                message_id,
                operation_id,
                delivery_state,
                accepted_at,
            } => (message_id, operation_id, delivery_state, accepted_at),
            SecureOutboxSendOutcome::Error(err) => {
                actions.push(SecureOutboxFlushAction::SetOutboxFailure {
                    outbox_id: outbox_id.clone(),
                    error_code: "send_failed".to_owned(),
                    retry_hint: "retry".to_owned(),
                    metadata: metadata_string(json!({"detail": err})),
                });
                warnings.push(format!(
                    "Failed to flush queued secure outbox {outbox_id}: {err}"
                ));
                continue;
            }
        };

        let sent_msg_id = default_string(&message_id, &outbox_id);
        let metadata = metadata_string(json!({
            "target_did": target_did,
            "operation_id": operation_id,
            "delivery_state": delivery_state,
            "flushed_from": "queued",
        }));
        actions.push(SecureOutboxFlushAction::MarkOutboxSent {
            outbox_id: outbox_id.clone(),
            session_id: outcome.session_id,
            sent_msg_id: sent_msg_id.clone(),
            metadata: metadata.clone(),
        });
        if let MarkSentOutcome::Error(err) = outcome.mark_sent {
            warnings.push(format!(
                "Failed to mark secure outbox {outbox_id} sent: {err}"
            ));
            continue;
        }

        actions.push(SecureOutboxFlushAction::StoreMessage {
            outbox_id: outbox_id.clone(),
            record: crate::internal::local_state::messages::MessageRecord {
                msg_id: sent_msg_id,
                owner_identity_id: owner_identity_id.trim().to_owned(),
                owner_did: owner_did.trim().to_owned(),
                thread_id: make_thread_id(owner_did, &target_did),
                direction: 1,
                sender_did: owner_did.trim().to_owned(),
                receiver_did: target_did,
                content_type: content_type_for_message_type(&original_type).to_owned(),
                content: plaintext,
                sent_at: accepted_at,
                is_read: true,
                is_e2ee: true,
                metadata,
                credential_name: credential_name.trim().to_owned(),
                ..crate::internal::local_state::messages::MessageRecord::default()
            },
        });
        if let StoreMessageOutcome::Error(err) = outcome.store_message {
            warnings.push(format!(
                "Failed to persist flushed secure outbox {outbox_id}: {err}"
            ));
        }
    }

    SecureOutboxFlushPlan {
        actions,
        warnings: compact_warnings(warnings),
    }
}

pub(crate) fn queued_secure_outbox_row_from_record(
    record: &crate::internal::store::e2ee_outbox::E2eeOutboxRecord,
) -> Option<QueuedSecureOutboxRow> {
    if record.outbox_id.trim().is_empty() || record.peer_did.trim().is_empty() {
        return None;
    }
    Some(QueuedSecureOutboxRow {
        outbox_id: record.outbox_id.clone(),
        peer_did: record.peer_did.clone(),
        original_type: record.original_type.clone(),
        plaintext: record.plaintext.clone(),
        created_at: record.created_at.clone(),
    })
}

fn execute_secure_outbox_flush_plan(
    connection: &rusqlite::Connection,
    scope: &crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope,
    plan: SecureOutboxFlushPlan,
) -> Vec<String> {
    let mut warnings = plan.warnings;
    let mut mark_sent_failed = std::collections::BTreeSet::<String>::new();
    for action in plan.actions {
        match action {
            SecureOutboxFlushAction::SendText { .. } | SecureOutboxFlushAction::SendJson { .. } => {
            }
            SecureOutboxFlushAction::SetOutboxFailure {
                outbox_id,
                error_code,
                retry_hint,
                metadata,
            } => {
                if let Err(err) = crate::internal::store::e2ee_outbox::set_e2ee_outbox_failure_by_id(
                    connection,
                    scope,
                    &outbox_id,
                    &error_code,
                    &retry_hint,
                    &metadata,
                ) {
                    warnings.push(format!(
                        "Failed to mark secure outbox {outbox_id} failed: {err}"
                    ));
                }
            }
            SecureOutboxFlushAction::MarkOutboxSent {
                outbox_id,
                session_id,
                sent_msg_id,
                metadata,
            } => {
                if let Err(err) = crate::internal::store::e2ee_outbox::mark_e2ee_outbox_sent(
                    connection,
                    scope,
                    &outbox_id,
                    &session_id,
                    &sent_msg_id,
                    None,
                    &metadata,
                ) {
                    mark_sent_failed.insert(outbox_id.clone());
                    warnings.push(format!(
                        "Failed to mark secure outbox {outbox_id} sent: {err}"
                    ));
                }
            }
            SecureOutboxFlushAction::StoreMessage { outbox_id, record } => {
                if mark_sent_failed.contains(&outbox_id) {
                    continue;
                }
                if let Err(err) =
                    crate::internal::local_state::messages::upsert_message(connection, &record)
                {
                    warnings.push(format!(
                        "Failed to persist flushed secure outbox {outbox_id}: {err}"
                    ));
                }
            }
        }
    }
    compact_warnings(warnings)
}

fn make_thread_id(owner_did: &str, peer_did: &str) -> String {
    let peer_did = peer_did.trim();
    let owner_did = owner_did.trim();
    if !peer_did.is_empty() {
        let mut pair = [owner_did.to_owned(), peer_did.to_owned()];
        pair.sort();
        return format!("dm:{}:{}", pair[0], pair[1]);
    }
    format!("dm:{owner_did}:unknown")
}

fn content_type_for_message_type(message_type: &str) -> &'static str {
    match message_type.trim().to_ascii_lowercase().as_str() {
        "attachment_manifest" => "application/anp-attachment-manifest+json",
        "event" | "json" => "application/json",
        "markdown" => "text/markdown",
        _ => "text/plain",
    }
}

fn string_value(value: &str) -> String {
    value.to_owned()
}

fn default_string(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_owned()
    } else {
        value.to_owned()
    }
}

fn metadata_string(value: Value) -> String {
    value.to_string()
}

fn compact_warnings(warnings: Vec<String>) -> Vec<String> {
    let mut seen = Vec::<String>::new();
    for warning in warnings {
        let warning = warning.trim().to_owned();
        if warning.is_empty() || seen.contains(&warning) {
            continue;
        }
        seen.push(warning);
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secure_outbox_flush_planner_builds_send_and_store_actions() {
        let rows = vec![QueuedSecureOutboxRow {
            outbox_id: "outbox-1".to_owned(),
            peer_did: "did:example:bob".to_owned(),
            original_type: "text".to_owned(),
            plaintext: "hello".to_owned(),
            created_at: "2026-05-24T00:00:00Z".to_owned(),
        }];

        let plan = flush_queued_secure_outbox_rows_plan(
            "alice-id",
            "did:example:alice",
            "alice",
            "",
            &rows,
            |_| SecureOutboxFlushRowOutcome {
                session_id: "session-1".to_owned(),
                send: SecureOutboxSendOutcome::Success {
                    message_id: "msg-1".to_owned(),
                    operation_id: "op-1".to_owned(),
                    delivery_state: "accepted".to_owned(),
                    accepted_at: "2026-05-24T00:00:01Z".to_owned(),
                },
                mark_sent: MarkSentOutcome::Success,
                store_message: StoreMessageOutcome::Success,
            },
        );

        assert_eq!(plan.warnings, Vec::<String>::new());
        assert!(matches!(
            &plan.actions[0],
            SecureOutboxFlushAction::SendText { plaintext, .. } if plaintext == "hello"
        ));
        assert!(matches!(
            &plan.actions[1],
            SecureOutboxFlushAction::MarkOutboxSent { session_id, sent_msg_id, .. }
                if session_id == "session-1" && sent_msg_id == "msg-1"
        ));
        let SecureOutboxFlushAction::StoreMessage { record, .. } = &plan.actions[2] else {
            panic!("expected StoreMessage action");
        };
        assert_eq!(record.owner_identity_id, "alice-id");
        assert_eq!(
            record.thread_id,
            "dm:did:example:alice:did:example:bob".to_owned()
        );
        assert_eq!(record.content, "hello");
        assert!(record.is_e2ee);
    }

    #[test]
    fn secure_outbox_flush_planner_marks_invalid_json_as_drop() {
        let rows = vec![QueuedSecureOutboxRow {
            outbox_id: "outbox-json".to_owned(),
            peer_did: "did:example:bob".to_owned(),
            original_type: "json".to_owned(),
            plaintext: "{".to_owned(),
            created_at: "2026-05-24T00:00:00Z".to_owned(),
        }];

        let plan =
            flush_queued_secure_outbox_rows_plan("", "did:example:alice", "", "", &rows, |_| {
                panic!("invalid json should not call sender")
            });

        assert!(matches!(
            &plan.actions[0],
            SecureOutboxFlushAction::SetOutboxFailure { error_code, retry_hint, .. }
                if error_code == "invalid_payload" && retry_hint == "drop"
        ));
        assert_eq!(plan.warnings.len(), 1);
    }

    #[test]
    fn secure_outbox_flush_planner_records_send_failure_as_retry() {
        let rows = vec![QueuedSecureOutboxRow {
            outbox_id: "outbox-1".to_owned(),
            peer_did: "did:example:bob".to_owned(),
            original_type: "text".to_owned(),
            plaintext: "hello".to_owned(),
            created_at: "2026-05-24T00:00:00Z".to_owned(),
        }];

        let plan =
            flush_queued_secure_outbox_rows_plan("", "did:example:alice", "", "", &rows, |_| {
                SecureOutboxFlushRowOutcome {
                    send: SecureOutboxSendOutcome::Error("network".to_owned()),
                    ..SecureOutboxFlushRowOutcome::default()
                }
            });

        assert!(matches!(
            &plan.actions[1],
            SecureOutboxFlushAction::SetOutboxFailure { error_code, retry_hint, .. }
                if error_code == "send_failed" && retry_hint == "retry"
        ));
        assert_eq!(plan.warnings.len(), 1);
    }

    #[test]
    fn secure_outbox_flush_executor_marks_sent_and_stores_local_message() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        let scope = crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope {
            owner_identity_id: "alice-id".to_owned(),
            owner_did: "did:example:alice".to_owned(),
            credential_name: "alice".to_owned(),
        };
        crate::internal::store::e2ee_outbox::queue_e2ee_outbox(
            &db,
            crate::internal::store::e2ee_outbox::E2eeOutboxRecord {
                outbox_id: "outbox-1".to_owned(),
                owner_identity_id: scope.owner_identity_id.clone(),
                owner_did: scope.owner_did.clone(),
                credential_name: scope.credential_name.clone(),
                peer_did: "did:example:bob".to_owned(),
                original_type: "text".to_owned(),
                plaintext: "queued plaintext".to_owned(),
                local_status: "queued".to_owned(),
                created_at: "2026-05-24T00:00:00Z".to_owned(),
                updated_at: "2026-05-24T00:00:00Z".to_owned(),
                ..Default::default()
            },
        )
        .unwrap();

        let warnings =
            flush_queued_secure_outbox_with_sender(&db, &scope, "did:example:bob", |request| {
                assert_eq!(request.outbox_id, "outbox-1");
                assert_eq!(request.plaintext, "queued plaintext");
                SecureOutboxSendResult {
                    session_id: "session-1".to_owned(),
                    send: SecureOutboxSendOutcome::Success {
                        message_id: "msg-flushed".to_owned(),
                        operation_id: "outbox-1".to_owned(),
                        delivery_state: "accepted".to_owned(),
                        accepted_at: "2026-05-24T00:00:01Z".to_owned(),
                    },
                }
            });

        assert!(warnings.is_empty());
        let row = crate::internal::store::e2ee_outbox::get_e2ee_outbox(&db, &scope, "outbox-1")
            .unwrap()
            .unwrap();
        assert_eq!(row.local_status, "sent");
        assert_eq!(row.session_id, "session-1");
        assert_eq!(row.sent_msg_id, "msg-flushed");
        let stored = db
            .query_row(
                "SELECT owner_identity_id, content, is_e2ee FROM messages WHERE msg_id = 'msg-flushed'",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(stored.0, "alice-id");
        assert_eq!(stored.1, "queued plaintext");
        assert_eq!(stored.2, 1);
    }
}
