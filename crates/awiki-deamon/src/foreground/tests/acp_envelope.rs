use super::*;

#[tokio::test]
async fn acp_control_validates_the_common_json_envelope_before_dispatch() {
    let (_root, config, state) = fixture();
    // A non-ACP target gives us a deterministic dispatch result without any
    // client process, authentication request or external service.
    let created = setup_daemon_agent(
        &config,
        &state,
        &MockRegistrationClient,
        "envelope-test",
        "did:human:alice",
        RegistrationToken::new("test-registration").unwrap(),
    )
    .unwrap();
    state
        .upsert_runtime_agent_profile(&RuntimeAgentProfile {
            agent_did: created.agent_did.clone(),
            agent_handle: "envelope-test".into(),
            controller_user_id: "user-alice".into(),
            controller_full_handle: "alice.anpclaw.com".into(),
            controller_scope_key: "controller-scope:v1:alice".into(),
            controller_did: "did:human:alice".into(),
            runtime_profile_id: "legacy-placeholder".into(),
            runtime_plugin_id: "generic-cli".into(),
            display_name: None,
            preferred_language: "zh-Hans".into(),
            workspace_id: None,
            workspace_root: None,
            workspace_mode: None,
        })
        .unwrap();
    let im_core = ImCoreAdapter::open(&config).unwrap();
    let registration =
        UserServiceAgentRegistrationClient::new(&config.user_service_base_url).unwrap();
    let client = im_core
        .client_for_agent(&config, &state, &created.agent_did)
        .unwrap();
    for (content_type, expected) in [
        (
            Some("text/plain"),
            "agent payload command must use application/json",
        ),
        (Some(""), "agent payload command must use application/json"),
        (Some("application/json"), "not_acp_runtime"),
        // Omitted metadata retains the existing common JSON default.
        (None, "not_acp_runtime"),
    ] {
        let mut message = plain_direct_message("msg_acp_envelope");
        message.metadata.content_type = content_type.map(str::to_owned);
        message.body = TestMessageBodyView::Payload {
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command": "runtime.acp.control",
                "command_id": "inspect-envelope",
                "args": {"action": "query"}
            }),
        };
        let error = route_message(
            &config,
            &state,
            &im_core,
            &registration,
            &client,
            &created.agent_did,
            &message,
            None,
        )
        .await
        .unwrap_err();
        assert_eq!(error.to_string(), expected, "content_type={content_type:?}");
    }
}
