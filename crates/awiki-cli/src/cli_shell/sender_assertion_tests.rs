use super::*;
#[test]
fn exact_sender_assertion_rejects_identity_switch_before_send() {
    assert!(check_expected_sender("did:wba:example.org:a", "did:wba:example.org:a").is_ok());
    assert!(check_expected_sender("did:wba:example.org:a", "did:wba:example.org:b").is_err());
    assert!(check_expected_sender("", "did:wba:example.org:b").is_ok());
}
