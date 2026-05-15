use super::listener_secure_ack_delivery::build_secure_ack_payload;
use super::listener_secure_notifications::{
    is_direct_secure_incoming_notification, plaintext_body_to_notification_body,
};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq)]
pub enum SecureProcessOutcome {
    Error,
    Result(Value),
}

#[derive(Debug, Clone, PartialEq)]
pub enum LocalSecureInitAckOutcome {
    DeliveredInProcess,
    NetworkSendFailed,
    NetworkSendSucceeded { ack_result: Value },
}

#[derive(Debug, Clone, PartialEq)]
pub enum NormalizeDirectSecureAction {
    BuildSecureE2eeClient,
    ProcessIncoming,
    FlushQueuedSecureOutbox {
        peer_did: String,
    },
    DeliverLocalSecureAckInProcess {
        recipient_did: String,
        session_id: String,
        replied_message_id: String,
        ack_message_id: String,
    },
    SendSecureAckJson {
        recipient_did: String,
        payload: Map<String, Value>,
        message_id: String,
        request_id: String,
    },
    DeliverLocalSecureAck {
        sender_did: String,
        recipient_did: String,
        fallback_message_id: String,
        ack_result: Value,
    },
    FlushPeerQueuedSecureOutbox {
        owner_did: String,
        peer_did: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormalizeDirectSecureDecision {
    KeepOriginal,
    Normalized,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NormalizeDirectSecurePlan {
    pub notification: Value,
    pub actions: Vec<NormalizeDirectSecureAction>,
    pub decision: NormalizeDirectSecureDecision,
}

pub fn normalize_direct_secure_notification_plan(
    notification: &Value,
    current_record_did: Option<&str>,
    secure_rpc_available: bool,
    client_ready: bool,
    process_outcome: SecureProcessOutcome,
    init_ack_outcome: LocalSecureInitAckOutcome,
) -> NormalizeDirectSecurePlan {
    if !is_direct_secure_incoming_notification(notification) {
        return keep_original(notification, Vec::new());
    }
    let Some(current_record_did) = current_record_did else {
        return keep_original(notification, Vec::new());
    };
    if !secure_rpc_available {
        return keep_original(notification, Vec::new());
    }

    let mut actions = vec![NormalizeDirectSecureAction::BuildSecureE2eeClient];
    if !client_ready {
        return keep_original(notification, actions);
    }
    actions.push(NormalizeDirectSecureAction::ProcessIncoming);

    let SecureProcessOutcome::Result(result) = process_outcome else {
        return keep_original(notification, actions);
    };
    if string_value(result.get("state")) != "decrypted" {
        return keep_original(notification, actions);
    }
    let Some(plaintext) = result.get("plaintext").and_then(Value::as_object) else {
        return keep_original(notification, actions);
    };

    let mut normalized = notification.clone();
    let (sender_did, message_id, original_content_type, original_body) =
        apply_decrypted_plaintext(&mut normalized, plaintext);

    if is_secure_ack_plaintext(plaintext) {
        actions.push(NormalizeDirectSecureAction::FlushQueuedSecureOutbox {
            peer_did: sender_did,
        });
        set_method(&mut normalized, "direct.secure.ack");
        return normalized_plan(normalized, actions);
    }

    if is_secure_init_plaintext(plaintext) {
        set_method(&mut normalized, "direct.secure.init");
    }

    if original_content_type == "application/anp-direct-init+json" {
        let session_id = string_value(
            original_body
                .as_object()
                .and_then(|body| body.get("session_id")),
        );
        if !session_id.is_empty() && !message_id.is_empty() {
            plan_secure_init_ack(
                &mut actions,
                current_record_did,
                &sender_did,
                &session_id,
                &message_id,
                init_ack_outcome,
            );
        }
    }

    normalized_plan(normalized, actions)
}

fn apply_decrypted_plaintext(
    notification: &mut Value,
    plaintext: &Map<String, Value>,
) -> (String, String, String, Value) {
    let params = notification
        .get_mut("params")
        .and_then(Value::as_object_mut)
        .expect("secure direct notification has params object");
    let original_body = params.get("body").cloned().unwrap_or(Value::Null);
    let (sender_did, message_id, original_content_type) = {
        let meta = params
            .get_mut("meta")
            .and_then(Value::as_object_mut)
            .expect("secure direct notification has meta object");
        let sender_did = string_value(meta.get("sender_did"));
        let message_id = string_value(meta.get("message_id"));
        let original_content_type = string_value(meta.get("content_type"));
        meta.insert(
            "content_type".to_string(),
            Value::String(string_value(plaintext.get("application_content_type"))),
        );
        (sender_did, message_id, original_content_type)
    };
    params.insert(
        "body".to_string(),
        Value::Object(plaintext_body_to_notification_body(&Value::Object(
            plaintext.clone(),
        ))),
    );
    params.insert(
        "secure_state".to_string(),
        Value::String("decrypted".to_string()),
    );
    params.insert(
        "secure_wire_content_type".to_string(),
        Value::String(original_content_type.clone()),
    );
    params.insert("secure_wire_body".to_string(), original_body.clone());
    (sender_did, message_id, original_content_type, original_body)
}

fn plan_secure_init_ack(
    actions: &mut Vec<NormalizeDirectSecureAction>,
    current_record_did: &str,
    peer_did: &str,
    session_id: &str,
    message_id: &str,
    init_ack_outcome: LocalSecureInitAckOutcome,
) {
    let ack_message_id = format!("ack-{session_id}");
    actions.push(
        NormalizeDirectSecureAction::DeliverLocalSecureAckInProcess {
            recipient_did: peer_did.to_string(),
            session_id: session_id.to_string(),
            replied_message_id: message_id.to_string(),
            ack_message_id: ack_message_id.clone(),
        },
    );
    match init_ack_outcome {
        LocalSecureInitAckOutcome::DeliveredInProcess => {}
        LocalSecureInitAckOutcome::NetworkSendFailed => {
            actions.push(send_secure_ack_action(
                peer_did,
                session_id,
                message_id,
                &ack_message_id,
            ));
        }
        LocalSecureInitAckOutcome::NetworkSendSucceeded { ack_result } => {
            actions.push(send_secure_ack_action(
                peer_did,
                session_id,
                message_id,
                &ack_message_id,
            ));
            actions.push(NormalizeDirectSecureAction::DeliverLocalSecureAck {
                sender_did: current_record_did.to_string(),
                recipient_did: peer_did.to_string(),
                fallback_message_id: ack_message_id.clone(),
                ack_result,
            });
        }
    }
    actions.push(NormalizeDirectSecureAction::FlushPeerQueuedSecureOutbox {
        owner_did: peer_did.to_string(),
        peer_did: current_record_did.to_string(),
    });
}

fn send_secure_ack_action(
    peer_did: &str,
    session_id: &str,
    message_id: &str,
    ack_message_id: &str,
) -> NormalizeDirectSecureAction {
    NormalizeDirectSecureAction::SendSecureAckJson {
        recipient_did: peer_did.to_string(),
        payload: build_secure_ack_payload(session_id, message_id),
        message_id: ack_message_id.to_string(),
        request_id: ack_message_id.to_string(),
    }
}

fn is_secure_ack_plaintext(plaintext: &Map<String, Value>) -> bool {
    is_secure_control_plaintext(plaintext, "awiki.direct.secure_ack.v1")
}

fn is_secure_init_plaintext(plaintext: &Map<String, Value>) -> bool {
    is_secure_control_plaintext(plaintext, "awiki.direct.secure_init.v1")
}

fn is_secure_control_plaintext(plaintext: &Map<String, Value>, system_type: &str) -> bool {
    if string_value(plaintext.get("application_content_type")) != "application/json" {
        return false;
    }
    let Some(payload) = plaintext.get("payload").and_then(Value::as_object) else {
        return false;
    };
    string_value(payload.get("system_type")) == system_type
}

fn set_method(notification: &mut Value, method: &str) {
    if let Some(object) = notification.as_object_mut() {
        object.insert("method".to_string(), Value::String(method.to_string()));
    }
}

fn keep_original(
    notification: &Value,
    actions: Vec<NormalizeDirectSecureAction>,
) -> NormalizeDirectSecurePlan {
    NormalizeDirectSecurePlan {
        notification: notification.clone(),
        actions,
        decision: NormalizeDirectSecureDecision::KeepOriginal,
    }
}

fn normalized_plan(
    notification: Value,
    actions: Vec<NormalizeDirectSecureAction>,
) -> NormalizeDirectSecurePlan {
    NormalizeDirectSecurePlan {
        notification,
        actions,
        decision: NormalizeDirectSecureDecision::Normalized,
    }
}

fn string_value(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        _ => String::new(),
    }
}
