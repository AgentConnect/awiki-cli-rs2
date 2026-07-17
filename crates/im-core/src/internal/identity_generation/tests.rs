use super::build_agent_anp_message_service;

#[test]
fn agent_message_service_advertises_group_profile() {
    let service = build_agent_anp_message_service(
        "https://community.example/anp-im/rpc",
        "did:wba:community.example",
    )
    .expect("message service entry should build");

    assert_eq!(service["type"], "ANPMessageService");
    assert_eq!(
        service["serviceEndpoint"],
        "https://community.example/anp-im/rpc"
    );
    assert_eq!(service["serviceDid"], "did:wba:community.example");
    assert_eq!(
        service["profiles"],
        serde_json::json!([
            "anp.core.binding.v1",
            "anp.direct.base.v1",
            "anp.group.base.v1",
            "anp.attachment.v1"
        ])
    );
}
