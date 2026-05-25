use std::time::{SystemTime, UNIX_EPOCH};
use time::OffsetDateTime;

pub fn now_utc() -> String {
    format_go_rfc3339(OffsetDateTime::now_utc())
}

fn format_go_rfc3339(value: OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        value.year(),
        u8::from(value.month()),
        value.day(),
        value.hour(),
        value.minute(),
        value.second()
    )
}

pub fn make_thread_id(my_did: &str, peer_did: &str, group_id: &str) -> String {
    let group_id = group_id.trim();
    if !group_id.is_empty() {
        return format!("group:{group_id}");
    }
    let peer_did = peer_did.trim();
    let my_did = my_did.trim();
    if !peer_did.is_empty() {
        let mut pair = [my_did.to_string(), peer_did.to_string()];
        pair.sort();
        return format!("dm:{}:{}", pair[0], pair[1]);
    }
    format!("dm:{my_did}:unknown")
}

pub fn normalize_owner_did(value: &str) -> String {
    value.trim().to_string()
}

pub fn normalize_credential_name(value: &str) -> String {
    value.trim().to_string()
}

pub fn normalize_optional_string(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

pub fn normalize_optional_bool(value: Option<bool>) -> Option<i64> {
    value.map(bool_to_int)
}

pub fn normalize_optional_int64(value: Option<i64>) -> Option<i64> {
    value
}

pub fn default_int64_ptr(value: Option<i64>, fallback: Option<i64>) -> Option<i64> {
    value.or(fallback)
}

pub fn normalize_optional_float64(value: Option<f64>) -> Option<f64> {
    value
}

pub fn normalize_metadata(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

pub fn default_bool_value(value: Option<bool>) -> i64 {
    value.map(bool_to_int).unwrap_or(0)
}

pub fn bool_to_int(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}

pub fn default_string(value: String, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value
    }
}

pub fn generate_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("local-{nanos}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::format_description::well_known::Rfc3339;

    #[test]
    fn now_utc_matches_go_rfc3339_second_precision_shape() {
        let value = now_utc();

        assert_eq!(value.len(), "2026-05-14T11:38:35Z".len());
        assert!(value.ends_with('Z'));
        assert!(!value.contains('.'));
        OffsetDateTime::parse(&value, &Rfc3339).expect("timestamp should parse as RFC3339");
    }

    #[test]
    fn normalization_helpers_match_go_store_helpers() {
        assert_eq!(normalize_owner_did(" did:owner \n"), "did:owner");
        assert_eq!(normalize_credential_name(" cred \t"), "cred");

        assert_eq!(normalize_optional_string("  "), None);
        assert_eq!(
            normalize_optional_string(" value "),
            Some("value".to_string())
        );
        assert_eq!(normalize_optional_bool(None), None);
        assert_eq!(normalize_optional_bool(Some(true)), Some(1));
        assert_eq!(normalize_optional_bool(Some(false)), Some(0));
        assert_eq!(normalize_optional_int64(Some(42)), Some(42));
        assert_eq!(normalize_optional_int64(None), None);
        assert_eq!(normalize_optional_float64(Some(4.5)), Some(4.5));
        assert_eq!(normalize_optional_float64(None), None);

        assert_eq!(normalize_metadata("  "), None);
        assert_eq!(
            normalize_metadata(" {\"a\":1} "),
            Some("{\"a\":1}".to_string())
        );
        assert_eq!(default_bool_value(None), 0);
        assert_eq!(default_bool_value(Some(true)), 1);
        assert_eq!(default_bool_value(Some(false)), 0);
        assert_eq!(bool_to_int(true), 1);
        assert_eq!(bool_to_int(false), 0);
        assert_eq!(default_string("  ".to_string(), "fallback"), "fallback");
        assert_eq!(default_string(" value ".to_string(), "fallback"), " value ");
        assert_eq!(default_int64_ptr(Some(7), Some(9)), Some(7));
        assert_eq!(default_int64_ptr(None, Some(9)), Some(9));
        assert_eq!(default_int64_ptr(None, None), None);
    }

    #[test]
    fn generate_id_matches_go_local_nanos_prefix() {
        let value = generate_id();

        assert!(value.starts_with("local-"));
        assert!(value["local-".len()..].parse::<u128>().is_ok());
    }
}
