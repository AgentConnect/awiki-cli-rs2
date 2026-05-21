use time::OffsetDateTime;

const ATTACHMENT_MANIFEST_CONTENT_TYPE: &str = "application/anp-attachment-manifest+json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WireIdentity {
    pub did: String,
}

pub(crate) fn now_rfc3339() -> String {
    let now = OffsetDateTime::now_utc()
        .replace_nanosecond(0)
        .unwrap_or_else(|_| OffsetDateTime::UNIX_EPOCH);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

pub(crate) fn generate_operation_id() -> String {
    let nanos = OffsetDateTime::now_utc()
        .unix_timestamp_nanos()
        .to_be_bytes();
    let pid = std::process::id().to_be_bytes();
    let counter = next_counter().to_be_bytes();

    let mut bytes = [0_u8; 8];
    for (index, byte) in nanos.iter().enumerate() {
        bytes[index % bytes.len()] ^= byte;
    }
    for (index, byte) in pid.iter().enumerate() {
        bytes[index % bytes.len()] ^= byte;
    }
    for (index, byte) in counter.iter().enumerate() {
        bytes[index % bytes.len()] ^= byte;
    }

    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(crate) fn content_type_for_message_kind(
    kind: crate::messages::MessageKind,
    message_type: Option<&str>,
) -> &'static str {
    match message_type
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("attachment_manifest") => ATTACHMENT_MANIFEST_CONTENT_TYPE,
        Some("event") => "application/json",
        _ => match kind {
            crate::messages::MessageKind::Text => "text/plain",
            crate::messages::MessageKind::Markdown => "text/markdown",
        },
    }
}

pub(crate) fn local_meta(sender_did: &str, profile: &str) -> serde_json::Value {
    serde_json::json!({
        "anp_version": "1.0",
        "profile": profile,
        "security_profile": "transport-protected",
        "sender_did": sender_did,
        "operation_id": format!("op-{}", generate_operation_id()),
        "created_at": now_rfc3339(),
    })
}

pub(crate) fn message_meta(
    sender_did: &str,
    service_did: &str,
    profile: &str,
) -> serde_json::Value {
    serde_json::json!({
        "anp_version": "1.0",
        "profile": profile,
        "security_profile": "transport-protected",
        "sender_did": sender_did,
        "target": {
            "kind": "service",
            "did": service_did,
        },
        "operation_id": format!("op-{}", generate_operation_id()),
        "created_at": now_rfc3339(),
    })
}

pub(crate) fn signed_message_meta(
    sender_did: &str,
    target_kind: &str,
    target_did: &str,
    profile: &str,
    content_type: &str,
) -> serde_json::Value {
    serde_json::json!({
        "anp_version": "1.0",
        "profile": profile,
        "security_profile": "transport-protected",
        "sender_did": sender_did,
        "target": {
            "kind": target_kind,
            "did": target_did,
        },
        "operation_id": format!("op-{}", generate_operation_id()),
        "message_id": format!("msg-{}", generate_operation_id()),
        "created_at": now_rfc3339(),
        "content_type": content_type,
    })
}

fn next_counter() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}
