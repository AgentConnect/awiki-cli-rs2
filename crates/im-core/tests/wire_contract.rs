use im_core::compat;
use im_core::prelude::*;

#[test]
fn wire_content_type_for_message_kind_matches_p1_contract() {
    assert_eq!(
        compat::wire::content_type_for_message_kind(MessageKind::Text, None),
        "text/plain"
    );
    assert_eq!(
        compat::wire::content_type_for_message_kind(MessageKind::Markdown, None),
        "text/markdown"
    );
    assert_eq!(
        compat::wire::content_type_for_message_kind(MessageKind::Text, Some("event")),
        "application/json"
    );
    assert_eq!(
        compat::wire::content_type_for_message_kind(MessageKind::Text, Some("attachment_manifest")),
        "application/anp-attachment-manifest+json"
    );
}

#[test]
fn wire_operation_id_is_lower_hex_without_prefix() {
    let first = compat::wire::generate_operation_id();
    let second = compat::wire::generate_operation_id();

    assert_eq!(first.len(), 16);
    assert_eq!(second.len(), 16);
    assert_ne!(first, second);
    assert!(first.chars().all(|ch| ch.is_ascii_hexdigit()));
    assert!(first.chars().all(|ch| !ch.is_ascii_uppercase()));
}

#[test]
fn wire_now_rfc3339_uses_second_precision_utc_shape() {
    let value = compat::wire::now_rfc3339();
    let bytes = value.as_bytes();

    assert_eq!(bytes.len(), 20);
    assert_eq!(bytes[4], b'-');
    assert_eq!(bytes[7], b'-');
    assert_eq!(bytes[10], b'T');
    assert_eq!(bytes[13], b':');
    assert_eq!(bytes[16], b':');
    assert_eq!(bytes[19], b'Z');
    assert!(bytes
        .iter()
        .enumerate()
        .filter(|(index, _)| !matches!(index, 4 | 7 | 10 | 13 | 16 | 19))
        .all(|(_, byte)| byte.is_ascii_digit()));
}
