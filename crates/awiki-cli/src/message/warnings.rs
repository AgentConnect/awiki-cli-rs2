use crate::message::types::ERR_TRANSPORT_UNAVAILABLE_TEXT;

pub const ERR_TRANSPORT_UNAVAILABLE: &str = ERR_TRANSPORT_UNAVAILABLE_TEXT;

pub fn websocket_http_fallback_warning(err: Option<&dyn std::error::Error>) -> String {
    let detail = websocket_transport_detail(err);
    if detail.is_empty() {
        "WebSocket listener was unavailable for this identity; used HTTP fallback.".to_string()
    } else {
        format!(
            "WebSocket listener was unavailable for this identity; used HTTP fallback. Details: {detail}"
        )
    }
}

pub fn websocket_cache_fallback_warning(err: Option<&dyn std::error::Error>) -> String {
    let detail = websocket_transport_detail(err);
    if detail.is_empty() {
        "WebSocket listener was unavailable for this identity; loaded data from local cache."
            .to_string()
    } else {
        format!(
            "WebSocket listener was unavailable for this identity; loaded data from local cache. Details: {detail}"
        )
    }
}

fn websocket_transport_detail(err: Option<&dyn std::error::Error>) -> String {
    let Some(err) = err else {
        return String::new();
    };
    let mut detail = err.to_string().trim().to_string();
    let prefix = format!("{ERR_TRANSPORT_UNAVAILABLE_TEXT}:");
    if detail.starts_with(&prefix) {
        detail = detail[prefix.len()..].trim().to_string();
    }
    detail
}
