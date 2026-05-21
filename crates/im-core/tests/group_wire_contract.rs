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

#[test]
fn group_read_rpc_params_match_go_local_contract() {
    let sender = "did:wba:awiki.ai:user:alice:e1_alice";
    let group = "did:wba:awiki.ai:groups:demo:e1_group";

    let get = compat::wire::build_group_get_rpc_params(sender, group).expect("group get params");
    assert_eq!(get["meta"]["profile"], "anp.group.local.v1");
    assert_eq!(get["meta"]["security_profile"], "transport-protected");
    assert_eq!(get["meta"]["sender_did"], sender);
    assert_eq!(
        get["meta"]["target"],
        json!({"kind": "group", "did": group})
    );
    assert_eq!(get["body"]["group_did"], group);
    assert!(get.get("auth").is_none());

    let list = compat::wire::build_group_list_rpc_params(sender, 25);
    assert_eq!(list["meta"]["profile"], "anp.group.local.v1");
    assert_eq!(list["meta"].get("target"), None);
    assert_eq!(list["body"]["limit"], 25);

    let members =
        compat::wire::build_group_members_rpc_params(sender, group, 10).expect("members params");
    assert_eq!(
        members["meta"]["target"],
        json!({"kind": "group", "did": group})
    );
    assert_eq!(members["body"]["group_did"], group);
    assert_eq!(members["body"]["limit"], 10);

    let messages = compat::wire::build_group_messages_rpc_params(sender, group, 5, Some("42"), 2)
        .expect("messages params");
    assert_eq!(
        messages["meta"]["target"],
        json!({"kind": "group", "did": group})
    );
    assert_eq!(messages["body"]["group_did"], group);
    assert_eq!(messages["body"]["limit"], 5);
    assert_eq!(messages["body"]["since_seq"], "42");
    assert_eq!(messages["body"]["skip"], 2);
}
