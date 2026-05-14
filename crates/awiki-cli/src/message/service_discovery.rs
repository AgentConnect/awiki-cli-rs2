use crate::message::types::MessageError;
use serde_json::Value;

const ANP_MESSAGE_SERVICE_TYPE: &str = "ANPMessageService";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiscoveredAttachmentService {
    pub sender_did: String,
    pub service_did: String,
    pub rpc_endpoint: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct DiscoveredMessageService {
    service_did: String,
    service_endpoint: String,
    profiles: Vec<String>,
    security_profiles: Vec<String>,
    priority: i64,
    has_priority: bool,
}

pub fn select_attachment_rpc_service_from_document(
    sender_did: &str,
    document: &Value,
) -> Result<DiscoveredAttachmentService, MessageError> {
    let services = service_entries_from_document(document);
    let mut candidates = services
        .into_iter()
        .filter(|service| !service.service_did.is_empty() && !service.service_endpoint.is_empty())
        .filter(|service| {
            service
                .profiles
                .iter()
                .any(|value| value == "anp.attachment.v1")
        })
        .filter(|service| {
            service
                .security_profiles
                .iter()
                .any(|value| value == "transport-protected")
        })
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Err(MessageError::Json(format!(
            "did document {sender_did} does not expose a compatible ANPMessageService for attachments"
        )));
    }
    candidates.sort_by(
        |left, right| match (left.has_priority, right.has_priority) {
            (true, true) => left.priority.cmp(&right.priority),
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            (false, false) => std::cmp::Ordering::Equal,
        },
    );
    let selected = candidates.remove(0);
    validate_request_uri(&selected.service_endpoint)?;
    Ok(DiscoveredAttachmentService {
        sender_did: sender_did.to_string(),
        service_did: selected.service_did,
        rpc_endpoint: selected.service_endpoint,
    })
}

fn service_entries_from_document(document: &Value) -> Vec<DiscoveredMessageService> {
    document
        .get("service")
        .and_then(Value::as_array)
        .map(|services| {
            services
                .iter()
                .filter_map(Value::as_object)
                .filter(|service| {
                    string_from_value(service.get("type")) == ANP_MESSAGE_SERVICE_TYPE
                })
                .map(|service| {
                    let priority = priority_from_value(service.get("priority"));
                    DiscoveredMessageService {
                        service_did: string_from_value(service.get("serviceDid")),
                        service_endpoint: string_from_value(service.get("serviceEndpoint")),
                        profiles: string_slice_from_value(service.get("profiles")),
                        security_profiles: string_slice_from_value(
                            service
                                .get("securityProfiles")
                                .or_else(|| service.get("security_profiles")),
                        ),
                        priority: priority.unwrap_or_default(),
                        has_priority: priority.is_some(),
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

fn validate_request_uri(value: &str) -> Result<(), MessageError> {
    let trimmed = value.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        Ok(())
    } else {
        Err(MessageError::InvalidAttachmentServiceEndpoint(
            "missing protocol scheme".to_string(),
        ))
    }
}

fn string_from_value(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn string_slice_from_value(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn priority_from_value(value: Option<&Value>) -> Option<i64> {
    match value {
        Some(Value::Number(number)) => number
            .as_i64()
            .or_else(|| number.as_f64().map(|v| v as i64)),
        Some(Value::String(text)) => text.trim().parse().ok(),
        _ => None,
    }
}
