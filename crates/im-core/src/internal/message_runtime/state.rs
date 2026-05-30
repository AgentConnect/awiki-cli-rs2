use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MessageRetryTarget {
    DirectText,
    GroupText,
    DirectPayload,
    GroupPayload,
}

pub(crate) fn send_state_from_delivery(
    delivery: &crate::messages::DeliveryState,
    operation_id: Option<String>,
    message_id: Option<crate::ids::MessageId>,
    updated_at: Option<String>,
    retry_target: Option<MessageRetryTarget>,
) -> (
    crate::messages::MessageSendState,
    Option<crate::messages::MessageRetryPlan>,
) {
    let reason = match delivery {
        crate::messages::DeliveryState::Failed { reason } => non_empty_string(reason),
        _ => None,
    };
    let state = match delivery {
        crate::messages::DeliveryState::Accepted => crate::messages::MessageSendStateKind::Accepted,
        crate::messages::DeliveryState::Sent => crate::messages::MessageSendStateKind::Sent,
        crate::messages::DeliveryState::StoredLocally => {
            crate::messages::MessageSendStateKind::StoredLocally
        }
        crate::messages::DeliveryState::Failed { .. } => {
            crate::messages::MessageSendStateKind::Failed
        }
    };
    let retry_plan = retry_plan_for_state(
        &state,
        retry_target,
        operation_id.clone(),
        message_id.clone(),
        reason.clone(),
    );
    (
        crate::messages::MessageSendState {
            state,
            operation_id,
            message_id,
            reason,
            updated_at,
        },
        retry_plan,
    )
}

pub(crate) fn send_state_label(state: &crate::messages::MessageSendStateKind) -> &'static str {
    match state {
        crate::messages::MessageSendStateKind::Accepted => "accepted",
        crate::messages::MessageSendStateKind::Sent => "sent",
        crate::messages::MessageSendStateKind::StoredLocally => "stored_locally",
        crate::messages::MessageSendStateKind::Failed => "failed",
    }
}

pub(crate) fn send_state_from_metadata(
    metadata: &str,
    fallback_message_id: &str,
) -> Option<crate::messages::MessageSendState> {
    let metadata = parse_metadata(metadata)?;
    if let Some(state) = metadata
        .get("send_state")
        .and_then(|value| send_state_from_value(value, fallback_message_id))
    {
        return Some(state);
    }
    let state = metadata
        .get("delivery_state")
        .and_then(Value::as_str)
        .and_then(send_state_kind_from_str)?;
    let operation_id = string_value(metadata.get("operation_id"));
    let message_id = string_value(metadata.get("message_id"))
        .or_else(|| non_empty_string(fallback_message_id))
        .and_then(parse_message_id);
    let reason = metadata
        .get("failure_reason")
        .and_then(Value::as_str)
        .and_then(non_empty_string);
    let updated_at = metadata
        .get("send_state_updated_at")
        .or_else(|| metadata.get("accepted_at"))
        .and_then(Value::as_str)
        .and_then(non_empty_string);
    Some(crate::messages::MessageSendState {
        state,
        operation_id,
        message_id,
        reason,
        updated_at,
    })
}

pub(crate) fn retry_plan_from_metadata(
    metadata: &str,
    send_state: Option<&crate::messages::MessageSendState>,
    retry_target: Option<MessageRetryTarget>,
) -> Option<crate::messages::MessageRetryPlan> {
    let metadata = parse_metadata(metadata)?;
    if let Some(plan) = metadata.get("retry_plan").and_then(retry_plan_from_value) {
        return Some(plan);
    }
    let state = send_state?;
    retry_plan_for_state(
        &state.state,
        retry_target,
        state.operation_id.clone(),
        state.message_id.clone(),
        state.reason.clone(),
    )
}

pub(crate) fn send_state_from_value(
    value: &Value,
    fallback_message_id: &str,
) -> Option<crate::messages::MessageSendState> {
    if let Some(value) = value.as_str() {
        return Some(crate::messages::MessageSendState {
            state: send_state_kind_from_str(value)?,
            operation_id: None,
            message_id: non_empty_string(fallback_message_id).and_then(parse_message_id),
            reason: None,
            updated_at: None,
        });
    }
    let object = value.as_object()?;
    let state = object
        .get("state")
        .and_then(Value::as_str)
        .and_then(send_state_kind_from_str)?;
    let operation_id = object
        .get("operation_id")
        .and_then(Value::as_str)
        .and_then(non_empty_string);
    let message_id = object
        .get("message_id")
        .and_then(Value::as_str)
        .and_then(non_empty_string)
        .or_else(|| non_empty_string(fallback_message_id))
        .and_then(parse_message_id);
    let reason = object
        .get("reason")
        .or_else(|| object.get("failure_reason"))
        .and_then(Value::as_str)
        .and_then(non_empty_string);
    let updated_at = object
        .get("updated_at")
        .and_then(Value::as_str)
        .and_then(non_empty_string);
    Some(crate::messages::MessageSendState {
        state,
        operation_id,
        message_id,
        reason,
        updated_at,
    })
}

pub(crate) fn retry_plan_for_state(
    state: &crate::messages::MessageSendStateKind,
    retry_target: Option<MessageRetryTarget>,
    operation_id: Option<String>,
    message_id: Option<crate::ids::MessageId>,
    reason: Option<String>,
) -> Option<crate::messages::MessageRetryPlan> {
    if state != &crate::messages::MessageSendStateKind::Failed {
        return None;
    }
    let action = match retry_target? {
        MessageRetryTarget::DirectText => crate::messages::MessageRetryAction::RetryDirectText,
        MessageRetryTarget::GroupText => crate::messages::MessageRetryAction::RetryGroupText,
        MessageRetryTarget::DirectPayload => {
            crate::messages::MessageRetryAction::RetryDirectPayload
        }
        MessageRetryTarget::GroupPayload => crate::messages::MessageRetryAction::RetryGroupPayload,
    };
    Some(crate::messages::MessageRetryPlan {
        retryable: true,
        action,
        operation_id,
        message_id,
        reason,
    })
}

fn retry_plan_from_value(value: &Value) -> Option<crate::messages::MessageRetryPlan> {
    let object = value.as_object()?;
    let retryable = object
        .get("retryable")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let action = object
        .get("action")
        .and_then(Value::as_str)
        .and_then(retry_action_from_str)
        .unwrap_or(crate::messages::MessageRetryAction::None);
    let operation_id = object
        .get("operation_id")
        .and_then(Value::as_str)
        .and_then(non_empty_string);
    let message_id = object
        .get("message_id")
        .and_then(Value::as_str)
        .and_then(non_empty_string)
        .and_then(parse_message_id);
    let reason = object
        .get("reason")
        .and_then(Value::as_str)
        .and_then(non_empty_string);
    Some(crate::messages::MessageRetryPlan {
        retryable,
        action,
        operation_id,
        message_id,
        reason,
    })
}

fn retry_action_from_str(value: &str) -> Option<crate::messages::MessageRetryAction> {
    match normalize_state(value).as_str() {
        "none" => Some(crate::messages::MessageRetryAction::None),
        "retry_direct_text" | "direct_text" => {
            Some(crate::messages::MessageRetryAction::RetryDirectText)
        }
        "retry_group_text" | "group_text" => {
            Some(crate::messages::MessageRetryAction::RetryGroupText)
        }
        "retry_direct_payload" | "direct_payload" => {
            Some(crate::messages::MessageRetryAction::RetryDirectPayload)
        }
        "retry_group_payload" | "group_payload" => {
            Some(crate::messages::MessageRetryAction::RetryGroupPayload)
        }
        _ => None,
    }
}

fn send_state_kind_from_str(value: &str) -> Option<crate::messages::MessageSendStateKind> {
    match normalize_state(value).as_str() {
        "accepted" => Some(crate::messages::MessageSendStateKind::Accepted),
        "sent" => Some(crate::messages::MessageSendStateKind::Sent),
        "stored_locally" => Some(crate::messages::MessageSendStateKind::StoredLocally),
        "failed" => Some(crate::messages::MessageSendStateKind::Failed),
        _ => None,
    }
}

fn parse_metadata(metadata: &str) -> Option<Value> {
    if metadata.trim().is_empty() {
        return None;
    }
    serde_json::from_str(metadata).ok()
}

fn string_value(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).and_then(non_empty_string)
}

fn non_empty_string(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn parse_message_id(value: String) -> Option<crate::ids::MessageId> {
    crate::ids::MessageId::parse(value).ok()
}

fn normalize_state(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace('-', "_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_state_metadata_projects_legacy_delivery_state() {
        let metadata = r#"{"operation_id":"op-1","delivery_state":"stored-locally"}"#;

        let state = send_state_from_metadata(metadata, "msg-1").unwrap();

        assert_eq!(
            state.state,
            crate::messages::MessageSendStateKind::StoredLocally
        );
        assert_eq!(state.operation_id.as_deref(), Some("op-1"));
        assert_eq!(state.message_id.unwrap().as_str(), "msg-1");
    }

    #[test]
    fn message_state_failed_generates_direct_retry_plan() {
        let (state, plan) = send_state_from_delivery(
            &crate::messages::DeliveryState::Failed {
                reason: "service unavailable".to_string(),
            },
            Some("op-2".to_string()),
            crate::ids::MessageId::parse("msg-2").ok(),
            None,
            Some(MessageRetryTarget::DirectText),
        );

        assert_eq!(state.state, crate::messages::MessageSendStateKind::Failed);
        let plan = plan.unwrap();
        assert!(plan.retryable);
        assert_eq!(
            plan.action,
            crate::messages::MessageRetryAction::RetryDirectText
        );
        assert_eq!(plan.operation_id.as_deref(), Some("op-2"));
        assert_eq!(plan.message_id.unwrap().as_str(), "msg-2");
        assert_eq!(plan.reason.as_deref(), Some("service unavailable"));
    }
}
