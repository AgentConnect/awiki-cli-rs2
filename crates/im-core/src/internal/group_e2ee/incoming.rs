use anp::group_e2ee::operations::DecryptInput;
use anp::group_e2ee::GroupCipherObject;
use serde_json::{Map, Value};

use super::provider::GroupMlsProvider;
use super::DEFAULT_GROUP_MLS_DEVICE_ID;

const REDACTED_GROUP_E2EE_CONTENT_TYPE: &str = "application/x-awiki-group-e2ee-redacted";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GroupRealtimeNotificationDecision {
    KeepOriginal,
    Normalized,
    Redacted,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GroupRealtimeNotificationProjection {
    pub(crate) notification: Option<Value>,
    pub(crate) warnings: Vec<String>,
    pub(crate) decision: GroupRealtimeNotificationDecision,
}

pub(crate) fn maybe_decrypt_group_e2ee_messages_for_client(
    client: &crate::core::ImClient,
    messages: &mut Vec<Value>,
) -> Vec<String> {
    if messages.is_empty() || !contains_group_e2ee_messages(messages) {
        return Vec::new();
    }
    let provider = match super::storage::native_provider_for_client(client) {
        Ok(provider) => provider,
        Err(err) => return decryptor_init_error(messages, err),
    };
    maybe_decrypt_group_e2ee_messages_with_provider(
        client.did().as_str(),
        device_id_for_client(client).as_str(),
        messages,
        &provider,
    )
}

pub(crate) async fn maybe_decrypt_group_e2ee_messages_for_client_async(
    client: &crate::core::ImClient,
    messages: &mut Vec<Value>,
) -> Vec<String> {
    if messages.is_empty() || !contains_group_e2ee_messages(messages) {
        return Vec::new();
    }
    let provider = match super::storage::native_provider_for_client(client) {
        Ok(provider) => provider,
        Err(err) => return decryptor_init_error(messages, err),
    };
    maybe_decrypt_group_e2ee_messages_with_provider_async(
        client.did().as_str(),
        device_id_for_client(client).as_str(),
        messages,
        provider,
    )
    .await
}

pub(crate) fn maybe_normalize_group_e2ee_notification_for_client(
    client: &crate::core::ImClient,
    notification: Value,
) -> GroupRealtimeNotificationProjection {
    if !is_group_e2ee_incoming_notification(&notification) {
        return keep_realtime_notification(notification);
    }
    let provider = match super::storage::native_provider_for_client(client) {
        Ok(provider) => provider,
        Err(err) => return realtime_decryptor_init_error(notification, err),
    };
    maybe_normalize_group_e2ee_notification_with_provider(
        client.did().as_str(),
        device_id_for_client(client).as_str(),
        notification,
        &provider,
    )
}

pub(crate) async fn maybe_normalize_group_e2ee_notification_for_client_async(
    client: &crate::core::ImClient,
    notification: Value,
) -> GroupRealtimeNotificationProjection {
    if !is_group_e2ee_incoming_notification(&notification) {
        return keep_realtime_notification(notification);
    }
    let provider = match super::storage::native_provider_for_client(client) {
        Ok(provider) => provider,
        Err(err) => return realtime_decryptor_init_error(notification, err),
    };
    maybe_normalize_group_e2ee_notification_with_provider_async(
        client.did().as_str(),
        device_id_for_client(client).as_str(),
        notification,
        provider,
    )
    .await
}

pub(crate) fn maybe_normalize_group_e2ee_notification_with_provider<M: GroupMlsProvider>(
    owner_did: &str,
    device_id: &str,
    notification: Value,
    provider: &M,
) -> GroupRealtimeNotificationProjection {
    if !is_group_e2ee_incoming_notification(&notification) {
        return keep_realtime_notification(notification);
    }
    let mut message = message_view_from_group_notification(&notification);
    let mut messages = vec![message];
    let warnings = maybe_decrypt_group_e2ee_messages_with_provider(
        owner_did,
        device_id,
        &mut messages,
        provider,
    );
    message = messages.into_iter().next().unwrap_or(Value::Null);
    if !warnings.is_empty()
        || !message
            .get("decrypted")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    {
        return redacted_realtime_notification(notification, "failed", warnings);
    }
    let mut normalized = notification;
    apply_decrypted_message_to_group_notification(&mut normalized, &message);
    GroupRealtimeNotificationProjection {
        notification: Some(normalized),
        warnings: Vec::new(),
        decision: GroupRealtimeNotificationDecision::Normalized,
    }
}

pub(crate) async fn maybe_normalize_group_e2ee_notification_with_provider_async<M>(
    owner_did: &str,
    device_id: &str,
    notification: Value,
    provider: M,
) -> GroupRealtimeNotificationProjection
where
    M: GroupMlsProvider + Send + 'static,
{
    if !is_group_e2ee_incoming_notification(&notification) {
        return keep_realtime_notification(notification);
    }
    let mut message = message_view_from_group_notification(&notification);
    let mut messages = vec![message];
    let warnings = maybe_decrypt_group_e2ee_messages_with_provider_async(
        owner_did,
        device_id,
        &mut messages,
        provider,
    )
    .await;
    message = messages.into_iter().next().unwrap_or(Value::Null);
    if !warnings.is_empty()
        || !message
            .get("decrypted")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    {
        return redacted_realtime_notification(notification, "failed", warnings);
    }
    let mut normalized = notification;
    apply_decrypted_message_to_group_notification(&mut normalized, &message);
    GroupRealtimeNotificationProjection {
        notification: Some(normalized),
        warnings: Vec::new(),
        decision: GroupRealtimeNotificationDecision::Normalized,
    }
}

pub(crate) fn maybe_decrypt_group_e2ee_messages_with_provider<M: GroupMlsProvider>(
    owner_did: &str,
    device_id: &str,
    messages: &mut Vec<Value>,
    provider: &M,
) -> Vec<String> {
    if messages.is_empty() || !contains_group_e2ee_messages(messages) {
        return Vec::new();
    }
    let device_id = default_string(device_id, DEFAULT_GROUP_MLS_DEVICE_ID);
    let mut warnings = Vec::new();
    for message in messages {
        let Some(cipher) = group_cipher_object_from_message(message) else {
            continue;
        };
        let group_did = group_did_from_message(message, &cipher);
        if group_did.is_empty() {
            mark_group_e2ee_processing_state(message, "failed");
            warnings.push(format!(
                "Skipped group E2EE message {}: group_did is missing",
                string_from_message(message, "id")
            ));
            continue;
        }
        let message_id = first_non_empty_string(&[message.get("message_id"), message.get("id")]);
        let operation_id = first_non_empty_string(&[
            message.get("operation_id"),
            message.pointer("/receipt/operation_id"),
        ]);
        let sender_did = string_from_message(message, "sender_did");
        let input = DecryptInput {
            recipient_did: owner_did.to_owned(),
            device_id: device_id.clone(),
            group_did: group_did.clone(),
            sender_did,
            message_id,
            operation_id,
            group_cipher_object: cipher,
            request_id: format!(
                "group-e2ee-decrypt-{}",
                crate::internal::wire::common::generate_operation_id()
            ),
        };
        match provider.decrypt(input) {
            Ok(output) => apply_group_plaintext(message, &output.application_plaintext),
            Err(err) => {
                mark_group_e2ee_processing_state(message, "failed");
                warnings.push(format!(
                    "Failed to decrypt group E2EE message {}: {err}",
                    string_from_message(message, "id")
                ));
            }
        }
    }
    compact_warnings(warnings)
}

pub(crate) async fn maybe_decrypt_group_e2ee_messages_with_provider_async<M>(
    owner_did: &str,
    device_id: &str,
    messages: &mut Vec<Value>,
    provider: M,
) -> Vec<String>
where
    M: GroupMlsProvider + Send + 'static,
{
    if messages.is_empty() || !contains_group_e2ee_messages(messages) {
        return Vec::new();
    }
    let owner_did = owner_did.to_owned();
    let device_id = device_id.to_owned();
    let messages_for_worker = messages.clone();
    match crate::internal::runtime::worker::run_blocking(move || {
        let mut messages = messages_for_worker;
        let warnings = maybe_decrypt_group_e2ee_messages_with_provider(
            &owner_did,
            &device_id,
            &mut messages,
            &provider,
        );
        (messages, warnings)
    })
    .await
    {
        Ok((projected, warnings)) => {
            *messages = projected;
            warnings
        }
        Err(err) => {
            redact_all_group_e2ee_messages(messages, "failed");
            compact_warnings(vec![format!(
                "Failed to decrypt group E2EE messages on worker: {err}"
            )])
        }
    }
}

fn decryptor_init_error(messages: &mut [Value], err: crate::ImError) -> Vec<String> {
    redact_all_group_e2ee_messages(messages, "failed");
    compact_warnings(vec![format!(
        "Failed to initialize group E2EE decryptor: {err}"
    )])
}

fn realtime_decryptor_init_error(
    notification: Value,
    err: crate::ImError,
) -> GroupRealtimeNotificationProjection {
    redacted_realtime_notification(
        notification,
        "failed",
        vec![format!(
            "Failed to initialize group E2EE realtime decryptor: {err}"
        )],
    )
}

fn message_view_from_group_notification(notification: &Value) -> Value {
    let params = notification.get("params").and_then(Value::as_object);
    let meta = params
        .and_then(|params| params.get("meta"))
        .and_then(Value::as_object);
    let body = params
        .and_then(|params| params.get("body"))
        .and_then(Value::as_object);
    let group_did = string_from_object(body, "group_did");
    let message_id = default_string(
        &string_from_object(meta, "message_id"),
        &fallback_group_message_id(&group_did, body, meta),
    );
    Value::Object(Map::from_iter([
        ("id".to_owned(), Value::String(message_id.clone())),
        ("message_id".to_owned(), Value::String(message_id)),
        (
            "sender_did".to_owned(),
            Value::String(string_from_object(meta, "sender_did")),
        ),
        ("group_did".to_owned(), Value::String(group_did)),
        (
            "content_type".to_owned(),
            Value::String(string_from_object(meta, "content_type")),
        ),
        (
            "operation_id".to_owned(),
            Value::String(string_from_object(meta, "operation_id")),
        ),
        (
            "group_event_seq".to_owned(),
            value_from_object(body, "group_event_seq")
                .cloned()
                .unwrap_or(Value::Null),
        ),
        (
            "content".to_owned(),
            body.map(|body| Value::Object(body.clone()))
                .unwrap_or(Value::Null),
        ),
    ]))
}

fn apply_decrypted_message_to_group_notification(notification: &mut Value, message: &Value) {
    let Some(params) = notification
        .get_mut("params")
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    params.insert("secure".to_owned(), Value::Bool(true));
    params.insert(
        "secure_state".to_owned(),
        Value::String("decrypted".to_owned()),
    );
    let mut secure_wire_content_type = None;
    if let Some(meta) = params.get_mut("meta").and_then(Value::as_object_mut) {
        secure_wire_content_type = meta
            .get("content_type")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .filter(|value| !value.trim().is_empty());
        if let Some(content_type) = optional_string(message.get("content_type")) {
            meta.insert("content_type".to_owned(), Value::String(content_type));
        }
    }
    if let Some(content_type) = secure_wire_content_type {
        params.insert(
            "secure_wire_content_type".to_owned(),
            Value::String(content_type),
        );
    }

    let original_body = params
        .get("body")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut safe_body = Map::new();
    for key in [
        "group_did",
        "group_event_seq",
        "accepted_at",
        "group_state_version",
    ] {
        if let Some(value) = original_body.get(key).cloned() {
            safe_body.insert(key.to_owned(), value);
        }
    }
    if !safe_body.contains_key("group_did") {
        if let Some(group_did) = optional_string(message.get("group_did")) {
            safe_body.insert("group_did".to_owned(), Value::String(group_did));
        }
    }
    if !safe_body.contains_key("group_event_seq") {
        if let Some(value) = message
            .get("group_event_seq")
            .filter(|value| !value.is_null())
        {
            safe_body.insert("group_event_seq".to_owned(), value.clone());
        }
    }
    safe_body.insert(
        "text".to_owned(),
        message.get("content").cloned().unwrap_or(Value::Null),
    );
    safe_body.insert("decrypted".to_owned(), Value::Bool(true));
    params.insert("body".to_owned(), Value::Object(safe_body));
}

fn is_group_e2ee_incoming_notification(notification: &Value) -> bool {
    if notification.get("method").and_then(Value::as_str) != Some("group.incoming") {
        return false;
    }
    let content_type = notification
        .pointer("/params/meta/content_type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    content_type == super::wire::GROUP_E2EE_CIPHER_CONTENT_TYPE
}

fn contains_group_e2ee_messages(messages: &[Value]) -> bool {
    messages.iter().any(is_group_e2ee_message)
}

fn is_group_e2ee_message(message: &Value) -> bool {
    string_from_message(message, "content_type") == super::wire::GROUP_E2EE_CIPHER_CONTENT_TYPE
        || group_cipher_object_from_message(message).is_some()
}

fn group_cipher_object_from_message(message: &Value) -> Option<GroupCipherObject> {
    for candidate in [
        message.get("group_cipher_object"),
        message.get("content").and_then(|value| {
            value
                .as_object()
                .and_then(|object| object.get("group_cipher_object"))
                .or(Some(value))
        }),
        message
            .get("body")
            .and_then(Value::as_object)
            .and_then(|body| body.get("group_cipher_object")),
    ] {
        let Some(candidate) = candidate else {
            continue;
        };
        if let Ok(cipher) = serde_json::from_value::<GroupCipherObject>(candidate.clone()) {
            return Some(cipher);
        }
    }
    None
}

fn group_did_from_message(message: &Value, cipher: &GroupCipherObject) -> String {
    default_string(
        &string_from_message(message, "group_did"),
        &cipher.group_state_ref.group_did,
    )
}

fn apply_group_plaintext(
    message: &mut Value,
    plaintext: &anp::group_e2ee::GroupApplicationPlaintext,
) {
    let Some(object) = message.as_object_mut() else {
        return;
    };
    object.insert("secure".to_owned(), Value::Bool(true));
    object.insert("decrypted".to_owned(), Value::Bool(true));
    object.insert(
        "decryption_state".to_owned(),
        Value::String("decrypted".to_owned()),
    );
    object.insert(
        "content_type".to_owned(),
        Value::String(default_string(
            plaintext.application_content_type.as_str(),
            "text/plain",
        )),
    );
    if let Some(text) = plaintext.text.as_deref().filter(|value| !value.is_empty()) {
        object.insert("content".to_owned(), Value::String(text.to_owned()));
        object.insert("type".to_owned(), Value::String("text".to_owned()));
    } else if let Some(payload) = plaintext.payload.as_ref() {
        object.insert("content".to_owned(), payload.clone());
        object.insert("type".to_owned(), Value::String("json".to_owned()));
    } else if let Some(payload_b64u) = plaintext
        .payload_b64u
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        object.insert("content".to_owned(), Value::String(payload_b64u.to_owned()));
        object.insert("type".to_owned(), Value::String("binary".to_owned()));
    } else {
        object.insert("content".to_owned(), Value::String(String::new()));
    }
}

fn redact_all_group_e2ee_messages(messages: &mut [Value], state: &str) {
    for message in messages {
        if is_group_e2ee_message(message) {
            mark_group_e2ee_processing_state(message, state);
        }
    }
}

fn mark_group_e2ee_processing_state(message: &mut Value, state: &str) {
    let Some(object) = message.as_object_mut() else {
        return;
    };
    object.insert("secure".to_owned(), Value::Bool(true));
    object.insert(
        "decryption_state".to_owned(),
        Value::String(state.to_owned()),
    );
    object.insert("content".to_owned(), Value::Null);
    object.insert(
        "content_type".to_owned(),
        Value::String(REDACTED_GROUP_E2EE_CONTENT_TYPE.to_owned()),
    );
    object.insert("type".to_owned(), Value::String("redacted".to_owned()));
}

fn redacted_realtime_notification(
    notification: Value,
    state: &str,
    warnings: Vec<String>,
) -> GroupRealtimeNotificationProjection {
    let mut notification = notification;
    if let Some(params) = notification
        .get_mut("params")
        .and_then(Value::as_object_mut)
    {
        params.insert("secure_state".to_owned(), Value::String(state.to_owned()));
        if let Some(meta) = params.get_mut("meta").and_then(Value::as_object_mut) {
            meta.insert(
                "content_type".to_owned(),
                Value::String(REDACTED_GROUP_E2EE_CONTENT_TYPE.to_owned()),
            );
        }
        params.insert("body".to_owned(), Value::Object(Map::new()));
    }
    GroupRealtimeNotificationProjection {
        notification: Some(notification),
        warnings: compact_warnings(warnings),
        decision: GroupRealtimeNotificationDecision::Redacted,
    }
}

fn keep_realtime_notification(notification: Value) -> GroupRealtimeNotificationProjection {
    GroupRealtimeNotificationProjection {
        notification: Some(notification),
        warnings: Vec::new(),
        decision: GroupRealtimeNotificationDecision::KeepOriginal,
    }
}

fn fallback_group_message_id(
    group_did: &str,
    body: Option<&Map<String, Value>>,
    meta: Option<&Map<String, Value>>,
) -> String {
    let group_event_seq = string_like_value(value_from_object(body, "group_event_seq"));
    if !group_did.trim().is_empty() && !group_event_seq.trim().is_empty() {
        return format!("{}:{}", group_did.trim(), group_event_seq.trim());
    }
    default_string(
        &string_from_object(meta, "operation_id"),
        "unknown-group-e2ee-message",
    )
}

fn string_from_message(message: &Value, key: &str) -> String {
    message
        .get(key)
        .and_then(string_from_value)
        .unwrap_or_default()
}

fn string_from_object(object: Option<&Map<String, Value>>, key: &str) -> String {
    value_from_object(object, key)
        .and_then(string_from_value)
        .unwrap_or_default()
}

fn value_from_object<'a>(object: Option<&'a Map<String, Value>>, key: &str) -> Option<&'a Value> {
    object.and_then(|object| object.get(key))
}

fn optional_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(string_from_value)
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn first_non_empty_string(values: &[Option<&Value>]) -> String {
    values
        .iter()
        .map(|value| value.and_then(string_from_value).unwrap_or_default())
        .find(|value| !value.trim().is_empty())
        .unwrap_or_default()
}

fn string_from_value(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn string_like_value(value: Option<&Value>) -> String {
    value.and_then(string_from_value).unwrap_or_default()
}

fn default_string(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_owned()
    } else {
        value.trim().to_owned()
    }
}

fn device_id_for_client(client: &crate::core::ImClient) -> String {
    client
        .current_identity()
        .device_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(DEFAULT_GROUP_MLS_DEVICE_ID)
        .to_owned()
}

fn compact_warnings(warnings: Vec<String>) -> Vec<String> {
    let mut compact = Vec::new();
    for warning in warnings {
        if !warning.trim().is_empty() && !compact.contains(&warning) {
            compact.push(warning);
        }
    }
    compact
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use anp::group_e2ee::operations::{
        AbortCommitInput, AbortCommitOutput, AddMemberInput, CreateGroupInput, DecryptInput,
        DecryptOutput, EncryptInput, EncryptOutput, FinalizeCommitInput, FinalizeCommitOutput,
        GenerateKeyPackageInput, GroupKeyPackageOutput, LeaveGroupInput, PreparedMlsCommitOutput,
        ProcessNoticeInput, ProcessNoticeOutput, ProcessWelcomeInput, ProcessWelcomeOutput,
        RecoverMemberInput, RemoveMemberInput, StatusInput, StatusOutput, UpdateMemberInput,
    };
    use anp::group_e2ee::{GroupApplicationPlaintext, GroupCipherObject, GroupStateRef};
    use serde_json::json;

    use super::*;

    #[test]
    fn group_e2ee_message_projection_decrypts_cipher_and_removes_raw_content() {
        let provider = StaticDecryptProvider::default();
        let mut messages = vec![group_cipher_message("secret-cipher")];
        let warnings = maybe_decrypt_group_e2ee_messages_with_provider(
            "did:example:alice",
            "default",
            &mut messages,
            &provider,
        );

        assert!(warnings.is_empty());
        assert_eq!(messages[0]["content"], "decrypted group text");
        assert_eq!(messages[0]["content_type"], "text/plain");
        assert_eq!(messages[0]["decrypted"], true);
        assert!(!serde_json::to_string(&messages)
            .unwrap()
            .contains("secret-cipher"));
        assert_eq!(
            provider
                .last_input
                .lock()
                .unwrap()
                .as_ref()
                .map(|input| input.group_did.as_str()),
            Some("did:example:groups:e2ee")
        );
    }

    #[test]
    fn group_e2ee_message_projection_redacts_failed_cipher() {
        let provider = FailingDecryptProvider;
        let mut messages = vec![group_cipher_message("failed-cipher")];
        let warnings = maybe_decrypt_group_e2ee_messages_with_provider(
            "did:example:alice",
            "default",
            &mut messages,
            &provider,
        );

        assert_eq!(warnings.len(), 1);
        assert_eq!(messages[0]["content"], Value::Null);
        assert_eq!(
            messages[0]["content_type"],
            REDACTED_GROUP_E2EE_CONTENT_TYPE
        );
        assert!(!serde_json::to_string(&messages)
            .unwrap()
            .contains("failed-cipher"));
    }

    #[tokio::test]
    async fn group_e2ee_message_projection_async_runs_decrypt_on_worker() {
        let provider = StaticDecryptProvider::default();
        let mut messages = vec![group_cipher_message("secret-cipher")];
        let warnings = maybe_decrypt_group_e2ee_messages_with_provider_async(
            "did:example:alice",
            "default",
            &mut messages,
            provider.clone(),
        )
        .await;

        assert!(warnings.is_empty());
        assert_eq!(messages[0]["content"], "decrypted group text");
        assert_eq!(messages[0]["content_type"], "text/plain");
        assert_eq!(messages[0]["decrypted"], true);
        let input = provider.last_input.lock().unwrap().clone().unwrap();
        assert_eq!(input.recipient_did, "did:example:alice");
        assert_eq!(input.group_did, "did:example:groups:e2ee");
        assert!(!serde_json::to_string(&messages)
            .unwrap()
            .contains("secret-cipher"));
    }

    #[tokio::test]
    async fn realtime_notification_projection_async_replaces_wire_body_with_plaintext() {
        let provider = StaticDecryptProvider::default();
        let projection = maybe_normalize_group_e2ee_notification_with_provider_async(
            "did:example:alice",
            "default",
            group_cipher_notification("secret-realtime-cipher"),
            provider,
        )
        .await;

        assert_eq!(
            projection.decision,
            GroupRealtimeNotificationDecision::Normalized
        );
        assert!(projection.warnings.is_empty());
        let notification = projection.notification.unwrap();
        assert_eq!(
            notification.pointer("/params/meta/content_type"),
            Some(&json!("text/plain"))
        );
        assert_eq!(
            notification.pointer("/params/body/text"),
            Some(&json!("decrypted group text"))
        );
        assert!(!serde_json::to_string(&notification)
            .unwrap()
            .contains("secret-realtime-cipher"));
    }

    #[test]
    fn realtime_notification_projection_replaces_wire_body_with_plaintext() {
        let provider = StaticDecryptProvider::default();
        let projection = maybe_normalize_group_e2ee_notification_with_provider(
            "did:example:alice",
            "default",
            group_cipher_notification("secret-realtime-cipher"),
            &provider,
        );

        assert_eq!(
            projection.decision,
            GroupRealtimeNotificationDecision::Normalized
        );
        assert!(projection.warnings.is_empty());
        let notification = projection.notification.unwrap();
        assert_eq!(
            notification.pointer("/params/meta/content_type"),
            Some(&json!("text/plain"))
        );
        assert_eq!(
            notification.pointer("/params/body/text"),
            Some(&json!("decrypted group text"))
        );
        assert_eq!(
            notification.pointer("/params/body/group_did"),
            Some(&json!("did:example:groups:e2ee"))
        );
        assert_eq!(
            notification.pointer("/params/secure_state"),
            Some(&json!("decrypted"))
        );
        assert_eq!(
            notification.pointer("/params/secure_wire_content_type"),
            Some(&json!(super::super::wire::GROUP_E2EE_CIPHER_CONTENT_TYPE))
        );
        let encoded = serde_json::to_string(&notification).unwrap();
        assert!(!encoded.contains("secret-realtime-cipher"));
        assert!(!encoded.contains("private_message_b64u"));
        assert!(!encoded.contains("crypto_group_id_b64u"));
    }

    #[derive(Clone, Default)]
    struct StaticDecryptProvider {
        last_input: Arc<Mutex<Option<DecryptInput>>>,
    }

    impl GroupMlsProvider for StaticDecryptProvider {
        fn decrypt(&self, input: DecryptInput) -> crate::ImResult<DecryptOutput> {
            self.last_input.lock().unwrap().replace(input);
            Ok(DecryptOutput {
                application_plaintext: GroupApplicationPlaintext {
                    application_content_type: "text/plain".to_owned(),
                    thread_id: None,
                    reply_to_message_id: None,
                    annotations: Map::new(),
                    text: Some("decrypted group text".to_owned()),
                    payload: None,
                    payload_b64u: None,
                },
                epoch: "1".to_owned(),
            })
        }

        fn generate_key_package(
            &self,
            _input: GenerateKeyPackageInput,
        ) -> crate::ImResult<GroupKeyPackageOutput> {
            unreachable!("incoming should not generate key packages")
        }

        fn create_group_prepare(
            &self,
            _input: CreateGroupInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("incoming should not create groups")
        }

        fn add_member_prepare(
            &self,
            _input: AddMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("incoming should not add members")
        }

        fn remove_member_prepare(
            &self,
            _input: RemoveMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("incoming should not remove members")
        }

        fn leave_prepare(
            &self,
            _input: LeaveGroupInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("incoming should not leave groups")
        }

        fn update_member_prepare(
            &self,
            _input: UpdateMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("incoming should not update members")
        }

        fn recover_member_prepare(
            &self,
            _input: RecoverMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("incoming should not recover members")
        }

        fn finalize_commit(
            &self,
            _input: FinalizeCommitInput,
        ) -> crate::ImResult<FinalizeCommitOutput> {
            unreachable!("incoming should not finalize commits")
        }

        fn abort_commit(&self, _input: AbortCommitInput) -> crate::ImResult<AbortCommitOutput> {
            unreachable!("incoming should not abort commits")
        }

        fn process_welcome(
            &self,
            _input: ProcessWelcomeInput,
        ) -> crate::ImResult<ProcessWelcomeOutput> {
            unreachable!("incoming should not process welcomes")
        }

        fn process_notice(
            &self,
            _input: ProcessNoticeInput,
        ) -> crate::ImResult<ProcessNoticeOutput> {
            unreachable!("incoming should not process notices")
        }

        fn encrypt(&self, _input: EncryptInput) -> crate::ImResult<EncryptOutput> {
            unreachable!("incoming should not encrypt")
        }

        fn status(&self, _input: StatusInput) -> crate::ImResult<StatusOutput> {
            unreachable!("incoming should not read status")
        }
    }

    struct FailingDecryptProvider;

    impl GroupMlsProvider for FailingDecryptProvider {
        fn decrypt(&self, _input: DecryptInput) -> crate::ImResult<DecryptOutput> {
            Err(crate::ImError::LocalStateUnavailable {
                detail: "missing group state".to_owned(),
            })
        }

        fn generate_key_package(
            &self,
            _input: GenerateKeyPackageInput,
        ) -> crate::ImResult<GroupKeyPackageOutput> {
            unreachable!("incoming should not generate key packages")
        }

        fn create_group_prepare(
            &self,
            _input: CreateGroupInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("incoming should not create groups")
        }

        fn add_member_prepare(
            &self,
            _input: AddMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("incoming should not add members")
        }

        fn remove_member_prepare(
            &self,
            _input: RemoveMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("incoming should not remove members")
        }

        fn leave_prepare(
            &self,
            _input: LeaveGroupInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("incoming should not leave groups")
        }

        fn update_member_prepare(
            &self,
            _input: UpdateMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("incoming should not update members")
        }

        fn recover_member_prepare(
            &self,
            _input: RecoverMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("incoming should not recover members")
        }

        fn finalize_commit(
            &self,
            _input: FinalizeCommitInput,
        ) -> crate::ImResult<FinalizeCommitOutput> {
            unreachable!("incoming should not finalize commits")
        }

        fn abort_commit(&self, _input: AbortCommitInput) -> crate::ImResult<AbortCommitOutput> {
            unreachable!("incoming should not abort commits")
        }

        fn process_welcome(
            &self,
            _input: ProcessWelcomeInput,
        ) -> crate::ImResult<ProcessWelcomeOutput> {
            unreachable!("incoming should not process welcomes")
        }

        fn process_notice(
            &self,
            _input: ProcessNoticeInput,
        ) -> crate::ImResult<ProcessNoticeOutput> {
            unreachable!("incoming should not process notices")
        }

        fn encrypt(&self, _input: EncryptInput) -> crate::ImResult<EncryptOutput> {
            unreachable!("incoming should not encrypt")
        }

        fn status(&self, _input: StatusInput) -> crate::ImResult<StatusOutput> {
            unreachable!("incoming should not read status")
        }
    }

    fn group_cipher_message(private_message_b64u: &str) -> Value {
        json!({
            "id": "msg-group-e2ee",
            "sender_did": "did:example:bob",
            "group_did": "did:example:groups:e2ee",
            "content_type": super::super::wire::GROUP_E2EE_CIPHER_CONTENT_TYPE,
            "operation_id": "op-group-e2ee",
            "content": GroupCipherObject {
                crypto_group_id_b64u: "crypto".to_owned(),
                epoch: "1".to_owned(),
                private_message_b64u: private_message_b64u.to_owned(),
                group_state_ref: GroupStateRef {
                    group_did: "did:example:groups:e2ee".to_owned(),
                    group_state_version: "42".to_owned(),
                    policy_hash: None,
                },
                epoch_authenticator: None,
                non_cryptographic: false,
                artifact_mode: None,
            }
        })
    }

    fn group_cipher_notification(private_message_b64u: &str) -> Value {
        json!({
            "method": "group.incoming",
            "params": {
                "meta": {
                    "message_id": "msg-group-realtime-e2ee",
                    "operation_id": "op-group-realtime-e2ee",
                    "sender_did": "did:example:bob",
                    "target": {
                        "kind": "agent",
                        "did": "did:example:alice"
                    },
                    "content_type": super::super::wire::GROUP_E2EE_CIPHER_CONTENT_TYPE
                },
                "body": {
                    "group_did": "did:example:groups:e2ee",
                    "group_event_seq": 7,
                    "crypto_group_id_b64u": "crypto",
                    "epoch": "1",
                    "private_message_b64u": private_message_b64u,
                    "group_state_ref": {
                        "group_did": "did:example:groups:e2ee",
                        "group_state_version": "42"
                    }
                }
            }
        })
    }
}
