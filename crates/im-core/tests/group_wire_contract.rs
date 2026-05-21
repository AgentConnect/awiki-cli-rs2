use im_core::compat;
use serde_json::json;

#[test]
fn group_send_payload_matches_go_contract() {
    let group = "did:wba:awiki.ai:groups:demo:e1_group";
    let payload = compat::wire::build_group_send_payload(
        "did:wba:awiki.ai:user:alice:e1_alice",
        group,
        " hello group ",
        "application/json",
    )
    .expect("group send payload");

    assert_eq!(payload.method, "group.send");
    assert_eq!(payload.meta["profile"], "anp.group.base.v1");
    assert_eq!(payload.meta["security_profile"], "transport-protected");
    assert_eq!(
        payload.meta["sender_did"],
        "did:wba:awiki.ai:user:alice:e1_alice"
    );
    assert_eq!(
        payload.meta["target"],
        json!({ "kind": "group", "did": group })
    );
    assert_eq!(payload.meta["content_type"], "application/json");
    assert!(payload.meta["operation_id"]
        .as_str()
        .expect("operation id")
        .starts_with("op-"));
    assert!(payload.meta["message_id"]
        .as_str()
        .expect("message id")
        .starts_with("msg-"));
    assert_eq!(payload.body, json!({ "text": " hello group " }));
}

#[test]
fn group_send_payload_validation_matches_go_boundaries() {
    let err = compat::wire::build_group_send_payload(
        "did:wba:awiki.ai:user:alice:e1_alice",
        "",
        "hello",
        "text/plain",
    )
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "invalid input for group_did: group target is required"
    );

    let err = compat::wire::build_group_send_payload(
        "did:wba:awiki.ai:user:alice:e1_alice",
        "did:wba:awiki.ai:groups:demo:e1_group",
        " ",
        "text/plain",
    )
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "invalid input for text: message text is required"
    );
}
