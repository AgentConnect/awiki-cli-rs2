use std::path::Path;
use std::time::Duration;

use awiki_deamon::cli_wrapper::CliWrapperRequest;
use awiki_deamon::local_rpc::{
    execute_runtime_rpc_request, execute_runtime_rpc_request_with_outbox, RuntimeRpcDebug,
    RuntimeRpcRequest,
};
use awiki_deamon::outbox::{MemoryRuntimeOutbox, OutboxRecordKind, RuntimeMessageSecurity};
use awiki_deamon::runtime::{RuntimeAgentProfile, RuntimeRun, RuntimeRunStatus, RuntimeTask};
use awiki_deamon::security::runtime_token::{
    current_time_millis, issue_runtime_token, RpcMethod, RuntimeTokenScope,
};
use awiki_deamon::workspace::WorkspaceMode;
use awiki_deamon::{DaemonConfig, DaemonState};
use rusqlite::Connection;
use serde_json::json;
use sha2::{Digest, Sha256};

fn fixture() -> (tempfile::TempDir, DaemonState) {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    (root, state)
}

fn issue(
    state: &DaemonState,
    methods: Vec<RpcMethod>,
    recipients: Option<Vec<String>>,
) -> awiki_deamon::security::runtime_token::IssuedRuntimeToken {
    let mut scope = RuntimeTokenScope::new(
        "did:agent:test",
        "profile_1",
        "run_1",
        methods,
        recipients,
        Duration::from_secs(60),
    )
    .unwrap();
    scope.allowed_message_security = Some(vec![
        "default_plain".to_string(),
        "direct_e2ee".to_string(),
        "group_e2ee".to_string(),
    ]);
    let issued = issue_runtime_token(scope).unwrap();
    state.store_runtime_token(&issued).unwrap();
    issued
}

fn insert_runtime_task_context(
    state: &DaemonState,
    agent_did: &str,
    run_id: &str,
    workspace_root: Option<&Path>,
) {
    if let Some(workspace_root) = workspace_root {
        state
            .upsert_runtime_agent_profile(&RuntimeAgentProfile {
                agent_did: agent_did.to_string(),
                controller_did: "did:human:controller".to_string(),
                runtime_profile_id: "profile_1".to_string(),
                runtime_plugin_id: "test-runtime".to_string(),
                display_name: Some("Test Runtime".to_string()),
                workspace_id: Some("workspace_test".to_string()),
                workspace_root: Some(workspace_root.to_path_buf()),
                workspace_mode: Some(WorkspaceMode::SharedRoot),
            })
            .unwrap();
    }
    state
        .insert_runtime_task(&RuntimeTask {
            task_id: "task_attachment".to_string(),
            agent_did: agent_did.to_string(),
            controller_did: "did:human:controller".to_string(),
            sender_did: "did:human:controller".to_string(),
            conversation_id: Some("dm:controller-agent".to_string()),
            text: "send attachment".to_string(),
        })
        .unwrap();
    state
        .insert_runtime_run(&RuntimeRun {
            run_id: run_id.to_string(),
            task_id: "task_attachment".to_string(),
            agent_did: agent_did.to_string(),
            runtime_profile_id: "profile_1".to_string(),
            runtime_plugin_id: "test-runtime".to_string(),
            workspace_id: workspace_root.map(|_| "workspace_test".to_string()),
            status: RuntimeRunStatus::Pending,
        })
        .unwrap();
}

#[test]
fn valid_token_authorizes_allowed_method_and_uses_token_context() {
    let (_root, state) = fixture();
    let issued = issue(&state, vec![RpcMethod::TaskStatus], None);
    let response = execute_runtime_rpc_request(
        &state,
        RuntimeRpcRequest {
            runtime_rpc_token: issued.token.as_str().to_string(),
            method: "task.status".to_string(),
            params: json!({ "state": "running" }),
            debug: Some(RuntimeRpcDebug {
                agent_did: Some("did:agent:spoofed".to_string()),
                run_id: Some("run_spoofed".to_string()),
            }),
        },
    )
    .unwrap();

    assert!(response.ok);
    let result = response.result.unwrap();
    assert_eq!(result["agent_did"], "did:agent:test");
    assert_eq!(result["run_id"], "run_1");
    assert_eq!(result["method"], "task.status");
}

#[test]
fn method_and_recipient_scope_are_enforced() {
    let (_root, state) = fixture();
    let issued = issue(
        &state,
        vec![RpcMethod::MsgSend],
        Some(vec!["@alice".to_string()]),
    );

    let method_error = execute_runtime_rpc_request(
        &state,
        RuntimeRpcRequest {
            runtime_rpc_token: issued.token.as_str().to_string(),
            method: "task.finish".to_string(),
            params: json!({}),
            debug: None,
        },
    )
    .unwrap_err();
    assert!(method_error.to_string().contains("method not allowed"));

    let outbox = MemoryRuntimeOutbox::default();
    let recipient_error = execute_runtime_rpc_request_with_outbox(
        &state,
        &outbox,
        RuntimeRpcRequest {
            runtime_rpc_token: issued.token.as_str().to_string(),
            method: "msg.send".to_string(),
            params: json!({ "to": "did:human:bob", "text": "hello" }),
            debug: None,
        },
    )
    .unwrap_err();
    assert!(recipient_error
        .to_string()
        .contains("recipient not allowed"));
    assert!(outbox.records().is_empty());
}

#[test]
fn msg_send_requires_outbox_execution_path() {
    let (_root, state) = fixture();
    let issued = issue(
        &state,
        vec![RpcMethod::MsgSend],
        Some(vec!["did:human:alice".to_string()]),
    );

    let error = execute_runtime_rpc_request(
        &state,
        RuntimeRpcRequest {
            runtime_rpc_token: issued.token.as_str().to_string(),
            method: "msg.send".to_string(),
            params: json!({ "to": "did:human:alice", "text": "hello" }),
            debug: None,
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("require runtime outbox"));
}

#[test]
fn msg_send_requires_recipient_text_and_supported_security() {
    let (_root, state) = fixture();
    let issued = issue(&state, vec![RpcMethod::MsgSend], None);
    let outbox = MemoryRuntimeOutbox::default();

    let missing_recipient = execute_runtime_rpc_request_with_outbox(
        &state,
        &outbox,
        RuntimeRpcRequest {
            runtime_rpc_token: issued.token.as_str().to_string(),
            method: "msg.send".to_string(),
            params: json!({ "text": "hello" }),
            debug: None,
        },
    )
    .unwrap_err();
    assert!(missing_recipient.to_string().contains("to or group"));

    let missing_text = execute_runtime_rpc_request_with_outbox(
        &state,
        &outbox,
        RuntimeRpcRequest {
            runtime_rpc_token: issued.token.as_str().to_string(),
            method: "msg.send".to_string(),
            params: json!({ "to": "did:human:alice", "text": "   " }),
            debug: None,
        },
    )
    .unwrap_err();
    assert!(missing_text.to_string().contains("text"));

    let unsupported_security = execute_runtime_rpc_request_with_outbox(
        &state,
        &outbox,
        RuntimeRpcRequest {
            runtime_rpc_token: issued.token.as_str().to_string(),
            method: "msg.send".to_string(),
            params: json!({
                "to": "did:human:alice",
                "text": "hello",
                "security": "unsupported_secure_mode"
            }),
            debug: None,
        },
    )
    .unwrap_err();
    assert!(unsupported_security.to_string().contains("unsupported"));
    assert!(outbox.records().is_empty());
}

#[test]
fn msg_send_records_direct_message_side_effect_with_security_mode() {
    let (_root, state) = fixture();
    let issued = issue(
        &state,
        vec![RpcMethod::MsgSend],
        Some(vec!["did:human:alice".to_string()]),
    );
    let outbox = MemoryRuntimeOutbox::default();
    let response = execute_runtime_rpc_request_with_outbox(
        &state,
        &outbox,
        RuntimeRpcRequest {
            runtime_rpc_token: issued.token.as_str().to_string(),
            method: "msg.send".to_string(),
            params: json!({
                "to": "did:human:alice",
                "text": "hello from Hermes",
                "security": "direct_e2ee",
                "agent_did": "did:agent:spoofed",
                "run_id": "run_spoofed"
            }),
            debug: Some(RuntimeRpcDebug {
                agent_did: Some("did:agent:spoofed".to_string()),
                run_id: Some("run_spoofed".to_string()),
            }),
        },
    )
    .unwrap();

    assert!(response.ok);
    let result = response.result.unwrap();
    assert_eq!(result["agent_did"], "did:agent:test");
    assert_eq!(result["run_id"], "run_1");
    assert_eq!(result["method"], "msg.send");

    let records = outbox.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].kind, OutboxRecordKind::Message);
    assert_eq!(records[0].recipient.as_deref(), Some("did:human:alice"));
    assert_eq!(records[0].raw_recipient.as_deref(), Some("did:human:alice"));
    assert_eq!(records[0].resolved_did.as_deref(), Some("did:human:alice"));
    assert!(records[0].message_id.is_some());
    assert_eq!(records[0].text.as_deref(), Some("hello from Hermes"));
    assert_eq!(
        records[0].security,
        Some(RuntimeMessageSecurity::DirectE2ee)
    );
}

#[test]
fn msg_send_records_direct_attachment_in_single_outbound_message() {
    let (root, state) = fixture();
    let workspace_root = root.path().join("workspace");
    std::fs::create_dir_all(&workspace_root).unwrap();
    insert_runtime_task_context(&state, "did:agent:test", "run_1", Some(&workspace_root));
    let issued = issue(
        &state,
        vec![RpcMethod::MsgSend],
        Some(vec!["did:human:alice".to_string()]),
    );
    let outbox = MemoryRuntimeOutbox::default();
    let file_path = workspace_root.join("report.txt");
    std::fs::write(&file_path, "small report").unwrap();

    let response = execute_runtime_rpc_request_with_outbox(
        &state,
        &outbox,
        RuntimeRpcRequest {
            runtime_rpc_token: issued.token.as_str().to_string(),
            method: "msg.send".to_string(),
            params: json!({
                "to": "did:human:alice",
                "text": "report caption",
                "file_path": file_path,
                "display_filename": "report.txt",
                "mime_type": "text/plain",
                "security": "direct_e2ee"
            }),
            debug: None,
        },
    )
    .unwrap();

    assert!(response.ok);
    let records = outbox.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].kind, OutboxRecordKind::Message);
    assert_eq!(records[0].recipient.as_deref(), Some("did:human:alice"));
    assert_eq!(records[0].text.as_deref(), Some("report caption"));
    assert_eq!(records[0].display_filename.as_deref(), Some("report.txt"));
    assert_eq!(records[0].mime_type.as_deref(), Some("text/plain"));
    assert!(records[0].file_path.is_some());
    assert_eq!(
        records[0].security,
        Some(RuntimeMessageSecurity::DirectE2ee)
    );
}

#[test]
fn msg_send_records_group_text_and_attachment_messages() {
    let (root, state) = fixture();
    let workspace_root = root.path().join("workspace");
    std::fs::create_dir_all(&workspace_root).unwrap();
    insert_runtime_task_context(&state, "did:agent:test", "run_1", Some(&workspace_root));
    let issued = issue(
        &state,
        vec![RpcMethod::MsgSend],
        Some(vec!["did:group:team".to_string()]),
    );
    let outbox = MemoryRuntimeOutbox::default();

    execute_runtime_rpc_request_with_outbox(
        &state,
        &outbox,
        RuntimeRpcRequest {
            runtime_rpc_token: issued.token.as_str().to_string(),
            method: "msg.send".to_string(),
            params: json!({
                "group": "did:group:team",
                "text": "hello group",
                "security": "group_e2ee"
            }),
            debug: None,
        },
    )
    .unwrap();

    let file_path = workspace_root.join("group-report.txt");
    std::fs::write(&file_path, "group report").unwrap();
    execute_runtime_rpc_request_with_outbox(
        &state,
        &outbox,
        RuntimeRpcRequest {
            runtime_rpc_token: issued.token.as_str().to_string(),
            method: "msg.send".to_string(),
            params: json!({
                "group": "did:group:team",
                "text": "group attachment caption",
                "file_path": file_path,
                "display_filename": "group-report.txt",
                "mime_type": "text/plain",
                "security": "group_e2ee"
            }),
            debug: None,
        },
    )
    .unwrap();

    let records = outbox.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].recipient.as_deref(), Some("did:group:team"));
    assert_eq!(records[0].raw_recipient.as_deref(), Some("did:group:team"));
    assert_eq!(records[0].resolved_did, None);
    assert_eq!(records[0].text.as_deref(), Some("hello group"));
    assert_eq!(records[0].security, Some(RuntimeMessageSecurity::GroupE2ee));
    assert!(records[0].file_path.is_none());

    assert_eq!(records[1].recipient.as_deref(), Some("did:group:team"));
    assert_eq!(records[1].text.as_deref(), Some("group attachment caption"));
    assert_eq!(
        records[1].display_filename.as_deref(),
        Some("group-report.txt")
    );
    assert!(records[1].file_path.is_some());
    assert_eq!(records[1].security, Some(RuntimeMessageSecurity::GroupE2ee));
}

#[test]
fn msg_send_allows_authorized_non_controller_did_and_records_send_audit() {
    let (root, state) = fixture();
    let issued = issue(
        &state,
        vec![RpcMethod::MsgSend],
        Some(vec!["did:human:bob".to_string()]),
    );
    let outbox = MemoryRuntimeOutbox::default();

    execute_runtime_rpc_request_with_outbox(
        &state,
        &outbox,
        RuntimeRpcRequest {
            runtime_rpc_token: issued.token.as_str().to_string(),
            method: "msg.send".to_string(),
            params: json!({
                "to": "did:human:bob",
                "text": "hello bob"
            }),
            debug: None,
        },
    )
    .unwrap();

    assert_eq!(outbox.records().len(), 1);
    let connection = Connection::open(root.path().join("daemon.db")).unwrap();
    let audit_dump: String = connection
        .query_row(
            "SELECT token_id || ' ' || COALESCE(detail_json, '') FROM audit_log WHERE event_type = 'runtime.msg_send.sent' ORDER BY created_at_ms DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(audit_dump.contains(&issued.token_id));
    assert!(audit_dump.contains("did:human:bob"));
    assert!(audit_dump.contains("default_plain"));
    assert!(!audit_dump.contains(issued.token.as_str()));
}

#[test]
fn msg_send_authorizes_raw_handle_after_resolve_to_did() {
    let (root, state) = fixture();
    let issued = issue(
        &state,
        vec![RpcMethod::MsgSend],
        Some(vec!["@bob".to_string()]),
    );
    let outbox =
        MemoryRuntimeOutbox::default().with_handle_resolution("@bob", "did:human:bob-resolved");

    execute_runtime_rpc_request_with_outbox(
        &state,
        &outbox,
        RuntimeRpcRequest {
            runtime_rpc_token: issued.token.as_str().to_string(),
            method: "msg.send".to_string(),
            params: json!({
                "to": "@bob",
                "text": "hello handle"
            }),
            debug: None,
        },
    )
    .unwrap();

    let records = outbox.records();
    assert_eq!(
        records[0].recipient.as_deref(),
        Some("did:human:bob-resolved")
    );
    assert_eq!(records[0].raw_recipient.as_deref(), Some("@bob"));
    assert_eq!(
        records[0].resolved_did.as_deref(),
        Some("did:human:bob-resolved")
    );

    let connection = Connection::open(root.path().join("daemon.db")).unwrap();
    let authorize_dump: String = connection
        .query_row(
            "SELECT COALESCE(detail_json, '') FROM audit_log WHERE event_type = 'runtime_rpc.authorize' AND detail_json LIKE '%recipient_candidates%' ORDER BY created_at_ms DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(authorize_dump.contains("@bob"));
    assert!(authorize_dump.contains("did:human:bob-resolved"));
}

#[test]
fn msg_send_rejects_unresolved_or_unauthorized_handle_without_side_effect() {
    let (_root, state) = fixture();
    let issued = issue(
        &state,
        vec![RpcMethod::MsgSend],
        Some(vec!["@alice".to_string()]),
    );
    let outbox = MemoryRuntimeOutbox::default();

    let unresolved = execute_runtime_rpc_request_with_outbox(
        &state,
        &outbox,
        RuntimeRpcRequest {
            runtime_rpc_token: issued.token.as_str().to_string(),
            method: "msg.send".to_string(),
            params: json!({ "to": "@alice", "text": "hello" }),
            debug: None,
        },
    )
    .unwrap_err();
    assert!(unresolved.to_string().contains("could not be resolved"));

    let outbox =
        MemoryRuntimeOutbox::default().with_handle_resolution("@mallory", "did:human:mallory");
    let unauthorized = execute_runtime_rpc_request_with_outbox(
        &state,
        &outbox,
        RuntimeRpcRequest {
            runtime_rpc_token: issued.token.as_str().to_string(),
            method: "msg.send".to_string(),
            params: json!({ "to": "@mallory", "text": "hello" }),
            debug: None,
        },
    )
    .unwrap_err();
    assert!(unauthorized.to_string().contains("recipient not allowed"));
    assert!(outbox.records().is_empty());

    let outbox = MemoryRuntimeOutbox::default().with_handle_resolution_status(
        "@alice",
        "did:human:alice",
        "inactive",
    );
    let inactive = execute_runtime_rpc_request_with_outbox(
        &state,
        &outbox,
        RuntimeRpcRequest {
            runtime_rpc_token: issued.token.as_str().to_string(),
            method: "msg.send".to_string(),
            params: json!({ "to": "@alice", "text": "hello" }),
            debug: None,
        },
    )
    .unwrap_err();
    let inactive_chain = inactive
        .chain()
        .map(|cause| cause.to_string())
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(inactive_chain.contains("handle_not_active"));
    assert!(outbox.records().is_empty());
}

#[test]
fn msg_send_rejects_policy_disallowed_security_without_side_effect() {
    let (_root, state) = fixture();
    let mut scope = RuntimeTokenScope::new(
        "did:agent:test",
        "profile_1",
        "run_1",
        vec![RpcMethod::MsgSend],
        Some(vec!["did:human:alice".to_string()]),
        Duration::from_secs(60),
    )
    .unwrap();
    scope.allowed_message_security = Some(vec!["default_plain".to_string()]);
    let issued = issue_runtime_token(scope).unwrap();
    state.store_runtime_token(&issued).unwrap();
    let outbox = MemoryRuntimeOutbox::default();

    let error = execute_runtime_rpc_request_with_outbox(
        &state,
        &outbox,
        RuntimeRpcRequest {
            runtime_rpc_token: issued.token.as_str().to_string(),
            method: "msg.send".to_string(),
            params: json!({
                "to": "did:human:alice",
                "text": "secure hello",
                "security": "direct_e2ee"
            }),
            debug: None,
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("message security not allowed"));
    assert!(outbox.records().is_empty());
}

#[test]
fn attachment_send_requires_scope_task_context_and_records_attachment_side_effect() {
    let (root, state) = fixture();
    let workspace_root = root.path().join("workspace");
    std::fs::create_dir_all(&workspace_root).unwrap();
    insert_runtime_task_context(&state, "did:agent:test", "run_1", Some(&workspace_root));
    let issued = issue(&state, vec![RpcMethod::SendAttachment], None);
    let outbox = MemoryRuntimeOutbox::default();
    let file_path = workspace_root.join("report.txt");
    std::fs::write(&file_path, "small report").unwrap();

    let response = execute_runtime_rpc_request_with_outbox(
        &state,
        &outbox,
        RuntimeRpcRequest {
            runtime_rpc_token: issued.token.as_str().to_string(),
            method: "attachment.send".to_string(),
            params: json!({
                "target": "current_conversation",
                "file_path": file_path,
                "display_filename": "report.txt",
                "caption": "report"
            }),
            debug: None,
        },
    )
    .unwrap();

    assert!(response.ok);
    assert_eq!(response.result.unwrap()["method"], "attachment.send");
    let records = outbox.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].kind, OutboxRecordKind::Message);
    assert_eq!(
        records[0].recipient.as_deref(),
        Some("did:human:controller")
    );
    assert_eq!(
        records[0].raw_recipient.as_deref(),
        Some("current_conversation")
    );
    assert_eq!(records[0].text.as_deref(), Some("report"));

    let connection = Connection::open(root.path().join("daemon.db")).unwrap();
    let audit_dump: String = connection
        .query_row(
            "SELECT token_id || ' ' || COALESCE(detail_json, '') FROM audit_log WHERE event_type = 'runtime.attachment_send.sent' ORDER BY created_at_ms DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(audit_dump.contains(&issued.token_id));
    assert!(audit_dump.contains("report.txt"));
    let expected_sha = hex_sha256(b"small report");
    assert!(audit_dump.contains(&expected_sha));
    assert!(!audit_dump.contains(file_path.to_string_lossy().as_ref()));
    assert!(!audit_dump.contains(issued.token.as_str()));
}

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[test]
fn attachment_send_rejects_file_outside_runtime_workspace() {
    let (root, state) = fixture();
    let workspace_root = root.path().join("workspace");
    std::fs::create_dir_all(&workspace_root).unwrap();
    insert_runtime_task_context(&state, "did:agent:test", "run_1", Some(&workspace_root));
    let issued = issue(&state, vec![RpcMethod::SendAttachment], None);
    let outbox = MemoryRuntimeOutbox::default();
    let outside_path = root.path().join("outside-report.txt");
    std::fs::write(&outside_path, "outside report").unwrap();

    let error = execute_runtime_rpc_request_with_outbox(
        &state,
        &outbox,
        RuntimeRpcRequest {
            runtime_rpc_token: issued.token.as_str().to_string(),
            method: "attachment.send".to_string(),
            params: json!({
                "target": "current_conversation",
                "file_path": outside_path,
                "display_filename": "outside-report.txt",
                "caption": "outside"
            }),
            debug: None,
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("inside the runtime workspace"));
    assert!(outbox.records().is_empty());
    let connection = Connection::open(root.path().join("daemon.db")).unwrap();
    let sent_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM audit_log WHERE event_type = 'runtime.attachment_send.sent'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(sent_count, 0);
}

#[test]
fn attachment_send_requires_outbox_execution_path() {
    let (_root, state) = fixture();
    let issued = issue(&state, vec![RpcMethod::SendAttachment], None);

    let error = execute_runtime_rpc_request(
        &state,
        RuntimeRpcRequest {
            runtime_rpc_token: issued.token.as_str().to_string(),
            method: "attachment.send".to_string(),
            params: json!({}),
            debug: None,
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("require runtime outbox"));
}

#[test]
fn product_wrapper_rejects_did_recipient_for_send_message() {
    let error = awiki_deamon::cli_wrapper::normalize_product_handle("did:human:alice").unwrap_err();

    assert!(error.to_string().contains("recipient_must_be_handle"));
}

#[test]
fn expired_revoked_and_single_use_tokens_are_rejected() {
    let (_root, state) = fixture();
    let mut expired_scope = RuntimeTokenScope::new(
        "did:agent:test",
        "profile_1",
        "run_expired",
        vec![RpcMethod::TaskStatus],
        None,
        Duration::from_secs(60),
    )
    .unwrap();
    expired_scope.expires_at_ms = current_time_millis().unwrap() - 1;
    let expired = issue_runtime_token(expired_scope).unwrap();
    state.store_runtime_token(&expired).unwrap();
    let expired_error = execute_runtime_rpc_request(
        &state,
        RuntimeRpcRequest {
            runtime_rpc_token: expired.token.as_str().to_string(),
            method: "task.status".to_string(),
            params: json!({}),
            debug: None,
        },
    )
    .unwrap_err();
    assert!(expired_error.to_string().contains("expired"));

    let revoked = issue(&state, vec![RpcMethod::TaskStatus], None);
    state.revoke_runtime_token(&revoked.token_id).unwrap();
    let revoked_error = execute_runtime_rpc_request(
        &state,
        RuntimeRpcRequest {
            runtime_rpc_token: revoked.token.as_str().to_string(),
            method: "task.status".to_string(),
            params: json!({}),
            debug: None,
        },
    )
    .unwrap_err();
    assert!(revoked_error.to_string().contains("revoked"));

    let mut single_use_scope = RuntimeTokenScope::new(
        "did:agent:test",
        "profile_1",
        "run_once",
        vec![RpcMethod::TaskStatus],
        None,
        Duration::from_secs(60),
    )
    .unwrap();
    single_use_scope.single_use = true;
    let single_use = issue_runtime_token(single_use_scope).unwrap();
    state.store_runtime_token(&single_use).unwrap();
    execute_runtime_rpc_request(
        &state,
        RuntimeRpcRequest {
            runtime_rpc_token: single_use.token.as_str().to_string(),
            method: "task.status".to_string(),
            params: json!({}),
            debug: None,
        },
    )
    .unwrap();
    let replay_error = execute_runtime_rpc_request(
        &state,
        RuntimeRpcRequest {
            runtime_rpc_token: single_use.token.as_str().to_string(),
            method: "task.status".to_string(),
            params: json!({}),
            debug: None,
        },
    )
    .unwrap_err();
    assert!(replay_error.to_string().contains("already used"));
}

#[test]
fn audit_records_token_id_not_token_secret() {
    let (root, state) = fixture();
    let issued = issue(&state, vec![RpcMethod::TaskStatus], None);
    execute_runtime_rpc_request(
        &state,
        RuntimeRpcRequest {
            runtime_rpc_token: issued.token.as_str().to_string(),
            method: "task.status".to_string(),
            params: json!({}),
            debug: None,
        },
    )
    .unwrap();

    let connection = Connection::open(root.path().join("daemon.db")).unwrap();
    let audit_dump: String = connection
        .query_row(
            "SELECT token_id || ' ' || COALESCE(detail_json, '') FROM audit_log ORDER BY created_at_ms DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(audit_dump.contains(&issued.token_id));
    assert!(!audit_dump.contains(issued.token.as_str()));

    let stored_token: String = connection
        .query_row(
            "SELECT token_secret_hash FROM runtime_rpc_tokens WHERE token_id = ?1",
            [&issued.token_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_ne!(stored_token, issued.token.as_str());
}

#[test]
fn cli_wrapper_request_only_uses_token_as_authorization_material() {
    let request = CliWrapperRequest::msg_send_with_security(
        "rtok_test_secret_value_123456789",
        "@alice",
        "hi",
        Some("direct_e2ee"),
    )
    .into_rpc_request();

    assert_eq!(
        request.runtime_rpc_token,
        "rtok_test_secret_value_123456789"
    );
    assert_eq!(request.method, "msg.send");
    assert_eq!(request.params["to"], "@alice");
    assert_eq!(request.params["security"], "direct_e2ee");
    assert!(request.debug.is_none());
}

#[test]
fn cli_wrapper_attachment_request_uses_token_and_current_conversation_target() {
    let request = CliWrapperRequest::attachment_send(
        "rtok_test_secret_value_123456789",
        "/tmp/report.txt",
        Some("report.txt"),
        Some("done"),
    )
    .into_rpc_request();

    assert_eq!(
        request.runtime_rpc_token,
        "rtok_test_secret_value_123456789"
    );
    assert_eq!(request.method, "attachment.send");
    assert_eq!(request.params["target"], "current_conversation");
    assert_eq!(request.params["display_filename"], "report.txt");
    assert!(request.debug.is_none());
}

#[test]
fn cli_wrapper_outbound_send_supports_group_attachment_message() {
    let request = CliWrapperRequest::outbound_send(
        "rtok_test_secret_value_123456789",
        awiki_deamon::cli_wrapper::OutboundMessageTarget::Group("did:group:team".to_string()),
        "caption",
        Some("/tmp/group-report.txt"),
        Some("group-report.txt"),
        Some("text/plain"),
        Some("group_e2ee"),
    )
    .into_rpc_request();

    assert_eq!(request.method, "msg.send");
    assert_eq!(request.params["group"], "did:group:team");
    assert_eq!(request.params["text"], "caption");
    assert_eq!(request.params["file_path"], "/tmp/group-report.txt");
    assert_eq!(request.params["display_filename"], "group-report.txt");
    assert_eq!(request.params["mime_type"], "text/plain");
    assert_eq!(request.params["security"], "group_e2ee");
    assert!(request.debug.is_none());
}

#[cfg(unix)]
#[test]
fn uds_server_enforces_permissions_and_handles_one_request() {
    use awiki_deamon::local_rpc::{
        call_uds_once, serve_one_uds_request, verify_socket_permissions,
    };

    let (root, state) = fixture();
    let socket_path = root.path().join("rpc").join("awiki-deamon.sock");
    let issued = issue(&state, vec![RpcMethod::TaskStatus], None);
    let server_state = state.clone();
    let server_socket = socket_path.clone();
    let server = std::thread::spawn(move || serve_one_uds_request(&server_state, &server_socket));

    let mut last_error = None;
    let mut response = None;
    for _ in 0..100 {
        match call_uds_once(
            &socket_path,
            &RuntimeRpcRequest {
                runtime_rpc_token: issued.token.as_str().to_string(),
                method: "task.status".to_string(),
                params: json!({ "state": "running" }),
                debug: None,
            },
        ) {
            Ok(value) => {
                response = Some(value);
                break;
            }
            Err(error) => {
                last_error = Some(error);
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    let response = response.unwrap_or_else(|| panic!("UDS call failed: {last_error:?}"));
    assert!(response.ok);
    server.join().unwrap().unwrap();
    verify_socket_permissions(&socket_path).unwrap();
}
