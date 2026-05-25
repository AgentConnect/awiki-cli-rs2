use anyhow::Context;
use serde_json::{Map, Value};

const DIRECT_SECURE_INIT_CONTENT_TYPE: &str = "application/anp-direct-init+json";
const DIRECT_SECURE_CIPHER_CONTENT_TYPE: &str = "application/anp-direct-cipher+json";

pub fn is_direct_secure_incoming_notification(notification: &Value) -> bool {
    if string_value(notification.get("method")) != "direct.incoming" {
        return false;
    }
    let params = map_value(notification.get("params"));
    let meta = map_value(value_from_object(params, "meta"));
    is_secure_direct_wire_content_type(&string_from_object(meta, "content_type"))
}

pub fn is_secure_direct_wire_content_type(content_type: &str) -> bool {
    matches!(
        content_type,
        DIRECT_SECURE_INIT_CONTENT_TYPE | DIRECT_SECURE_CIPHER_CONTENT_TYPE
    )
}

pub fn secure_notification_from_message_view(message_view: &Value) -> anyhow::Result<Value> {
    let view = message_view
        .as_object()
        .context("content is not a direct-e2ee object")?;
    let body = value_from_object(Some(view), "content")
        .and_then(Value::as_object)
        .cloned()
        .context("content is not a direct-e2ee object")?;
    let sender_did = string_from_object(Some(view), "sender_did");
    let receiver_did = string_from_object(Some(view), "receiver_did");
    let message_id = string_from_object(Some(view), "id");
    if sender_did.is_empty() || receiver_did.is_empty() || message_id.is_empty() {
        anyhow::bail!("missing sender_did/receiver_did/id");
    }

    let mut meta = Map::new();
    meta.insert("sender_did".to_string(), Value::String(sender_did));
    meta.insert("target".to_string(), target_object(receiver_did));
    meta.insert("message_id".to_string(), Value::String(message_id));
    meta.insert(
        "profile".to_string(),
        Value::String("anp.direct.e2ee.v1".to_string()),
    );
    meta.insert(
        "security_profile".to_string(),
        Value::String("direct-e2ee".to_string()),
    );
    meta.insert(
        "content_type".to_string(),
        Value::String(string_from_object(Some(view), "content_type")),
    );

    let mut params = Map::new();
    params.insert("meta".to_string(), Value::Object(meta));
    params.insert("body".to_string(), Value::Object(body));
    if let Some(server_seq) = value_from_object(Some(view), "server_seq").filter(|v| !v.is_null()) {
        params.insert("server_seq".to_string(), server_seq.clone());
    }

    let mut notification = Map::new();
    notification.insert(
        "method".to_string(),
        Value::String("direct.incoming".to_string()),
    );
    notification.insert("params".to_string(), Value::Object(params));
    Ok(Value::Object(notification))
}

pub fn plaintext_body_to_notification_body(plaintext: &Value) -> Map<String, Value> {
    let plaintext = plaintext.as_object();
    let mut body = Map::new();
    for key in ["conversation_id", "reply_to_message_id", "annotations"] {
        if let Some(value) = value_from_object(plaintext, key).filter(|value| !value.is_null()) {
            body.insert(key.to_string(), value.clone());
        }
    }
    let text = string_from_object(plaintext, "text");
    if !text.is_empty() {
        body.insert("text".to_string(), Value::String(text));
    }
    if let Some(payload) = value_from_object(plaintext, "payload").filter(|value| !value.is_null())
    {
        body.insert("payload".to_string(), payload.clone());
    }
    let payload_b64u = string_from_object(plaintext, "payload_b64u");
    if !payload_b64u.is_empty() {
        body.insert("payload_b64u".to_string(), Value::String(payload_b64u));
    }
    body
}

fn target_object(receiver_did: String) -> Value {
    let mut target = Map::new();
    target.insert("kind".to_string(), Value::String("agent".to_string()));
    target.insert("did".to_string(), Value::String(receiver_did));
    Value::Object(target)
}

fn value_from_object<'a>(object: Option<&'a Map<String, Value>>, key: &str) -> Option<&'a Value> {
    object.and_then(|object| object.get(key))
}

fn map_value(value: Option<&Value>) -> Option<&Map<String, Value>> {
    value.and_then(Value::as_object)
}

fn string_from_object(object: Option<&Map<String, Value>>, key: &str) -> String {
    string_value(value_from_object(object, key))
}

fn string_value(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        _ => String::new(),
    }
}
