use super::types::IdentityError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedHandle {
    pub local_part: String,
    pub full_handle: String,
    pub effective_domain: String,
    pub explicit_domain: bool,
}

pub fn normalize_handle_input(
    raw: &str,
    did_domain: &str,
) -> Result<NormalizedHandle, IdentityError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(invalid_input("handle is required"));
    }
    let lower = trimmed.to_lowercase();
    if lower.starts_with("did:") {
        return Err(invalid_input(format!(
            "did values are not supported in handle input {raw:?}"
        )));
    }

    let handle = lower.strip_prefix("wba://").unwrap_or(&lower);
    if handle.is_empty() {
        return Err(invalid_input("handle is required"));
    }
    if let Some(dot) = handle.find('.') {
        let local_part = handle[..dot].trim().to_string();
        let domain = normalize_handle_domain(&handle[dot + 1..]);
        if local_part.is_empty() || domain.is_empty() {
            return Err(invalid_input(format!("invalid handle {raw:?}")));
        }
        return Ok(NormalizedHandle {
            full_handle: format!("{local_part}.{domain}"),
            local_part,
            effective_domain: domain,
            explicit_domain: true,
        });
    }

    let domain = normalize_handle_domain(did_domain);
    if domain.is_empty() {
        return Err(invalid_input(format!(
            "did_domain is required to complete bare handle {raw:?}"
        )));
    }
    Ok(NormalizedHandle {
        full_handle: format!("{handle}.{domain}"),
        local_part: handle.to_string(),
        effective_domain: domain,
        explicit_domain: false,
    })
}

pub(crate) fn stored_handle_fields(handle: &str, full_handle: &str, did: &str) -> (String, String) {
    let mut local_part = handle.trim().to_lowercase();
    if let Some(stripped) = local_part.strip_prefix("wba://") {
        local_part = stripped.to_string();
    }
    if let Some(index) = local_part.find('.') {
        local_part.truncate(index);
    }

    if let Some(normalized_full) = normalize_stored_full_handle(full_handle, did) {
        if local_part.is_empty() {
            local_part = normalized_full.local_part;
        }
        return (local_part, normalized_full.full_handle);
    }
    if local_part.is_empty() {
        return (String::new(), String::new());
    }
    let full = derive_full_handle_from_did(&local_part, did);
    (local_part, full)
}

pub fn derive_full_handle_from_did(handle: &str, did: &str) -> String {
    let local_part = handle.trim().to_lowercase();
    if local_part.is_empty() {
        return String::new();
    }
    let Ok((domain, _)) = handle_path_prefix_from_did(did) else {
        return String::new();
    };
    let domain = normalize_handle_domain(&domain);
    if domain.is_empty() {
        return String::new();
    }
    format!("{local_part}.{domain}")
}

fn normalize_stored_full_handle(full_handle: &str, did: &str) -> Option<NormalizedHandle> {
    let trimmed = full_handle.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(normalized) = normalize_handle_input(trimmed, "") {
        return Some(normalized);
    }
    let Ok((domain, _)) = handle_path_prefix_from_did(did) else {
        return None;
    };
    normalize_handle_input(trimmed, &domain).ok()
}

pub fn default_string(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

fn handle_path_prefix_from_did(did: &str) -> Result<(String, Vec<String>), IdentityError> {
    let (domain, path_segments) = parse_did_path(did)?;
    if path_segments.is_empty() || path_segments[0].eq_ignore_ascii_case("user") {
        return Err(invalid_input("current did is not a handle did"));
    }
    Ok((domain, path_segments))
}

fn parse_did_path(did: &str) -> Result<(String, Vec<String>), IdentityError> {
    let trimmed = did.trim();
    if !trimmed.starts_with("did:wba:") {
        return Err(invalid_input(format!("invalid did {did:?}")));
    }
    let parts: Vec<&str> = trimmed.split(':').collect();
    if parts.len() < 5 {
        return Err(invalid_input(format!("invalid did {did:?}")));
    }
    let domain = path_unescape(parts[2])
        .ok_or_else(|| invalid_input(format!("invalid did domain {:?}", parts[2])))?;
    let path_segments = parts[3..parts.len() - 1]
        .iter()
        .map(|segment| (*segment).to_string())
        .collect::<Vec<_>>();
    if path_segments.is_empty() {
        return Err(invalid_input("missing did path segments"));
    }
    Ok((domain, path_segments))
}

fn normalize_handle_domain(domain: &str) -> String {
    let value = domain.trim().to_lowercase();
    value
        .strip_suffix('.')
        .map(ToString::to_string)
        .unwrap_or(value)
}

fn path_unescape(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            output.push(bytes[index]);
            index += 1;
            continue;
        }
        if index + 2 >= bytes.len() {
            return None;
        }
        let high = hex_value(bytes[index + 1])?;
        let low = hex_value(bytes[index + 2])?;
        output.push((high << 4) | low);
        index += 3;
    }
    String::from_utf8(output).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn invalid_input(message: impl Into<String>) -> IdentityError {
    IdentityError::InvalidInput(format!("invalid input: {}", message.into()))
}
