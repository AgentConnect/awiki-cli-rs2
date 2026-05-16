use crate::message::content_type_for_message_type;
use crate::store::{self, MessageRecord};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedSecureOutboxRow {
    pub outbox_id: String,
    pub peer_did: String,
    pub original_type: String,
    pub plaintext: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecureOutboxSendOutcome {
    Success {
        message_id: String,
        operation_id: String,
        delivery_state: String,
        accepted_at: String,
    },
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkSentOutcome {
    Success,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreMessageOutcome {
    Success,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecureOutboxFlushRowOutcome {
    pub send: SecureOutboxSendOutcome,
    pub session_id: String,
    pub mark_sent: MarkSentOutcome,
    pub store_message: StoreMessageOutcome,
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

#[derive(Debug, Clone)]
pub enum SecureOutboxFlushAction {
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
        record: MessageRecord,
    },
}

impl PartialEq for SecureOutboxFlushAction {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::SendText {
                    outbox_id,
                    target_did,
                    plaintext,
                },
                Self::SendText {
                    outbox_id: other_outbox_id,
                    target_did: other_target_did,
                    plaintext: other_plaintext,
                },
            ) => {
                outbox_id == other_outbox_id
                    && target_did == other_target_did
                    && plaintext == other_plaintext
            }
            (
                Self::SendJson {
                    outbox_id,
                    target_did,
                    payload,
                },
                Self::SendJson {
                    outbox_id: other_outbox_id,
                    target_did: other_target_did,
                    payload: other_payload,
                },
            ) => {
                outbox_id == other_outbox_id
                    && target_did == other_target_did
                    && payload == other_payload
            }
            (
                Self::SetOutboxFailure {
                    outbox_id,
                    error_code,
                    retry_hint,
                    metadata,
                },
                Self::SetOutboxFailure {
                    outbox_id: other_outbox_id,
                    error_code: other_error_code,
                    retry_hint: other_retry_hint,
                    metadata: other_metadata,
                },
            ) => {
                outbox_id == other_outbox_id
                    && error_code == other_error_code
                    && retry_hint == other_retry_hint
                    && metadata == other_metadata
            }
            (
                Self::MarkOutboxSent {
                    outbox_id,
                    session_id,
                    sent_msg_id,
                    metadata,
                },
                Self::MarkOutboxSent {
                    outbox_id: other_outbox_id,
                    session_id: other_session_id,
                    sent_msg_id: other_sent_msg_id,
                    metadata: other_metadata,
                },
            ) => {
                outbox_id == other_outbox_id
                    && session_id == other_session_id
                    && sent_msg_id == other_sent_msg_id
                    && metadata == other_metadata
            }
            (
                Self::StoreMessage { outbox_id, record },
                Self::StoreMessage {
                    outbox_id: other_outbox_id,
                    record: other_record,
                },
            ) => outbox_id == other_outbox_id && message_records_equal(record, other_record),
            _ => false,
        }
    }
}

fn message_records_equal(left: &MessageRecord, right: &MessageRecord) -> bool {
    left.msg_id == right.msg_id
        && left.owner_did == right.owner_did
        && left.thread_id == right.thread_id
        && left.direction == right.direction
        && left.sender_did == right.sender_did
        && left.receiver_did == right.receiver_did
        && left.group_id == right.group_id
        && left.group_did == right.group_did
        && left.content_type == right.content_type
        && left.content == right.content
        && left.title == right.title
        && left.server_seq == right.server_seq
        && left.sent_at == right.sent_at
        && left.stored_at == right.stored_at
        && left.is_e2ee == right.is_e2ee
        && left.is_read == right.is_read
        && left.sender_name == right.sender_name
        && left.metadata == right.metadata
        && left.credential_name == right.credential_name
}

#[derive(Debug, Clone, PartialEq)]
pub struct SecureOutboxFlushPlan {
    pub actions: Vec<SecureOutboxFlushAction>,
    pub warnings: Vec<String>,
}

pub fn flush_queued_secure_outbox_rows_plan(
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
                        error_code: "invalid_payload".to_string(),
                        retry_hint: "drop".to_string(),
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
                    error_code: "unsupported_original_type".to_string(),
                    retry_hint: "drop".to_string(),
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
                    error_code: "send_failed".to_string(),
                    retry_hint: "retry".to_string(),
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
            record: MessageRecord {
                msg_id: sent_msg_id,
                owner_did: owner_did.to_string(),
                thread_id: store::make_thread_id(owner_did, &target_did, ""),
                direction: 1,
                sender_did: owner_did.to_string(),
                receiver_did: target_did,
                content_type: content_type_for_message_type(&original_type).to_string(),
                content: plaintext,
                sent_at: accepted_at,
                is_read: true,
                is_e2ee: true,
                metadata,
                credential_name: credential_name.to_string(),
                ..MessageRecord::default()
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

pub fn flush_queued_secure_outbox_with_sender(
    connection: &rusqlite::Connection,
    owner_did: &str,
    credential_name: &str,
    peer_filter_did: &str,
    mut sender: impl FnMut(SecureOutboxSendRequest) -> SecureOutboxSendOutcome,
    mut current_session_id: impl FnMut(&str) -> String,
) -> Vec<String> {
    let rows = match store::list_e2ee_outbox(connection, owner_did, credential_name, "queued") {
        Ok(rows) => rows,
        Err(err) => {
            return compact_warnings(vec![format!("Failed to list secure outbox: {err}")]);
        }
    };
    let mut rows = rows
        .iter()
        .filter_map(queued_secure_outbox_row_from_value)
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return Vec::new();
    }
    rows.sort_by(|left, right| left.created_at.cmp(&right.created_at));
    let peer_filter = peer_filter_did.trim().to_string();

    let mut warnings = Vec::new();
    for row in rows {
        if !peer_filter.is_empty() && row.peer_did != peer_filter {
            continue;
        }
        let plan = flush_queued_secure_outbox_rows_plan(
            owner_did,
            credential_name,
            "",
            std::slice::from_ref(&row),
            |row| {
                let original_type = default_string(&row.original_type, "text");
                let json_payload = if original_type == "json" {
                    serde_json::from_str::<Map<String, Value>>(&row.plaintext).ok()
                } else {
                    None
                };
                let send = sender(SecureOutboxSendRequest {
                    outbox_id: row.outbox_id.clone(),
                    target_did: row.peer_did.clone(),
                    original_type,
                    plaintext: row.plaintext.clone(),
                    json_payload,
                });
                let session_id = match &send {
                    SecureOutboxSendOutcome::Success { .. } => current_session_id(&row.peer_did),
                    SecureOutboxSendOutcome::Error(_) => String::new(),
                };
                SecureOutboxFlushRowOutcome {
                    session_id,
                    send,
                    mark_sent: MarkSentOutcome::Success,
                    store_message: StoreMessageOutcome::Success,
                }
            },
        );
        warnings.extend(execute_secure_outbox_flush_plan(
            connection,
            owner_did,
            credential_name,
            plan,
        ));
    }
    compact_warnings(warnings)
}

#[derive(Debug, Clone, PartialEq)]
pub struct SecureOutboxSendRequest {
    pub outbox_id: String,
    pub target_did: String,
    pub original_type: String,
    pub plaintext: String,
    pub json_payload: Option<Map<String, Value>>,
}

fn execute_secure_outbox_flush_plan(
    connection: &rusqlite::Connection,
    owner_did: &str,
    credential_name: &str,
    plan: SecureOutboxFlushPlan,
) -> Vec<String> {
    let mut warnings = plan.warnings;
    let mut mark_sent_failed = BTreeSet::<String>::new();
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
                let _ = store::set_e2ee_outbox_failure_by_id(
                    connection,
                    &outbox_id,
                    owner_did,
                    credential_name,
                    &error_code,
                    &retry_hint,
                    &metadata,
                );
            }
            SecureOutboxFlushAction::MarkOutboxSent {
                outbox_id,
                session_id,
                sent_msg_id,
                metadata,
            } => {
                if let Err(err) = store::mark_e2ee_outbox_sent(
                    connection,
                    &outbox_id,
                    owner_did,
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
                if let Err(err) = store::store_message(connection, record) {
                    warnings.push(format!(
                        "Failed to persist flushed secure outbox {outbox_id}: {err}"
                    ));
                }
            }
        }
    }
    compact_warnings(warnings)
}

fn queued_secure_outbox_row_from_value(row: &Value) -> Option<QueuedSecureOutboxRow> {
    let outbox_id = row.get("outbox_id").and_then(Value::as_str)?.to_string();
    let peer_did = row.get("peer_did").and_then(Value::as_str)?.to_string();
    if outbox_id.is_empty() || peer_did.is_empty() {
        return None;
    }
    Some(QueuedSecureOutboxRow {
        outbox_id,
        peer_did,
        original_type: row
            .get("original_type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        plaintext: row
            .get("plaintext")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        created_at: row
            .get("created_at")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

fn string_value(value: &str) -> String {
    value.to_string()
}

fn default_string(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

fn metadata_string(value: Value) -> String {
    value.to_string()
}

pub fn compact_warnings(warnings: Vec<String>) -> Vec<String> {
    let mut seen = Vec::<String>::new();
    for warning in warnings {
        let warning = warning.trim().to_string();
        if warning.is_empty() || seen.contains(&warning) {
            continue;
        }
        seen.push(warning);
    }
    seen
}
