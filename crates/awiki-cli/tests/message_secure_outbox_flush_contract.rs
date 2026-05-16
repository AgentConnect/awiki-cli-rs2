use awiki_cli::message::{
    build_secure_ack_payload, build_secure_init_payload, compact_warnings,
    current_secure_session_id, flush_queued_secure_outbox_rows_plan, is_pending_confirmation_error,
    is_secure_ack_plaintext, is_secure_init_plaintext, queue_secure_outbox_record,
    secure_ack_session_id, MarkSentOutcome, QueuedSecureOutboxRow, SecureOutboxFlushAction,
    SecureOutboxFlushRowOutcome, SecureOutboxSendOutcome, StoreMessageOutcome,
};
use awiki_cli::{anpsdk, config, identity, store};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn rows_are_stably_sorted_by_created_at_before_flushing() {
    let rows = vec![
        row(
            "outbox-b",
            "did:bob",
            "text",
            "second",
            "2026-05-16T00:00:02Z",
        ),
        row(
            "outbox-a1",
            "did:bob",
            "text",
            "first",
            "2026-05-16T00:00:01Z",
        ),
        row(
            "outbox-a2",
            "did:bob",
            "text",
            "first-stable",
            "2026-05-16T00:00:01Z",
        ),
    ];
    let mut calls = Vec::new();

    let plan = flush_queued_secure_outbox_rows_plan("did:alice", "alice", "", &rows, |row| {
        calls.push(row.outbox_id.clone());
        success_with_message(&row.outbox_id)
    });

    assert_eq!(calls, vec!["outbox-a1", "outbox-a2", "outbox-b"]);
    assert_eq!(
        send_ids(&plan.actions),
        vec!["outbox-a1", "outbox-a2", "outbox-b"]
    );
    assert!(plan.warnings.is_empty());
}

#[test]
fn peer_filter_trims_input_but_compares_row_peer_exactly() {
    let rows = vec![
        row("skip", " did:bob ", "text", "skip", "1"),
        row("flush", "did:bob", "text", "flush", "2"),
        row("other", "did:carol", "text", "other", "3"),
    ];

    let plan =
        flush_queued_secure_outbox_rows_plan("did:alice", "alice", " did:bob ", &rows, |row| {
            success_with_message(&row.outbox_id)
        });

    assert_eq!(send_ids(&plan.actions), vec!["flush"]);
}

#[test]
fn missing_outbox_id_or_peer_did_skips_without_calling_send_or_warning() {
    let rows = vec![
        row("", "did:bob", "text", "missing id", "1"),
        row("missing-peer", "", "text", "missing peer", "2"),
        row("valid", "did:bob", "text", "ok", "3"),
    ];
    let mut calls = Vec::new();

    let plan = flush_queued_secure_outbox_rows_plan("did:alice", "alice", "", &rows, |row| {
        calls.push(row.outbox_id.clone());
        success_with_message(&row.outbox_id)
    });

    assert_eq!(calls, vec!["valid"]);
    assert_eq!(send_ids(&plan.actions), vec!["valid"]);
    assert!(plan.warnings.is_empty());
}

#[test]
fn text_and_blank_original_type_send_text_and_store_text_plain() {
    for original_type in ["text", "", " \t\n"] {
        let plan = flush_queued_secure_outbox_rows_plan(
            "did:alice",
            "alice",
            "",
            &[row("outbox-1", "did:bob", original_type, "hello", "1")],
            |_| success_with_message(""),
        );

        assert!(matches!(
            plan.actions[0],
            SecureOutboxFlushAction::SendText { .. }
        ));
        let record = stored_record(&plan.actions);
        assert_eq!(record.msg_id, "outbox-1");
        assert_eq!(record.content_type, "text/plain");
        assert_eq!(record.content, "hello");
        assert_eq!(
            record.thread_id,
            awiki_cli::store::make_thread_id("did:alice", "did:bob", "")
        );
    }
}

#[test]
fn json_original_type_parses_payload_and_preserves_go_text_plain_storage() {
    let plan = flush_queued_secure_outbox_rows_plan(
        "did:alice",
        "alice",
        "",
        &[row("json-1", "did:bob", "json", r#"{"hello":true}"#, "1")],
        |_| success_with_message("msg-json"),
    );

    match &plan.actions[0] {
        SecureOutboxFlushAction::SendJson { payload, .. } => {
            assert_eq!(
                payload,
                &Map::from_iter([("hello".to_string(), json!(true))])
            );
        }
        other => panic!("expected SendJson, got {other:?}"),
    }
    let record = stored_record(&plan.actions);
    assert_eq!(record.msg_id, "msg-json");
    assert_eq!(record.content_type, "text/plain");
    assert_eq!(record.content, r#"{"hello":true}"#);
}

#[test]
fn invalid_json_sets_drop_failure_and_warning_then_continues() {
    let rows = vec![
        row("bad-json", "did:bob", "json", "{bad", "1"),
        row("good", "did:bob", "text", "ok", "2"),
    ];

    let plan = flush_queued_secure_outbox_rows_plan("did:alice", "alice", "", &rows, |row| {
        success_with_message(&row.outbox_id)
    });

    assert!(matches!(
        &plan.actions[0],
        SecureOutboxFlushAction::SetOutboxFailure {
            outbox_id,
            error_code,
            retry_hint,
            metadata,
        } if outbox_id == "bad-json"
            && error_code == "invalid_payload"
            && retry_hint == "drop"
            && metadata.contains("\"detail\"")
    ));
    assert_eq!(send_ids(&plan.actions), vec!["good"]);
    assert_eq!(plan.warnings.len(), 1);
    assert!(plan.warnings[0].starts_with("Failed to parse queued secure JSON payload bad-json:"));
}

#[test]
fn unsupported_original_type_sets_drop_failure_and_warning_then_continues() {
    let rows = vec![
        row("bad-type", "did:bob", "image", "ignored", "1"),
        row("good", "did:bob", "text", "ok", "2"),
    ];

    let plan = flush_queued_secure_outbox_rows_plan("did:alice", "alice", "", &rows, |row| {
        success_with_message(&row.outbox_id)
    });

    assert_eq!(
        plan.actions[0],
        SecureOutboxFlushAction::SetOutboxFailure {
            outbox_id: "bad-type".to_string(),
            error_code: "unsupported_original_type".to_string(),
            retry_hint: "drop".to_string(),
            metadata: json!({"original_type": "image"}).to_string(),
        }
    );
    assert_eq!(
        plan.warnings,
        vec!["Queued secure outbox bad-type uses unsupported original_type=image"]
    );
    assert_eq!(send_ids(&plan.actions), vec!["good"]);
}

#[test]
fn invalid_json_and_unsupported_type_do_not_request_send_outcomes() {
    let rows = vec![
        row("bad-json", "did:bob", "json", "{bad", "1"),
        row("bad-type", "did:bob", "image", "ignored", "2"),
        row("good", "did:bob", "text", "ok", "3"),
    ];
    let mut calls = Vec::new();

    let plan = flush_queued_secure_outbox_rows_plan("did:alice", "alice", "", &rows, |row| {
        calls.push(row.outbox_id.clone());
        success_with_message(&row.outbox_id)
    });

    assert_eq!(calls, vec!["good"]);
    assert_eq!(send_ids(&plan.actions), vec!["good"]);
}

#[test]
fn send_error_sets_retry_failure_and_skips_mark_sent_and_store() {
    let plan = flush_queued_secure_outbox_rows_plan(
        "did:alice",
        "alice",
        "",
        &[row("outbox-1", "did:bob", "text", "hello", "1")],
        |_| SecureOutboxFlushRowOutcome {
            send: SecureOutboxSendOutcome::Error("network down".to_string()),
            ..SecureOutboxFlushRowOutcome::default()
        },
    );

    assert_eq!(
        plan.actions[1],
        SecureOutboxFlushAction::SetOutboxFailure {
            outbox_id: "outbox-1".to_string(),
            error_code: "send_failed".to_string(),
            retry_hint: "retry".to_string(),
            metadata: json!({"detail": "network down"}).to_string(),
        }
    );
    assert!(!plan
        .actions
        .iter()
        .any(|action| matches!(action, SecureOutboxFlushAction::MarkOutboxSent { .. })));
    assert_eq!(
        plan.warnings,
        vec!["Failed to flush queued secure outbox outbox-1: network down"]
    );
}

#[test]
fn success_marks_sent_with_metadata_and_stores_e2ee_message_record() {
    let plan = flush_queued_secure_outbox_rows_plan(
        "did:alice",
        "alice",
        "",
        &[row("outbox-1", "did:bob", "text", "hello", "1")],
        |_| SecureOutboxFlushRowOutcome {
            send: SecureOutboxSendOutcome::Success {
                message_id: "msg-remote".to_string(),
                operation_id: "op-1".to_string(),
                delivery_state: "accepted".to_string(),
                accepted_at: "2026-04-23T10:00:00Z".to_string(),
            },
            session_id: "sess-1".to_string(),
            mark_sent: MarkSentOutcome::Success,
            store_message: StoreMessageOutcome::Success,
        },
    );

    let metadata = json!({
        "target_did": "did:bob",
        "operation_id": "op-1",
        "delivery_state": "accepted",
        "flushed_from": "queued",
    })
    .to_string();
    assert_eq!(
        plan.actions[1],
        SecureOutboxFlushAction::MarkOutboxSent {
            outbox_id: "outbox-1".to_string(),
            session_id: "sess-1".to_string(),
            sent_msg_id: "msg-remote".to_string(),
            metadata: metadata.clone(),
        }
    );
    let record = stored_record(&plan.actions);
    assert_eq!(record.msg_id, "msg-remote");
    assert_eq!(record.owner_did, "did:alice");
    assert_eq!(record.direction, 1);
    assert_eq!(record.sender_did, "did:alice");
    assert_eq!(record.receiver_did, "did:bob");
    assert_eq!(record.sent_at, "2026-04-23T10:00:00Z");
    assert!(record.is_read);
    assert!(record.is_e2ee);
    assert_eq!(record.metadata, metadata);
    assert_eq!(record.credential_name, "alice");
    assert!(plan.warnings.is_empty());
}

#[test]
fn blank_sent_message_id_falls_back_to_outbox_id() {
    let plan = flush_queued_secure_outbox_rows_plan(
        "did:alice",
        "alice",
        "",
        &[row("outbox-1", "did:bob", "text", "hello", "1")],
        |_| success_with_message(""),
    );

    assert!(matches!(
        &plan.actions[1],
        SecureOutboxFlushAction::MarkOutboxSent { sent_msg_id, .. }
            if sent_msg_id == "outbox-1"
    ));
    assert_eq!(stored_record(&plan.actions).msg_id, "outbox-1");
}

#[test]
fn mark_sent_error_warns_and_skips_store_message() {
    let plan = flush_queued_secure_outbox_rows_plan(
        "did:alice",
        "alice",
        "",
        &[row("outbox-1", "did:bob", "text", "hello", "1")],
        |_| SecureOutboxFlushRowOutcome {
            mark_sent: MarkSentOutcome::Error("db busy".to_string()),
            ..success_with_message("msg-1")
        },
    );

    assert!(plan
        .actions
        .iter()
        .any(|action| matches!(action, SecureOutboxFlushAction::MarkOutboxSent { .. })));
    assert!(!plan
        .actions
        .iter()
        .any(|action| matches!(action, SecureOutboxFlushAction::StoreMessage { .. })));
    assert_eq!(
        plan.warnings,
        vec!["Failed to mark secure outbox outbox-1 sent: db busy"]
    );
}

#[test]
fn store_message_error_warns_after_store_attempt() {
    let plan = flush_queued_secure_outbox_rows_plan(
        "did:alice",
        "alice",
        "",
        &[row("outbox-1", "did:bob", "text", "hello", "1")],
        |_| SecureOutboxFlushRowOutcome {
            store_message: StoreMessageOutcome::Error("constraint".to_string()),
            ..success_with_message("msg-1")
        },
    );

    assert!(plan
        .actions
        .iter()
        .any(|action| matches!(action, SecureOutboxFlushAction::StoreMessage { .. })));
    assert_eq!(
        plan.warnings,
        vec!["Failed to persist flushed secure outbox outbox-1: constraint"]
    );
}

#[test]
fn compact_warnings_trims_deduplicates_and_drops_empty_values() {
    assert_eq!(
        compact_warnings(vec![
            " warning ".to_string(),
            "".to_string(),
            "warning".to_string(),
            "\tother\n".to_string(),
            " ".to_string(),
        ]),
        vec!["warning".to_string(), "other".to_string()]
    );
    assert!(compact_warnings(vec![" ".to_string(), "".to_string()]).is_empty());
}

#[test]
fn secure_ack_payload_trims_session_and_acked_message_ids() {
    assert_eq!(
        Value::Object(build_secure_ack_payload(" session-1 \n", "\tmsg-9 ")),
        json!({
            "system_type": "awiki.direct.secure_ack.v1",
            "session_id": "session-1",
            "acked_message_id": "msg-9",
        })
    );
}

#[test]
fn secure_init_payload_matches_go_manual_init_control_payload() {
    assert_eq!(
        Value::Object(build_secure_init_payload()),
        json!({
            "system_type": "awiki.direct.secure_init.v1",
            "reason": "manual_init",
        })
    );
}

#[test]
fn secure_control_plaintext_detection_accepts_matching_json_object_payloads() {
    assert!(is_secure_ack_plaintext(&plaintext_with_payload(json!({
        "system_type": "awiki.direct.secure_ack.v1",
        "session_id": "session-1",
        "acked_message_id": "msg-9",
    }))));
    assert!(is_secure_init_plaintext(&plaintext_with_payload(json!({
        "system_type": "awiki.direct.secure_init.v1",
        "reason": "manual_init",
    }))));
    assert!(is_secure_ack_plaintext(&plaintext_with_payload(json!(
        r#"{"system_type":"awiki.direct.secure_ack.v1","session_id":"session-from-string"}"#
    ))));
}

#[test]
fn secure_control_plaintext_detection_rejects_non_matching_shapes() {
    let valid_ack_payload = json!({
        "system_type": "awiki.direct.secure_ack.v1",
        "session_id": "session-1",
        "acked_message_id": "msg-9",
    });

    let mut missing_content_type = plaintext_with_payload(valid_ack_payload.clone());
    missing_content_type.remove("application_content_type");
    assert!(!is_secure_ack_plaintext(&missing_content_type));

    let mut wrong_content_type = plaintext_with_payload(valid_ack_payload.clone());
    wrong_content_type.insert("application_content_type".to_string(), json!("text/plain"));
    assert!(!is_secure_ack_plaintext(&wrong_content_type));

    assert!(!is_secure_ack_plaintext(&plaintext_with_payload(json!(
        "not-an-object"
    ))));
    assert!(is_secure_ack_plaintext(&plaintext_with_payload(json!(
        r#"{"system_type":"awiki.direct.secure_ack.v1"}"#
    ))));
    assert!(!is_secure_ack_plaintext(&plaintext_with_payload(json!({
        "system_type": 42,
        "session_id": "session-1",
        "acked_message_id": "msg-9",
    }))));
    assert!(!is_secure_ack_plaintext(&plaintext_with_payload(json!({
        "system_type": "awiki.direct.secure_init.v1",
        "reason": "manual_init",
    }))));
    assert!(!is_secure_init_plaintext(&plaintext_with_payload(json!({
        "system_type": "awiki.direct.secure_ack.v1",
        "session_id": "session-1",
        "acked_message_id": "msg-9",
    }))));
}

#[test]
fn secure_ack_session_id_reads_only_string_session_from_object_payload() {
    assert_eq!(
        secure_ack_session_id(&plaintext_with_payload(json!({
            "system_type": "awiki.direct.secure_ack.v1",
            "session_id": "session-1",
        }))),
        "session-1"
    );
    assert_eq!(
        secure_ack_session_id(&plaintext_with_payload(json!({
            "system_type": "awiki.direct.secure_ack.v1",
            "session_id": 42,
        }))),
        ""
    );
    assert_eq!(
        secure_ack_session_id(&plaintext_with_payload(json!("not-an-object"))),
        ""
    );
    assert_eq!(
        secure_ack_session_id(&plaintext_with_payload(json!(
            r#"{"session_id":"session-1"}"#
        ))),
        "session-1"
    );
}

#[test]
fn pending_confirmation_error_detection_matches_go_string_checks() {
    assert!(!is_pending_confirmation_error(None));

    assert!(is_pending_confirmation_error(Some(
        "remote returned PENDING CONFIRMATION for peer"
    )));
    assert!(is_pending_confirmation_error(Some(
        "secure state is Pending-Confirmation"
    )));
    assert!(!is_pending_confirmation_error(Some("confirmation pending")));
}

#[test]
fn current_secure_session_id_reads_matching_file_session_store_like_go() {
    let (workspace, resolved, manager, record) = secure_helper_workspace();

    assert_eq!(
        current_secure_session_id(None, Some(&record), "did:bob"),
        ""
    );
    assert_eq!(
        current_secure_session_id(Some(&manager), None, "did:bob"),
        ""
    );
    assert_eq!(
        current_secure_session_id(Some(&manager), Some(&record), "did:bob"),
        ""
    );
    assert!(
        Path::new(&manager.paths_for_identity("alice").unwrap().identity_dir)
            .join("p5-e2ee-sessions")
            .is_dir()
    );

    let mut session_store = anpsdk::FileSessionStore::new(
        Path::new(&manager.paths_for_identity("alice").unwrap().identity_dir)
            .join("p5-e2ee-sessions"),
    )
    .expect("session store");
    session_store
        .save_session(&direct_session("  session-001  ", "did:bob"))
        .expect("save bob session");
    session_store
        .save_session(&direct_session("session-002", "did:bob:extra"))
        .expect("save non-exact session");

    assert_eq!(
        current_secure_session_id(Some(&manager), Some(&record), "did:bob"),
        "session-001"
    );
    assert_eq!(
        current_secure_session_id(Some(&manager), Some(&record), "did:carol"),
        ""
    );
    drop(workspace);
    let _ = resolved;
}

#[test]
fn queue_secure_outbox_record_uses_current_session_and_go_pending_metadata() {
    let (_workspace, resolved, manager, record) = secure_helper_workspace();
    let mut session_store = anpsdk::FileSessionStore::new(
        Path::new(&manager.paths_for_identity("alice").unwrap().identity_dir)
            .join("p5-e2ee-sessions"),
    )
    .expect("session store");
    session_store
        .save_session(&direct_session("session-queued", "did:bob"))
        .expect("save session");

    let outbox_id = queue_secure_outbox_record(
        &resolved,
        &manager,
        Some(&record),
        "did:bob",
        " \t\n",
        "queued plaintext",
    )
    .expect("queue secure outbox");

    let connection = store::open(&resolved.paths).expect("open store");
    store::ensure_schema(&connection).expect("ensure schema");
    let row = store::get_e2ee_outbox(&connection, &outbox_id, &record.did, &record.identity_name)
        .expect("get queued outbox");

    assert_eq!(row["owner_did"], "did:alice");
    assert_eq!(row["peer_did"], "did:bob");
    assert_eq!(row["session_id"], "session-queued");
    assert_eq!(row["original_type"], "text");
    assert_eq!(row["plaintext"], "queued plaintext");
    assert_eq!(row["local_status"], "queued");
    assert_eq!(row["credential_name"], "alice");
    assert_eq!(
        row["metadata"],
        json!(r#"{"reason":"pending_confirmation"}"#)
    );
}

#[test]
fn queue_secure_outbox_record_requires_identity_record() {
    let (_workspace, resolved, manager, _record) = secure_helper_workspace();

    let error = queue_secure_outbox_record(&resolved, &manager, None, "did:bob", "text", "hello")
        .expect_err("missing record should fail");

    assert_eq!(error.to_string(), "identity record is required");
}

fn row(
    outbox_id: &str,
    peer_did: &str,
    original_type: &str,
    plaintext: &str,
    created_at: &str,
) -> QueuedSecureOutboxRow {
    QueuedSecureOutboxRow {
        outbox_id: outbox_id.to_string(),
        peer_did: peer_did.to_string(),
        original_type: original_type.to_string(),
        plaintext: plaintext.to_string(),
        created_at: created_at.to_string(),
    }
}

fn success_with_message(message_id: &str) -> SecureOutboxFlushRowOutcome {
    SecureOutboxFlushRowOutcome {
        send: SecureOutboxSendOutcome::Success {
            message_id: message_id.to_string(),
            operation_id: format!("op-{message_id}"),
            delivery_state: "accepted".to_string(),
            accepted_at: "2026-04-23T10:00:00Z".to_string(),
        },
        session_id: format!("sess-{message_id}"),
        mark_sent: MarkSentOutcome::Success,
        store_message: StoreMessageOutcome::Success,
    }
}

fn send_ids(actions: &[SecureOutboxFlushAction]) -> Vec<String> {
    actions
        .iter()
        .filter_map(|action| match action {
            SecureOutboxFlushAction::SendText { outbox_id, .. }
            | SecureOutboxFlushAction::SendJson { outbox_id, .. } => Some(outbox_id.clone()),
            _ => None,
        })
        .collect()
}

fn stored_record(actions: &[SecureOutboxFlushAction]) -> awiki_cli::store::MessageRecord {
    actions
        .iter()
        .find_map(|action| match action {
            SecureOutboxFlushAction::StoreMessage { record, .. } => Some(record.clone()),
            _ => None,
        })
        .expect("StoreMessage action")
}

fn plaintext_with_payload(payload: Value) -> Map<String, Value> {
    Map::from_iter([
        (
            "application_content_type".to_string(),
            json!("application/json"),
        ),
        ("payload".to_string(), payload),
    ])
}

fn secure_helper_workspace() -> (
    TempDir,
    config::Resolved,
    identity::Manager,
    identity::types::StoredIdentity,
) {
    let workspace = TempDir::new().expect("workspace");
    let resolved = test_resolved(workspace.path());
    let manager = identity::Manager::new(resolved.paths.clone());
    let record = manager
        .save(identity::types::SaveInput {
            identity_name: "alice".to_string(),
            did: "did:alice".to_string(),
            unique_id: "e1_alice".to_string(),
            display_name: "Alice".to_string(),
            ..identity::types::SaveInput::default()
        })
        .expect("save identity");
    (workspace, resolved, manager, record)
}

fn direct_session(session_id: &str, peer_did: &str) -> anpsdk::DirectSessionState {
    anpsdk::DirectSessionState {
        session_id: session_id.to_string(),
        suite: "ANP-DIRECT-E2EE-X3DH-25519-CHACHA20POLY1305-SHA256-V1".to_string(),
        peer_did: peer_did.to_string(),
        local_key_agreement_id: "did:alice#key-3".to_string(),
        peer_key_agreement_id: "did:bob#key-3".to_string(),
        root_key_b64u: "root".to_string(),
        send_chain_key_b64u: Some("send".to_string()),
        recv_chain_key_b64u: Some("recv".to_string()),
        ratchet_private_key_b64u: "private".to_string(),
        ratchet_public_key_b64u: "public".to_string(),
        peer_ratchet_public_key_b64u: None,
        send_n: 1,
        recv_n: 2,
        previous_send_chain_length: 0,
        skipped_message_keys: Vec::new(),
        is_initiator: true,
        status: "established".to_string(),
    }
}

fn test_resolved(root: &Path) -> config::Resolved {
    config::Resolved {
        paths: config::Paths {
            workspace_home_dir: root.to_string_lossy().to_string(),
            root_dir: root.to_string_lossy().to_string(),
            config_dir: root.join("config").to_string_lossy().to_string(),
            data_dir: root.join("data").to_string_lossy().to_string(),
            state_dir: root.join("state").to_string_lossy().to_string(),
            cache_dir: root.join("cache").to_string_lossy().to_string(),
            logs_dir: root.join("logs").to_string_lossy().to_string(),
            config_file: root.join("config.yaml").to_string_lossy().to_string(),
            identity_dir: root.join("identities").to_string_lossy().to_string(),
            database_file: root
                .join("data")
                .join("awiki-cli.db")
                .to_string_lossy()
                .to_string(),
            legacy_credentials_dir: root.join("credentials").to_string_lossy().to_string(),
            legacy_data_dir: root.join("legacy-data").to_string_lossy().to_string(),
        },
        config_schema_version: 1,
        active_identity: "alice".to_string(),
        runtime_mode: "websocket".to_string(),
        runtime_socket_path: String::new(),
        runtime_listener_enabled: true,
        runtime_listener_auto_install: true,
        runtime_listener_auto_start: true,
        host_notify_enabled: true,
        host_notify_sink: "log".to_string(),
        host_notify_file_path: String::new(),
        host_notify_openclaw_hook_url: String::new(),
        host_notify_openclaw_agent_id: String::new(),
        host_notify_openclaw_hook_name: String::new(),
        host_notify_hermes_notify_url: String::new(),
        host_notify_hermes_deliver: String::new(),
        output_format: "json".to_string(),
        no_color: false,
        service_base_url: "https://awiki.ai".to_string(),
        did_domain: "awiki.ai".to_string(),
        anp_service_endpoint: "https://awiki.ai/anp-im/rpc".to_string(),
        anp_service_did: "did:wba:awiki.ai".to_string(),
        mail_service_url: "https://awiki.ai/mail/rpc".to_string(),
        ca_bundle: String::new(),
        update_disable_strict_version: false,
        update_metadata_cache_ttl_seconds: 3600,
        config_exists: true,
        config_error: String::new(),
        env_hits: Vec::new(),
        sources: Default::default(),
    }
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> std::io::Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-secure-control-test-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
