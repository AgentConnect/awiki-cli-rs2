use super::{common::WireIdentity, inbox};

#[test]
fn exact_device_secure_inbox_request_has_closed_direct_e2ee_selector() {
    let params = inbox::build_exact_device_secure_inbox_rpc_params(
        &WireIdentity {
            did: "did:example:alice".to_owned(),
        },
        37,
    );

    assert_eq!(params["meta"]["profile"], "anp.inbox.local.v1");
    assert_eq!(params["meta"]["security_profile"], "transport-protected");
    assert_eq!(params["meta"]["sender_did"], "did:example:alice");
    assert_eq!(params["body"]["user_did"], "did:example:alice");
    assert_eq!(params["body"]["limit"], 37);
    assert_eq!(params["body"]["security_profile"], "direct-e2ee");
    assert_eq!(params["body"].as_object().unwrap().len(), 3);
}

#[test]
fn generic_inbox_request_does_not_gain_secure_selector() {
    let params = inbox::build_inbox_rpc_params(
        &WireIdentity {
            did: "did:example:alice".to_owned(),
        },
        inbox::InboxWireRequest {
            limit: 20,
            auth: None,
        },
    );

    assert!(params["body"].get("security_profile").is_none());
}
