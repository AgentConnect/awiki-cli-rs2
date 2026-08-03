use super::*;

#[test]
fn recovery_document_has_no_delegated_daemon_method_or_secret() {
    let generated = generate_handle_recovery_identity("example.invalid", "alice", None, None)
        .expect("generate Recovery identity");
    let encoded = serde_json::to_string(&generated.did_document).unwrap();
    assert!(!encoded.contains("daemon-key-1"));
    assert!(!format!("{generated:?}").contains("PRIVATE KEY"));

    let methods = generated.did_document["verificationMethod"]
        .as_array()
        .expect("verification methods");
    assert_eq!(methods.len(), 3);
    assert_eq!(
        generated.did_document["deviceManifest"]["devices"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
