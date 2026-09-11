use std::path::Path;

use anyhow::Result;
use awiki_deamon::plugins::acp::protocol::{
    cancel_notification, initialize_request, new_session_request, parse_inbound,
    permission_response, prompt_request, AcpInbound, AcpPermissionPolicy, AcpTextAccumulator,
    ACP_PROTOCOL_VERSION,
};
use serde_json::json;

#[test]
fn acp_initialize_advertises_protocol_v1_without_host_capabilities() {
    let request = initialize_request(7);

    assert_eq!(request["jsonrpc"], "2.0");
    assert_eq!(request["id"], 7);
    assert_eq!(request["method"], "initialize");
    assert_eq!(request["params"]["protocolVersion"], ACP_PROTOCOL_VERSION);
    assert_eq!(request["params"]["clientInfo"]["name"], "awiki-daemon");
    assert_eq!(request["params"]["clientCapabilities"]["terminal"], false);
    assert_eq!(
        request["params"]["clientCapabilities"]["auth"]["terminal"],
        false
    );
    assert_eq!(
        request["params"]["clientCapabilities"]["fs"],
        json!({ "readTextFile": false, "writeTextFile": false })
    );
    assert_eq!(
        request["params"]["clientCapabilities"]["positionEncodings"],
        json!([])
    );
    assert!(request["params"]["clientCapabilities"]["elicitation"].is_null());
    assert!(request["params"]["clientCapabilities"]["nes"].is_null());
    assert!(request["params"]["clientCapabilities"]["plan"].is_null());
}

#[test]
fn acp_session_new_requires_absolute_cwd_and_disables_extra_roots() -> Result<()> {
    let request = new_session_request(8, Path::new("/tmp/awiki-acp"))?;

    assert_eq!(request["method"], "session/new");
    assert_eq!(request["params"]["cwd"], "/tmp/awiki-acp");
    assert_eq!(request["params"]["mcpServers"], json!([]));
    assert_eq!(request["params"]["additionalDirectories"], json!([]));

    let error = new_session_request(9, Path::new("relative/workspace")).unwrap_err();
    assert!(error.to_string().contains("absolute"));
    Ok(())
}

#[test]
fn acp_prompt_and_cancel_use_baseline_text_contract() -> Result<()> {
    let prompt = prompt_request(10, "session-1", "hello")?;
    assert_eq!(prompt["method"], "session/prompt");
    assert_eq!(prompt["params"]["sessionId"], "session-1");
    assert_eq!(
        prompt["params"]["prompt"],
        json!([{ "type": "text", "text": "hello" }])
    );

    let cancel = cancel_notification("session-1")?;
    assert_eq!(cancel["method"], "session/cancel");
    assert_eq!(cancel["params"]["sessionId"], "session-1");
    assert!(cancel.get("id").is_none());

    assert!(prompt_request(11, "session-1", "  ").is_err());
    assert!(cancel_notification(" ").is_err());
    Ok(())
}

#[test]
fn acp_updates_accumulate_only_matching_committed_text() -> Result<()> {
    let first = parse_inbound(
        r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"session-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"hello "}}}}"#,
    )?;
    let second = parse_inbound(
        r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"session-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"world"}}}}"#,
    )?;
    let other_session = parse_inbound(
        r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"session-2","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"ignored"}}}}"#,
    )?;

    assert!(matches!(first, AcpInbound::SessionUpdate { .. }));
    let mut accumulator = AcpTextAccumulator::new("session-1");
    assert!(accumulator.observe(&first));
    assert!(accumulator.observe(&second));
    assert!(!accumulator.observe(&other_session));
    assert_eq!(accumulator.final_text(), Some("hello world".to_string()));
    Ok(())
}

#[test]
fn acp_permission_policy_selects_only_an_advertised_one_shot_option() -> Result<()> {
    let request = parse_inbound(
        r#"{"jsonrpc":"2.0","id":91,"method":"session/request_permission","params":{"sessionId":"session-1","toolCall":{"toolCallId":"call-1"},"options":[{"optionId":"allow-once","name":"Allow once","kind":"allow_once"},{"optionId":"reject-once","name":"Reject","kind":"reject_once"}]}}"#,
    )?;

    let allow = permission_response(&request, AcpPermissionPolicy::AllowOnce)?;
    assert_eq!(
        allow,
        json!({
            "jsonrpc": "2.0",
            "id": 91,
            "result": {
                "outcome": { "outcome": "selected", "optionId": "allow-once" }
            }
        })
    );

    let reject = permission_response(&request, AcpPermissionPolicy::RejectOnce)?;
    assert_eq!(reject["result"]["outcome"]["optionId"], "reject-once");

    let unavailable = parse_inbound(
        r#"{"jsonrpc":"2.0","id":"permission-2","method":"session/request_permission","params":{"sessionId":"session-1","toolCall":{"toolCallId":"call-2"},"options":[{"optionId":"reject-once","name":"Reject","kind":"reject_once"}]}}"#,
    )?;
    assert!(permission_response(&unavailable, AcpPermissionPolicy::AllowOnce).is_err());
    Ok(())
}

#[test]
fn acp_parser_correlates_numeric_and_string_response_ids() -> Result<()> {
    let numeric = parse_inbound(r#"{"jsonrpc":"2.0","id":12,"result":{"sessionId":"s"}}"#)?;
    let string =
        parse_inbound(r#"{"jsonrpc":"2.0","id":"13","error":{"code":-32602,"message":"bad"}}"#)?;

    assert!(matches!(
        numeric,
        AcpInbound::Response { ref id, .. } if id == "12"
    ));
    assert!(matches!(
        string,
        AcpInbound::Response { ref id, .. } if id == "13"
    ));
    Ok(())
}
