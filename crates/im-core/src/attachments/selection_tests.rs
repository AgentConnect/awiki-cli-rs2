use super::*;

#[test]
fn group_e2ee_authorizes_with_opaque_original_message_id() {
    for original in [
        "vendor-message-15",
        "e84f4a31-289c-466f-ae4f-695d15560897",
        "did:example:group:15",
        "did:example:group:01",
        "did:example:group:opaque",
    ] {
        assert_eq!(
            attachment_authorization_message_id(
                "group-e2ee",
                "did:example:group:15",
                "local-message",
                original
            ),
            original,
        );
    }
}

#[test]
fn group_e2ee_missing_original_id_falls_back_to_complete_local_id() {
    for missing in ["", " "] {
        assert_eq!(
            attachment_authorization_message_id(
                "group-e2ee",
                "did:example:group:15",
                "message",
                missing
            ),
            "did:example:group:15"
        );
    }
}

#[test]
fn other_profiles_preserve_original_id_and_existing_fallback() {
    for profile in ["direct-e2ee", "transport-protected"] {
        assert_eq!(
            attachment_authorization_message_id(
                profile,
                "view-message",
                "local-message",
                "wire-message"
            ),
            "wire-message"
        );
        assert_eq!(
            attachment_authorization_message_id(profile, "view-message", "message", ""),
            "message"
        );
    }
}
