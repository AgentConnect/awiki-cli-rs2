use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalSecureAckDeliveryAction {
    HandleNotification { notification: Value },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalSecureAckDeliveryDecision {
    Skipped,
    Delivered,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSecureAckDeliveryPlan {
    pub actions: Vec<LocalSecureAckDeliveryAction>,
    pub decision: LocalSecureAckDeliveryDecision,
}

pub fn deliver_local_secure_ack_plan(
    target_session_active: bool,
    sender_did: &str,
    recipient_did: &str,
    fallback_message_id: &str,
    ack_result: &Value,
) -> LocalSecureAckDeliveryPlan {
    if !target_session_active {
        return skipped();
    }
    let Some(body) = ack_result.get("body").and_then(Value::as_object) else {
        return skipped();
    };
    if body.is_empty() {
        return skipped();
    }
    let message_id = fallback_string(
        ack_result
            .get("message_id")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        fallback_message_id,
    );
    let notification = json!({
        "method": "direct.incoming",
        "params": {
            "meta": {
                "sender_did": sender_did,
                "target": {"kind": "agent", "did": recipient_did},
                "message_id": message_id,
                "profile": "anp.direct.e2ee.v1",
                "security_profile": "direct-e2ee",
                "content_type": "application/anp-direct-cipher+json",
            },
            "body": Value::Object(body.clone()),
        },
    });
    LocalSecureAckDeliveryPlan {
        actions: vec![LocalSecureAckDeliveryAction::HandleNotification { notification }],
        decision: LocalSecureAckDeliveryDecision::Delivered,
    }
}

fn skipped() -> LocalSecureAckDeliveryPlan {
    LocalSecureAckDeliveryPlan {
        actions: Vec::new(),
        decision: LocalSecureAckDeliveryDecision::Skipped,
    }
}

fn fallback_string(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}
