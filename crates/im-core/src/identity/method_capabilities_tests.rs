use super::*;

#[test]
fn product_method_capabilities_preserve_wba_and_limit_web() {
    let wba = identity_method_capabilities("did:wba:example.com:user:legacy").unwrap();
    assert_eq!(wba.method, DidMethod::Wba);
    assert!(wba.handle_recovery && wba.root_import && wba.root_transfer);
    assert!(!wba.services_update);
    let web = identity_method_capabilities("did:web:identity.example:awiki:web:alice").unwrap();
    assert_eq!(web.method, DidMethod::Web);
    assert!(!web.handle_recovery && !web.root_import && !web.root_transfer);
    assert!(web.services_update);
    assert_eq!(serde_json::to_value(web).unwrap()["rootTransfer"], false);
}

#[test]
fn unsupported_or_malformed_method_has_no_product_capability() {
    for did in [
        "did:example:alice",
        "did:wba:",
        "did:web:",
        "did:web:localhost",
        "did:web:example.com:%2E%2E",
        "did:web:example.com#key",
        " did:wba:example.com",
        "did:wba:example.com?",
    ] {
        assert!(identity_method_capabilities(did).is_err(), "{did}");
    }
}
