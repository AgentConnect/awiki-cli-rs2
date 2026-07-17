use std::sync::{Arc, Mutex};

use anyhow::anyhow;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use im_core::ids::{GroupRef, MessageId, PeerRef};
use im_core::messages::{MessageKind, MessageMetadata, MessageMetadataAttribute, ThreadRef};
use tempfile::TempDir;

use crate::runtime::{RuntimeAgentProfile, RuntimeRun};

use super::*;

#[derive(Clone)]
struct MockClient {
    pages: Arc<Mutex<Vec<DelegatedInboxPage>>>,
    calls: Arc<Mutex<Vec<(String, String, Option<String>)>>>,
}

impl UserDelegatedInboxClient for MockClient {
    fn fetch_user_delegated_inbox(
        &self,
        identity: &UserDelegatedIdentityRecord,
        binding: &AppPersonalAgentBindingRecord,
        cursor: Option<&str>,
        _limit: u32,
    ) -> Result<DelegatedInboxPage> {
        self.calls.lock().unwrap().push((
            identity.user_did.clone(),
            binding.inbox_auth_verification_method.clone(),
            cursor.map(ToOwned::to_owned),
        ));
        let mut pages = self.pages.lock().unwrap();
        if pages.is_empty() {
            return Ok(DelegatedInboxPage {
                messages: Vec::new(),
                next_cursor: None,
                has_more: false,
            });
        }
        Ok(pages.remove(0))
    }
}

#[derive(Default)]
struct RecordingDispatcher {
    dispatched: Arc<Mutex<Vec<(RuntimeTask, UserMessageEnvelope)>>>,
    fail_once: Arc<Mutex<bool>>,
}

impl UserDelegatedMessageDispatcher for RecordingDispatcher {
    fn dispatch_user_message(
        &self,
        _binding: &AppPersonalAgentBindingRecord,
        task: RuntimeTask,
        envelope: &UserMessageEnvelope,
    ) -> Result<()> {
        let mut fail_once = self.fail_once.lock().unwrap();
        if *fail_once {
            *fail_once = false;
            return Err(anyhow!("simulated dispatcher failure"));
        }
        drop(fail_once);
        self.dispatched
            .lock()
            .unwrap()
            .push((task, envelope.clone()));
        Ok(())
    }
}

#[derive(Default)]
struct RecordingMessageSyncSender {
    sent: Arc<Mutex<Vec<(String, String, Value)>>>,
    fail_once: Arc<Mutex<bool>>,
}

impl MessageSyncPayloadSender for RecordingMessageSyncSender {
    fn send_message_sync_payload(
        &self,
        binding: &AppPersonalAgentBindingRecord,
        idempotency_key: &str,
        payload: Value,
    ) -> Result<Option<String>> {
        let mut fail_once = self.fail_once.lock().unwrap();
        if *fail_once {
            *fail_once = false;
            return Err(anyhow!("simulated message sync send failure"));
        }
        drop(fail_once);
        self.sent.lock().unwrap().push((
            binding.user_did.clone(),
            idempotency_key.to_string(),
            payload,
        ));
        Ok(Some(format!("sent_{idempotency_key}")))
    }
}

#[test]
fn delegated_inbox_dispatches_plain_message_as_untrusted_envelope() {
    let fixture = fixture();
    let state = &fixture.state;
    let identity = &fixture.identity;
    let binding = &fixture.binding;
    let client = MockClient {
        pages: Arc::new(Mutex::new(vec![DelegatedInboxPage {
            messages: vec![plain_message("msg_1", "did:human:bob", "hello agent")],
            next_cursor: Some("cursor_2".to_string()),
            has_more: false,
        }])),
        calls: Arc::new(Mutex::new(Vec::new())),
    };
    let dispatcher = RecordingDispatcher::default();

    let outcome =
        process_user_delegated_inbox_for_binding(state, &client, &dispatcher, binding).unwrap();

    assert_eq!(outcome.dispatched_messages, 1);
    assert_eq!(outcome.next_cursor.as_deref(), Some("cursor_2"));
    let calls = client.calls.lock().unwrap();
    assert_eq!(calls[0].0, identity.user_did);
    assert_eq!(calls[0].1, identity.verification_method);
    assert_eq!(calls[0].2, None);
    let dispatched = dispatcher.dispatched.lock().unwrap();
    assert_eq!(dispatched.len(), 1);
    let (task, envelope) = &dispatched[0];
    assert_eq!(task.agent_did, binding.runtime_agent_did);
    assert_eq!(task.controller_did, binding.user_did);
    assert_eq!(task.sender_did, "did:human:bob");
    assert_eq!(task.requester_did, "did:human:bob");
    assert_eq!(task.reply_recipient_did, binding.user_did);
    assert_eq!(envelope.content_role, "user_message_untrusted");
    assert_eq!(envelope.source_sender_did, "did:human:bob");
    assert_eq!(envelope.content_text, "hello agent");
    assert_eq!(envelope.message_kind, "text");
    assert_eq!(
        envelope.allowed_actions,
        vec![
            "message.summarize_plain".to_string(),
            "message.create_draft".to_string()
        ]
    );
    assert!(!task.text.contains("system"));

    let event = state
        .load_message_event(&event_id(&binding.user_did, "msg_1"))
        .unwrap()
        .unwrap();
    assert_eq!(event.processing_status, MESSAGE_EVENT_STATUS_DISPATCHED);
    assert_eq!(event.retention_class, RETENTION_CLASS_SHORT_EXCERPT);
    assert_eq!(
        event.plain_text_ref_or_excerpt.as_deref(),
        Some("hello agent")
    );
    let sync = state
        .load_message_sync_outbox(&message_sync_idempotency_key(binding, "msg_1"))
        .unwrap()
        .unwrap();
    assert_eq!(sync.payload_json["content_role"], "user_message_untrusted");

    let sync_sender = RecordingMessageSyncSender::default();
    let sent = flush_message_sync_outbox_with_sender(state, &sync_sender, 10).unwrap();
    assert_eq!(sent, 1);
    let sent_payloads = sync_sender.sent.lock().unwrap();
    assert_eq!(sent_payloads[0].0, binding.user_did);
    assert_eq!(
        sent_payloads[0].1,
        message_sync_idempotency_key(binding, "msg_1")
    );
    assert_eq!(sent_payloads[0].2["message_id"], "msg_1");
    let delivered = state
        .load_message_sync_outbox(&message_sync_idempotency_key(binding, "msg_1"))
        .unwrap()
        .unwrap();
    assert_eq!(delivered.status, "sent");
}

#[test]
fn explicit_empty_capabilities_project_empty_allowed_actions() {
    let fixture = fixture();
    let mut binding = fixture.binding.clone();
    binding.capability_policy_json = json!({
        "schema": crate::app_bridge::action::APP_CAPABILITIES_SCHEMA,
        "capabilities": []
    });
    let message = plain_message("msg_1", "did:human:bob", "hello agent");

    let envelope =
        user_message_envelope(&binding, &message, plain_dispatch("hello agent")).unwrap();

    assert!(envelope.allowed_actions.is_empty());
}

#[test]
fn delegated_inbox_skips_revoked_binding_without_fetch_or_dispatch() {
    let fixture = fixture();
    let state = &fixture.state;
    let mut binding = fixture.binding.clone();
    binding.status = "personal_agent_revoked".to_string();
    binding.revoked_at_ms = Some(123);
    state.upsert_app_personal_agent_binding(&binding).unwrap();
    let client = MockClient {
        pages: Arc::new(Mutex::new(vec![DelegatedInboxPage {
            messages: vec![plain_message("msg_after_revoke", "did:human:bob", "hello")],
            next_cursor: Some("cursor_after_revoke".to_string()),
            has_more: false,
        }])),
        calls: Arc::new(Mutex::new(Vec::new())),
    };
    let dispatcher = RecordingDispatcher::default();

    let outcome =
        process_user_delegated_inbox_for_binding(state, &client, &dispatcher, &binding).unwrap();

    assert_eq!(outcome.fetched_messages, 0);
    assert_eq!(outcome.dispatched_messages, 0);
    assert_eq!(outcome.next_cursor, None);
    assert!(client.calls.lock().unwrap().is_empty());
    assert!(dispatcher.dispatched.lock().unwrap().is_empty());
    assert!(state
        .load_message_event(&event_id(&binding.user_did, "msg_after_revoke"))
        .unwrap()
        .is_none());
    assert!(state
        .load_inbox_cursor(&binding.user_did, &inbox_scope_for_binding(&binding))
        .unwrap()
        .is_none());
    assert!(state
        .audit_event_exists(
            "user_delegated_inbox.sync.skipped_inactive_binding",
            Some(&binding.daemon_agent_did),
            Some("personal_agent_revoked"),
        )
        .unwrap());
}

#[test]
fn delegated_inbox_skips_disabled_binding_without_fetch_or_dispatch() {
    let fixture = fixture();
    let state = &fixture.state;
    let mut binding = fixture.binding.clone();
    binding.status = "personal_agent_disabled".to_string();
    state.upsert_app_personal_agent_binding(&binding).unwrap();
    let client = MockClient {
        pages: Arc::new(Mutex::new(vec![DelegatedInboxPage {
            messages: vec![plain_message("msg_disabled", "did:human:bob", "hello")],
            next_cursor: Some("cursor_disabled".to_string()),
            has_more: false,
        }])),
        calls: Arc::new(Mutex::new(Vec::new())),
    };
    let dispatcher = RecordingDispatcher::default();

    let outcome =
        process_user_delegated_inbox_for_binding(state, &client, &dispatcher, &binding).unwrap();

    assert_eq!(outcome.fetched_messages, 0);
    assert_eq!(outcome.dispatched_messages, 0);
    assert!(client.calls.lock().unwrap().is_empty());
    assert!(dispatcher.dispatched.lock().unwrap().is_empty());
    assert!(state
        .load_message_event(&event_id(&binding.user_did, "msg_disabled"))
        .unwrap()
        .is_none());
    assert!(state
        .audit_event_exists(
            "user_delegated_inbox.sync.skipped_inactive_binding",
            Some(&binding.daemon_agent_did),
            Some("personal_agent_disabled"),
        )
        .unwrap());
}

#[test]
fn delegated_inbox_uses_cursor_and_skips_processed_replay() {
    let fixture = fixture();
    let state = &fixture.state;
    let binding = &fixture.binding;
    state
        .upsert_inbox_cursor(&InboxCursorRecord {
            owner_did: binding.user_did.clone(),
            inbox_scope: inbox_scope_for_binding(binding),
            cursor: Some("cursor_1".to_string()),
            updated_at_ms: 0,
        })
        .unwrap();
    let client = MockClient {
        pages: Arc::new(Mutex::new(vec![
            DelegatedInboxPage {
                messages: vec![plain_message("msg_1", "did:human:bob", "hello")],
                next_cursor: Some("cursor_2".to_string()),
                has_more: false,
            },
            DelegatedInboxPage {
                messages: vec![plain_message("msg_1", "did:human:bob", "hello")],
                next_cursor: Some("cursor_3".to_string()),
                has_more: false,
            },
        ])),
        calls: Arc::new(Mutex::new(Vec::new())),
    };
    let dispatcher = RecordingDispatcher::default();

    process_user_delegated_inbox_for_binding(state, &client, &dispatcher, binding).unwrap();
    let second =
        process_user_delegated_inbox_for_binding(state, &client, &dispatcher, binding).unwrap();

    assert_eq!(second.dispatched_messages, 0);
    assert_eq!(second.skipped_processed_messages, 1);
    let calls = client.calls.lock().unwrap();
    assert_eq!(calls[0].2.as_deref(), Some("cursor_1"));
    assert_eq!(calls[1].2.as_deref(), Some("cursor_2"));
    assert_eq!(dispatcher.dispatched.lock().unwrap().len(), 1);
    let cursor = state
        .load_inbox_cursor(&binding.user_did, &inbox_scope_for_binding(binding))
        .unwrap()
        .unwrap();
    assert_eq!(cursor.cursor.as_deref(), Some("cursor_3"));
}

#[test]
fn delegated_inbox_retries_retryable_processing_after_dispatch_failure() {
    let fixture = fixture();
    let state = &fixture.state;
    let binding = &fixture.binding;
    let client = MockClient {
        pages: Arc::new(Mutex::new(vec![
            DelegatedInboxPage {
                messages: vec![plain_message("msg_retry", "did:human:bob", "retry me")],
                next_cursor: Some("cursor_after_failure".to_string()),
                has_more: false,
            },
            DelegatedInboxPage {
                messages: vec![plain_message("msg_retry", "did:human:bob", "retry me")],
                next_cursor: Some("cursor_after_success".to_string()),
                has_more: false,
            },
        ])),
        calls: Arc::new(Mutex::new(Vec::new())),
    };
    let dispatcher = RecordingDispatcher {
        fail_once: Arc::new(Mutex::new(true)),
        ..RecordingDispatcher::default()
    };

    let first = process_user_delegated_inbox_for_binding(state, &client, &dispatcher, binding);

    assert!(first.is_err());
    let processed = state
        .load_processed_message(&binding.user_did, "msg_retry")
        .unwrap()
        .unwrap();
    assert_eq!(processed.status, PROCESSED_STATUS_FAILED_RETRYABLE);
    assert!(state
        .load_message_event(&event_id(&binding.user_did, "msg_retry"))
        .unwrap()
        .is_none());
    assert!(state
        .load_inbox_cursor(&binding.user_did, &inbox_scope_for_binding(binding))
        .unwrap()
        .is_none());

    let second =
        process_user_delegated_inbox_for_binding(state, &client, &dispatcher, binding).unwrap();

    assert_eq!(second.dispatched_messages, 1);
    assert_eq!(dispatcher.dispatched.lock().unwrap().len(), 1);
    let processed = state
        .load_processed_message(&binding.user_did, "msg_retry")
        .unwrap()
        .unwrap();
    assert_eq!(processed.status, PROCESSED_STATUS_DISPATCHED);
    let cursor = state
        .load_inbox_cursor(&binding.user_did, &inbox_scope_for_binding(binding))
        .unwrap()
        .unwrap();
    assert_eq!(cursor.cursor.as_deref(), Some("cursor_after_success"));
}

#[test]
fn delegated_runtime_run_id_skips_failed_prior_attempt() {
    let fixture = fixture();
    let state = &fixture.state;
    let binding = &fixture.binding;
    let failed = crate::runtime::RuntimeRun {
        run_id: "run_task_user_msg_failed".to_string(),
        task_id: "task_user_msg_failed".to_string(),
        agent_did: binding.runtime_agent_did.clone(),
        runtime_profile_id: binding.runtime_profile_id.clone(),
        runtime_plugin_id: "hermes".to_string(),
        workspace_id: None,
        status: RuntimeRunStatus::Failed,
    };
    state.try_insert_runtime_run(&failed).unwrap();

    let run_id = delegated_runtime_run_id(state, "task_user_msg_failed").unwrap();

    assert_eq!(run_id, "run_task_user_msg_failed_retry_1");
}

#[test]
fn delegated_inbox_ignores_e2ee_without_plaintext_event_or_dispatch() {
    let fixture = fixture();
    let state = &fixture.state;
    let binding = &fixture.binding;
    let client = MockClient {
        pages: Arc::new(Mutex::new(vec![DelegatedInboxPage {
            messages: vec![e2ee_message("msg_e2ee", "did:human:bob")],
            next_cursor: None,
            has_more: false,
        }])),
        calls: Arc::new(Mutex::new(Vec::new())),
    };
    let dispatcher = RecordingDispatcher::default();

    let outcome =
        process_user_delegated_inbox_for_binding(state, &client, &dispatcher, binding).unwrap();

    assert_eq!(outcome.ignored_e2ee_messages, 1);
    assert!(dispatcher.dispatched.lock().unwrap().is_empty());
    let event = state
        .load_message_event(&event_id(&binding.user_did, "msg_e2ee"))
        .unwrap()
        .unwrap();
    assert_eq!(event.processing_status, PROCESSED_STATUS_IGNORED_E2EE);
    assert_eq!(event.retention_class, RETENTION_CLASS_OPAQUE_ONLY);
    assert!(event.plain_text_ref_or_excerpt.is_none());
}

#[test]
fn delegated_inbox_does_not_dispatch_system_payload_as_user_text() {
    let fixture = fixture();
    let state = &fixture.state;
    let binding = &fixture.binding;
    let client = MockClient {
        pages: Arc::new(Mutex::new(vec![DelegatedInboxPage {
            messages: vec![system_payload_message("msg_control", "did:human:bob")],
            next_cursor: None,
            has_more: false,
        }])),
        calls: Arc::new(Mutex::new(Vec::new())),
    };
    let dispatcher = RecordingDispatcher::default();

    let outcome =
        process_user_delegated_inbox_for_binding(state, &client, &dispatcher, binding).unwrap();

    assert_eq!(outcome.dispatched_messages, 0);
    assert_eq!(outcome.skipped_unsupported_messages, 1);
    assert_eq!(outcome.skipped_processed_messages, 0);
    assert!(dispatcher.dispatched.lock().unwrap().is_empty());
}

#[test]
fn delegated_inbox_syncs_unsupported_system_payload_without_dispatch() {
    let fixture = fixture();
    let state = &fixture.state;
    let binding = &fixture.binding;
    let client = MockClient {
        pages: Arc::new(Mutex::new(vec![DelegatedInboxPage {
            messages: vec![system_payload_message("msg_control", "did:human:bob")],
            next_cursor: None,
            has_more: false,
        }])),
        calls: Arc::new(Mutex::new(Vec::new())),
    };
    let dispatcher = RecordingDispatcher::default();

    let outcome =
        process_user_delegated_inbox_for_binding(state, &client, &dispatcher, binding).unwrap();

    assert_eq!(outcome.dispatched_messages, 0);
    assert!(dispatcher.dispatched.lock().unwrap().is_empty());
    let event = state
        .load_message_event(&event_id(&binding.user_did, "msg_control"))
        .unwrap()
        .unwrap();
    assert_eq!(event.processing_status, "skipped_unsupported");
    assert_eq!(event.retention_class, RETENTION_CLASS_OPAQUE_ONLY);
    assert!(event.plain_text_ref_or_excerpt.is_none());
    let sync = state
        .load_message_sync_outbox(&message_sync_idempotency_key(binding, "msg_control"))
        .unwrap()
        .unwrap();
    assert_eq!(sync.payload_json["schema"], MESSAGE_SYNC_SCHEMA);
    assert_eq!(
        sync.payload_json["processing_status"],
        "skipped_unsupported"
    );
    assert_eq!(
        sync.payload_json["unsupported_reason"],
        "system_control_payload"
    );
    assert_eq!(
        sync.payload_json["retention_class"],
        RETENTION_CLASS_OPAQUE_ONLY
    );
}

#[test]
fn delegated_inbox_skips_app_recovery_control_payload_without_resyncing() {
    let fixture = fixture();
    let state = &fixture.state;
    let binding = &fixture.binding;
    let client = MockClient {
        pages: Arc::new(Mutex::new(vec![DelegatedInboxPage {
            messages: vec![direct_payload_message(
                "msg_control_recovery",
                &binding.daemon_agent_did,
                json!({
                    "schema": MESSAGE_SYNC_SCHEMA,
                    "sync_type": "runtime_final",
                    "source_message_id": "msg_1",
                    "runtime_agent_did": binding.runtime_agent_did,
                }),
            )],
            next_cursor: None,
            has_more: false,
        }])),
        calls: Arc::new(Mutex::new(Vec::new())),
    };
    let dispatcher = RecordingDispatcher::default();

    let outcome =
        process_user_delegated_inbox_for_binding(state, &client, &dispatcher, binding).unwrap();

    assert_eq!(outcome.dispatched_messages, 0);
    assert_eq!(outcome.skipped_app_control_messages, 1);
    assert_eq!(outcome.skipped_unsupported_messages, 0);
    assert!(dispatcher.dispatched.lock().unwrap().is_empty());
    let processed = state
        .load_processed_message(&binding.user_did, "msg_control_recovery")
        .unwrap()
        .unwrap();
    assert_eq!(processed.schema, MESSAGE_SYNC_SCHEMA);
    assert_eq!(processed.status, PROCESSED_STATUS_SKIPPED_APP_CONTROL);
    assert!(state
        .load_message_event(&event_id(&binding.user_did, "msg_control_recovery"))
        .unwrap()
        .is_none());
    assert!(state
        .load_message_sync_outbox(&message_sync_idempotency_key(
            binding,
            "msg_control_recovery"
        ))
        .unwrap()
        .is_none());
}

#[test]
fn delegated_inbox_skips_bound_daemon_status_payload_without_resyncing() {
    let fixture = fixture();
    let state = &fixture.state;
    let binding = &fixture.binding;
    let client = MockClient {
        pages: Arc::new(Mutex::new(vec![DelegatedInboxPage {
            messages: vec![direct_payload_message(
                "msg_daemon_status",
                &binding.daemon_agent_did,
                json!({
                    "command_id": "cmd_agent_status_1",
                    "daemon_agent_did": binding.daemon_agent_did,
                    "daemon": {},
                }),
            )],
            next_cursor: None,
            has_more: false,
        }])),
        calls: Arc::new(Mutex::new(Vec::new())),
    };
    let dispatcher = RecordingDispatcher::default();

    let outcome =
        process_user_delegated_inbox_for_binding(state, &client, &dispatcher, binding).unwrap();

    assert_eq!(outcome.dispatched_messages, 0);
    assert_eq!(outcome.skipped_app_control_messages, 1);
    assert_eq!(outcome.skipped_unsupported_messages, 0);
    assert!(dispatcher.dispatched.lock().unwrap().is_empty());
    let processed = state
        .load_processed_message(&binding.user_did, "msg_daemon_status")
        .unwrap()
        .unwrap();
    assert_eq!(processed.schema, "awiki.app_control.unknown.v1");
    assert_eq!(processed.status, PROCESSED_STATUS_SKIPPED_APP_CONTROL);
    assert!(state
        .load_message_event(&event_id(&binding.user_did, "msg_daemon_status"))
        .unwrap()
        .is_none());
    assert!(state
        .load_message_sync_outbox(&message_sync_idempotency_key(binding, "msg_daemon_status"))
        .unwrap()
        .is_none());
}

#[test]
fn delegated_inbox_ignores_structured_group_agent_mentions() {
    let fixture = fixture();
    let state = &fixture.state;
    let binding = &fixture.binding;
    let payload = json!({
        "text": "@Hermes please summarize",
        "mentions": [{
            "id": "men_agent",
            "range": {"start": 0, "end": 7, "unit": "unicode_code_point"},
            "target": {
                "kind": "agent",
                "did": binding.runtime_agent_did,
                "display_name": "Hermes display ignored"
            },
            "mention_role": "addressee"
        }]
    });
    let client = MockClient {
        pages: Arc::new(Mutex::new(vec![DelegatedInboxPage {
            messages: vec![mention_payload_message(
                "msg_agent",
                "did:human:bob",
                payload,
            )],
            next_cursor: None,
            has_more: false,
        }])),
        calls: Arc::new(Mutex::new(Vec::new())),
    };
    let dispatcher = RecordingDispatcher::default();

    let outcome =
        process_user_delegated_inbox_for_binding(state, &client, &dispatcher, binding).unwrap();

    assert_eq!(outcome.dispatched_messages, 0);
    assert!(dispatcher.dispatched.lock().unwrap().is_empty());
}

#[test]
fn delegated_inbox_ignores_group_selector_mentions() {
    let fixture = fixture();
    let state = &fixture.state;
    let binding = &fixture.binding;
    let client = MockClient {
        pages: Arc::new(Mutex::new(vec![DelegatedInboxPage {
            messages: vec![
                mention_payload_message(
                    "msg_agents",
                    "did:human:bob",
                    json!({
                        "text": "@agents FYI only",
                        "mentions": [{
                            "id": "men_agents",
                            "range": {"start": 0, "end": 7, "unit": "unicode_code_point"},
                            "target": {"kind": "group_selector", "selector": "agents"},
                            "mention_role": "cc"
                        }]
                    }),
                ),
                mention_payload_message(
                    "msg_all",
                    "did:human:carol",
                    json!({
                        "text": "@all please look",
                        "mentions": [{
                            "id": "men_all",
                            "range": {"start": 0, "end": 4, "unit": "unicode_code_point"},
                            "target": {"kind": "group_selector", "selector": "all"}
                        }]
                    }),
                ),
            ],
            next_cursor: None,
            has_more: false,
        }])),
        calls: Arc::new(Mutex::new(Vec::new())),
    };
    let dispatcher = RecordingDispatcher::default();

    let outcome =
        process_user_delegated_inbox_for_binding(state, &client, &dispatcher, binding).unwrap();

    assert_eq!(outcome.dispatched_messages, 0);
    assert!(dispatcher.dispatched.lock().unwrap().is_empty());
}

#[test]
fn delegated_inbox_does_not_dispatch_group_plain_text_humans_or_invalid_mentions() {
    let fixture = fixture();
    let state = &fixture.state;
    let binding = &fixture.binding;
    let client = MockClient {
        pages: Arc::new(Mutex::new(vec![DelegatedInboxPage {
            messages: vec![
                group_plain_message("msg_plain_at", "did:human:bob", "@Hermes hello"),
                mention_payload_message(
                    "msg_humans",
                    "did:human:bob",
                    json!({
                        "text": "@humans please look",
                        "mentions": [{
                            "id": "men_humans",
                            "range": {"start": 0, "end": 7, "unit": "unicode_code_point"},
                            "target": {"kind": "group_selector", "selector": "humans"}
                        }]
                    }),
                ),
                mention_payload_message(
                    "msg_invalid",
                    "did:human:bob",
                    json!({
                        "text": "@agents broken range",
                        "mentions": [{
                            "id": "men_invalid",
                            "range": {"start": 0, "end": 99, "unit": "unicode_code_point"},
                            "target": {"kind": "group_selector", "selector": "agents"}
                        }]
                    }),
                ),
            ],
            next_cursor: Some("cursor_after_skips".to_string()),
            has_more: false,
        }])),
        calls: Arc::new(Mutex::new(Vec::new())),
    };
    let dispatcher = RecordingDispatcher::default();

    let outcome =
        process_user_delegated_inbox_for_binding(state, &client, &dispatcher, binding).unwrap();

    assert_eq!(outcome.dispatched_messages, 0);
    assert_eq!(outcome.skipped_unsupported_messages, 3);
    assert_eq!(outcome.skipped_processed_messages, 0);
    assert_eq!(outcome.next_cursor.as_deref(), Some("cursor_after_skips"));
    assert!(dispatcher.dispatched.lock().unwrap().is_empty());

    for (message_id, reason) in [
        ("msg_plain_at", "group_message"),
        ("msg_humans", "group_message"),
        ("msg_invalid", "group_message"),
    ] {
        let sync = state
            .load_message_sync_outbox(&message_sync_idempotency_key(binding, message_id))
            .unwrap()
            .unwrap();
        assert_eq!(
            sync.payload_json["processing_status"],
            "skipped_unsupported"
        );
        assert_eq!(sync.payload_json["unsupported_reason"], reason);
    }
}

#[test]
fn delegated_inbox_ignores_group_e2ee_mention_cipher_without_dispatch() {
    let fixture = fixture();
    let state = &fixture.state;
    let binding = &fixture.binding;
    let client = MockClient {
        pages: Arc::new(Mutex::new(vec![DelegatedInboxPage {
            messages: vec![group_e2ee_mention_cipher_message(
                "msg_group_e2ee",
                "did:human:bob",
                binding,
            )],
            next_cursor: None,
            has_more: false,
        }])),
        calls: Arc::new(Mutex::new(Vec::new())),
    };
    let dispatcher = RecordingDispatcher::default();

    let outcome =
        process_user_delegated_inbox_for_binding(state, &client, &dispatcher, binding).unwrap();

    assert_eq!(outcome.ignored_e2ee_messages, 1);
    assert!(dispatcher.dispatched.lock().unwrap().is_empty());
    let event = state
        .load_message_event(&event_id(&binding.user_did, "msg_group_e2ee"))
        .unwrap()
        .unwrap();
    assert_eq!(event.message_kind, "e2ee_opaque");
    assert_eq!(event.retention_class, RETENTION_CLASS_OPAQUE_ONLY);
    assert!(event.plain_text_ref_or_excerpt.is_none());
}

#[test]
fn delegated_runtime_status_and_final_are_queued_without_plaintext_final() {
    let fixture = fixture();
    let state = &fixture.state;
    let binding = &fixture.binding;
    let task = RuntimeTask {
        task_id: "task_user_msg_1".to_string(),
        agent_did: binding.runtime_agent_did.clone(),
        agent_handle: "alice-hermes".to_string(),
        controller_user_id: "user-alice".to_string(),
        controller_full_handle: "alice.anpclaw.com".to_string(),
        controller_scope_key: "controller-scope:v1:user-alice:alice.anpclaw.com".to_string(),
        controller_did: binding.user_did.clone(),
        sender_did: "did:human:bob".to_string(),
        requester_did: "did:human:bob".to_string(),
        requester_user_id: Some("user-bob".to_string()),
        requester_full_handle: Some("bob.example.com".to_string()),
        trigger_kind: RuntimeTaskTriggerKind::DelegatedDirect,
        conversation_scope: RuntimeConversationScope::direct("user-bob", "bob.example.com")
            .unwrap(),
        invocation_authority: RuntimeInvocationAuthority::Requester,
        reply_recipient_did: binding.user_did.clone(),
        conversation_id: Some("direct:did:human:bob".to_string()),
        text: serde_json::to_string(&json!({
            "schema": "awiki.runtime.user_message_task.v1",
            "source_message_id": "msg_user_1",
            "source_conversation_id": "direct:did:human:bob",
            "source_sender_did": "did:human:bob",
            "content_hash": "sha256:test",
            "content_text": "hello agent"
        }))
        .unwrap(),
    };
    state.insert_runtime_task(&task).unwrap();
    state
        .insert_runtime_run(&RuntimeRun {
            run_id: "run_user_msg_1".to_string(),
            task_id: task.task_id.clone(),
            agent_did: binding.runtime_agent_did.clone(),
            runtime_profile_id: binding.runtime_profile_id.clone(),
            runtime_plugin_id: HERMES_RUNTIME_PLUGIN_ID.to_string(),
            workspace_id: None,
            status: RuntimeRunStatus::Running,
        })
        .unwrap();
    let outbox = UserDelegatedRuntimeOutbox::new_for_test(state);
    let context = AuthorizedRuntimeContext {
        token_id: "token_1".to_string(),
        agent_did: binding.runtime_agent_did.clone(),
        runtime_profile_id: binding.runtime_profile_id.clone(),
        run_id: "run_user_msg_1".to_string(),
        method: crate::security::runtime_token::RpcMethod::TaskStatus,
    };

    outbox
        .send_status(&context, "running", Some("working on it"))
        .unwrap();
    outbox
        .send_final(&context, Some("full final text should not be stored"))
        .unwrap();

    let status = state
        .load_message_sync_outbox(
            "message-sync:did:human:alice:runtime-status:run_user_msg_1:running",
        )
        .unwrap()
        .unwrap();
    assert_eq!(status.payload_json["sync_type"], "runtime_status");
    assert_eq!(status.payload_json["source_message_id"], "msg_user_1");
    assert_eq!(status.payload_json["has_text"], true);
    assert!(status.payload_json["text_hash"].as_str().is_some());

    let final_sync = state
        .load_message_sync_outbox("message-sync:did:human:alice:runtime-final:run_user_msg_1")
        .unwrap()
        .unwrap();
    assert_eq!(final_sync.payload_json["sync_type"], "runtime_final");
    assert_eq!(final_sync.payload_json["source_message_id"], "msg_user_1");
    assert_eq!(
        final_sync.payload_json["source_conversation_id"],
        "direct:did:human:bob"
    );
    assert_eq!(final_sync.payload_json["retention_class"], "hash_only");
    assert!(final_sync.payload_json["text_hash"].as_str().is_some());
    assert!(!final_sync
        .payload_json
        .to_string()
        .contains("full final text should not be stored"));

    let sync_sender = RecordingMessageSyncSender::default();
    let sent = flush_message_sync_outbox_with_sender(state, &sync_sender, 10).unwrap();
    assert_eq!(sent, 2);
    let sent_payloads = sync_sender.sent.lock().unwrap();
    assert!(sent_payloads
        .iter()
        .any(|(_, _, payload)| payload["sync_type"] == "runtime_final"
            && payload["source_message_id"] == "msg_user_1"));
}

#[test]
fn delegated_runtime_host_final_message_is_converted_to_message_sync() {
    let fixture = fixture();
    let state = &fixture.state;
    let binding = &fixture.binding;
    let task = RuntimeTask {
        task_id: "task_user_msg_host_final".to_string(),
        agent_did: binding.runtime_agent_did.clone(),
        agent_handle: "alice-hermes".to_string(),
        controller_user_id: "user-alice".to_string(),
        controller_full_handle: "alice.anpclaw.com".to_string(),
        controller_scope_key: "controller-scope:v1:user-alice:alice.anpclaw.com".to_string(),
        controller_did: binding.user_did.clone(),
        sender_did: "did:human:bob".to_string(),
        requester_did: "did:human:bob".to_string(),
        requester_user_id: Some("user-bob".to_string()),
        requester_full_handle: Some("bob.example.com".to_string()),
        trigger_kind: RuntimeTaskTriggerKind::DelegatedDirect,
        conversation_scope: RuntimeConversationScope::direct("user-bob", "bob.example.com")
            .unwrap(),
        invocation_authority: RuntimeInvocationAuthority::Requester,
        reply_recipient_did: binding.user_did.clone(),
        conversation_id: Some("direct:did:human:bob".to_string()),
        text: serde_json::to_string(&json!({
            "schema": "awiki.runtime.user_message_task.v1",
            "source_message_id": "msg_user_host_final",
            "source_conversation_id": "direct:did:human:bob",
            "source_sender_did": "did:human:bob",
            "content_hash": "sha256:test",
            "content_text": "hello agent"
        }))
        .unwrap(),
    };
    state.insert_runtime_task(&task).unwrap();
    state
        .insert_runtime_run(&RuntimeRun {
            run_id: "run_user_msg_host_final".to_string(),
            task_id: task.task_id.clone(),
            agent_did: binding.runtime_agent_did.clone(),
            runtime_profile_id: binding.runtime_profile_id.clone(),
            runtime_plugin_id: HERMES_RUNTIME_PLUGIN_ID.to_string(),
            workspace_id: None,
            status: RuntimeRunStatus::Running,
        })
        .unwrap();
    let outbox = UserDelegatedRuntimeOutbox::new_for_test(state);
    let context = AuthorizedRuntimeContext {
        token_id: HOST_RUNTIME_FINAL_OUTBOX_TOKEN_ID.to_string(),
        agent_did: binding.runtime_agent_did.clone(),
        runtime_profile_id: binding.runtime_profile_id.clone(),
        run_id: "run_user_msg_host_final".to_string(),
        method: crate::security::runtime_token::RpcMethod::MsgSend,
    };
    let result = outbox
        .send_message(
            &context,
            &RuntimeMessageSend {
                target: crate::outbox::RuntimeMessageTarget::Direct {
                    recipient: binding.user_did.clone(),
                    raw_recipient: binding.user_did.clone(),
                    resolved_did: Some(binding.user_did.clone()),
                },
                text: "final summary text".to_string(),
                payload: None,
                file_path: None,
                display_filename: None,
                mime_type: None,
                idempotency_key: Some("runtime-final:test".to_string()),
                security: crate::outbox::RuntimeMessageSecurity::DefaultPlain,
            },
        )
        .unwrap();

    assert_eq!(result.message_id.as_deref(), Some("runtime-final:test"));
    let final_sync = state
        .load_message_sync_outbox(
            "message-sync:did:human:alice:runtime-final:run_user_msg_host_final",
        )
        .unwrap()
        .unwrap();
    assert_eq!(final_sync.payload_json["sync_type"], "runtime_final");
    assert_eq!(
        final_sync.payload_json["source_message_id"],
        "msg_user_host_final"
    );
    assert_eq!(final_sync.payload_json["has_text"], true);
    assert!(!final_sync
        .payload_json
        .to_string()
        .contains("final summary text"));
}

#[test]
fn delegated_inbox_key_ref_uses_vault_without_private_key_files() {
    install_test_im_core_vault_root_key();
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let mut identity = fixture().identity;
    identity.private_key_material =
        crate::app_bridge::secret_store::ed25519_private_key_pem_for_test(&[17_u8; 32]);

    let key_ref = ensure_delegated_inbox_key_ref(&config, &identity).unwrap();
    ensure_delegated_inbox_did_shadow(&config, &identity).unwrap();

    assert!(key_ref.starts_with("vault:"));
    let legacy_key_path = config
        .runtime_cache_dir
        .join("delegated-inbox")
        .join(stable_id_suffix(&identity.user_did))
        .join("daemon-key-1.pem");
    let shadow_private_key_path = config
        .identity_root_dir
        .join(delegated_identity_alias(&identity.user_did))
        .join("private.key");
    assert!(!legacy_key_path.exists());
    assert!(!shadow_private_key_path.exists());
    assert!(config
        .im_core_sqlite_path
        .parent()
        .unwrap()
        .join("secrets")
        .join("vault")
        .join("records")
        .is_dir());
}

#[test]
fn delegated_runtime_host_final_rejects_non_controller_target() {
    let fixture = fixture();
    let state = &fixture.state;
    let binding = &fixture.binding;
    insert_delegated_runtime_task_and_run(
        state,
        binding,
        "task_user_msg_host_final_wrong_target",
        "run_user_msg_host_final_wrong_target",
        "msg_user_host_final_wrong_target",
    );
    let outbox = UserDelegatedRuntimeOutbox::new_for_test(state);
    let context = AuthorizedRuntimeContext {
        token_id: HOST_RUNTIME_FINAL_OUTBOX_TOKEN_ID.to_string(),
        agent_did: binding.runtime_agent_did.clone(),
        runtime_profile_id: binding.runtime_profile_id.clone(),
        run_id: "run_user_msg_host_final_wrong_target".to_string(),
        method: crate::security::runtime_token::RpcMethod::MsgSend,
    };

    let error = outbox
        .send_message(
            &context,
            &RuntimeMessageSend {
                target: crate::outbox::RuntimeMessageTarget::Direct {
                    recipient: "did:human:bob".to_string(),
                    raw_recipient: "did:human:bob".to_string(),
                    resolved_did: Some("did:human:bob".to_string()),
                },
                text: "must not leave owner sync channel".to_string(),
                payload: None,
                file_path: None,
                display_filename: None,
                mime_type: None,
                idempotency_key: Some("runtime-final:wrong-target".to_string()),
                security: crate::outbox::RuntimeMessageSecurity::DefaultPlain,
            },
        )
        .unwrap_err();

    assert!(error
        .to_string()
        .contains("host runtime final must target the delegated controller DID"));
    assert!(state
        .load_message_sync_outbox(
            "message-sync:did:human:alice:runtime-final:run_user_msg_host_final_wrong_target",
        )
        .unwrap()
        .is_none());
    let connection = state.connection().unwrap();
    let audit_dump: String = connection
        .query_row(
            "SELECT COALESCE(detail_json, '') FROM audit_log \
             WHERE event_type = 'user_delegated_inbox.runtime_final.host_sync.rejected' \
             ORDER BY created_at_ms DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(audit_dump.contains("host_runtime_final_must_target_controller_did"));
    assert!(!audit_dump.contains("must not leave owner sync channel"));
}

#[test]
fn delegated_runtime_outbound_message_and_attachment_are_rejected_without_plaintext_audit() {
    let fixture = fixture();
    let state = &fixture.state;
    let binding = &fixture.binding;
    insert_delegated_runtime_task_and_run(
        state,
        binding,
        "task_user_msg_outbound_reject",
        "run_user_msg_outbound_reject",
        "msg_user_outbound_reject",
    );
    let outbox = UserDelegatedRuntimeOutbox::new_for_test(state);
    let context = AuthorizedRuntimeContext {
        token_id: "token_plaintext_secret".to_string(),
        agent_did: binding.runtime_agent_did.clone(),
        runtime_profile_id: binding.runtime_profile_id.clone(),
        run_id: "run_user_msg_outbound_reject".to_string(),
        method: crate::security::runtime_token::RpcMethod::MsgSend,
    };

    let message_error = outbox
        .send_message(
            &context,
            &RuntimeMessageSend {
                target: crate::outbox::RuntimeMessageTarget::Direct {
                    recipient: "did:human:bob".to_string(),
                    raw_recipient: "did:human:bob".to_string(),
                    resolved_did: Some("did:human:bob".to_string()),
                },
                text: "sensitive user draft should not be audited".to_string(),
                payload: None,
                file_path: None,
                display_filename: None,
                mime_type: None,
                idempotency_key: Some("runtime-msg:blocked".to_string()),
                security: crate::outbox::RuntimeMessageSecurity::DefaultPlain,
            },
        )
        .unwrap_err();
    let attachment_error = outbox
        .send_attachment(
            &context,
            &RuntimeAttachmentSend {
                target: "current_conversation".to_string(),
                target_did: Some("did:human:bob".to_string()),
                file_path: std::path::PathBuf::from("/tmp/secret-report.txt"),
                display_filename: Some("secret-report.txt".to_string()),
                caption: Some("attachment plaintext should not be audited".to_string()),
            },
        )
        .unwrap_err();

    assert!(message_error
        .to_string()
        .contains("outbound send is not enabled"));
    assert!(attachment_error
        .to_string()
        .contains("attachment send is not enabled"));
    let connection = state.connection().unwrap();
    let audit_dump: String = connection
        .query_row(
            "SELECT GROUP_CONCAT(COALESCE(detail_json, ''), '\n') FROM audit_log",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!audit_dump.contains("sensitive user draft should not be audited"));
    assert!(!audit_dump.contains("attachment plaintext should not be audited"));
    assert!(!audit_dump.contains("/tmp/secret-report.txt"));
    assert!(audit_dump.contains("user_delegated_personal_agent_outbound_send_not_enabled"));
    assert!(audit_dump.contains("user_delegated_personal_agent_attachment_send_not_enabled"));
}

struct TestFixture {
    _root: TempDir,
    state: DaemonState,
    identity: UserDelegatedIdentityRecord,
    binding: AppPersonalAgentBindingRecord,
}

fn insert_delegated_runtime_task_and_run(
    state: &DaemonState,
    binding: &AppPersonalAgentBindingRecord,
    task_id: &str,
    run_id: &str,
    source_message_id: &str,
) {
    let task = RuntimeTask {
        task_id: task_id.to_string(),
        agent_did: binding.runtime_agent_did.clone(),
        agent_handle: "alice-hermes".to_string(),
        controller_user_id: "user-alice".to_string(),
        controller_full_handle: "alice.anpclaw.com".to_string(),
        controller_scope_key: "controller-scope:v1:user-alice:alice.anpclaw.com".to_string(),
        controller_did: binding.user_did.clone(),
        sender_did: "did:human:bob".to_string(),
        requester_did: "did:human:bob".to_string(),
        requester_user_id: Some("user-bob".to_string()),
        requester_full_handle: Some("bob.example.com".to_string()),
        trigger_kind: RuntimeTaskTriggerKind::DelegatedDirect,
        conversation_scope: RuntimeConversationScope::direct("user-bob", "bob.example.com")
            .unwrap(),
        invocation_authority: RuntimeInvocationAuthority::Requester,
        reply_recipient_did: binding.user_did.clone(),
        conversation_id: Some("direct:did:human:bob".to_string()),
        text: serde_json::to_string(&json!({
            "schema": "awiki.runtime.user_message_task.v1",
            "source_message_id": source_message_id,
            "source_conversation_id": "direct:did:human:bob",
            "source_sender_did": "did:human:bob",
            "content_hash": "sha256:test",
            "content_text": "hello agent"
        }))
        .unwrap(),
    };
    state.insert_runtime_task(&task).unwrap();
    state
        .insert_runtime_run(&RuntimeRun {
            run_id: run_id.to_string(),
            task_id: task.task_id,
            agent_did: binding.runtime_agent_did.clone(),
            runtime_profile_id: binding.runtime_profile_id.clone(),
            runtime_plugin_id: HERMES_RUNTIME_PLUGIN_ID.to_string(),
            workspace_id: None,
            status: RuntimeRunStatus::Running,
        })
        .unwrap();
}

fn fixture() -> TestFixture {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open_with_root_key_bytes(&config, [41_u8; 32]);
    state.initialize().unwrap();
    let identity = UserDelegatedIdentityRecord {
        user_did: "did:human:alice".to_string(),
        verification_method: "did:human:alice#daemon-key-1".to_string(),
        app_instance_id: "app_1".to_string(),
        controller_did: "did:human:alice".to_string(),
        daemon_agent_did: "did:agent:daemon".to_string(),
        public_key_multibase: "z-public".to_string(),
        private_key_material: "pem-private".to_string(),
        private_key_ref_json: None,
        allowed_scopes_json: json!(["message.inbox.read.plain"]),
        status: "paired_key_received".to_string(),
        expires_at: None,
        bootstrap_id: "boot_1".to_string(),
        idempotency_key: "bootstrap:1".to_string(),
        created_at_ms: 0,
        updated_at_ms: 0,
    };
    let replay = crate::state::BootstrapReplayRecord {
        bootstrap_id: identity.bootstrap_id.clone(),
        idempotency_key: identity.idempotency_key.clone(),
        payload_hash: "hash".to_string(),
        user_did: identity.user_did.clone(),
        verification_method: identity.verification_method.clone(),
        app_instance_id: identity.app_instance_id.clone(),
        daemon_agent_did: identity.daemon_agent_did.clone(),
        status: identity.status.clone(),
        created_at_ms: 0,
        updated_at_ms: 0,
    };
    state.store_bootstrap_state(&identity, &replay).unwrap();
    state
        .upsert_runtime_agent_profile(&RuntimeAgentProfile {
            agent_did: "did:agent:hermes".to_string(),
            agent_handle: "alice-hermes".to_string(),
            controller_user_id: "user-alice".to_string(),
            controller_full_handle: "alice.anpclaw.com".to_string(),
            controller_scope_key: "controller-scope:v1:user-alice:alice.anpclaw.com".to_string(),
            controller_did: identity.user_did.clone(),
            runtime_profile_id: "profile_hermes".to_string(),
            runtime_plugin_id: HERMES_RUNTIME_PLUGIN_ID.to_string(),
            display_name: Some("Hermes".to_string()),
            preferred_language: "zh-Hans".to_string(),
            workspace_id: None,
            workspace_root: None,
            workspace_mode: None,
        })
        .unwrap();
    let binding = AppPersonalAgentBindingRecord {
        binding_id: "app-personal-agent:did:human:alice:app_1".to_string(),
        user_did: identity.user_did.clone(),
        inbox_auth_verification_method: identity.verification_method.clone(),
        app_instance_id: identity.app_instance_id.clone(),
        bootstrap_id: identity.bootstrap_id.clone(),
        idempotency_key: identity.idempotency_key.clone(),
        daemon_agent_did: identity.daemon_agent_did.clone(),
        runtime_agent_did: "did:agent:hermes".to_string(),
        runtime_profile_id: "profile_hermes".to_string(),
        role: APP_MESSAGE_HANDLER_ROLE.to_string(),
        desired_agent_json: json!({
            "role": APP_MESSAGE_HANDLER_ROLE,
            "allowed_actions": ["message.summarize_plain", "message.create_draft"]
        }),
        capability_policy_json: json!({
            "schema": crate::app_bridge::action::APP_CAPABILITIES_SCHEMA,
            "capabilities": ["message.summarize_plain", "message.create_draft"],
            "require_confirmation_for_write_actions": true
        }),
        status: "personal_agent_ready".to_string(),
        created_at_ms: 0,
        updated_at_ms: 0,
        revoked_at_ms: None,
    };
    state.upsert_app_personal_agent_binding(&binding).unwrap();
    TestFixture {
        _root: root,
        state,
        identity,
        binding,
    }
}

fn plain_message(id: &str, sender: &str, text: &str) -> Message {
    Message {
        id: MessageId::parse(id).unwrap(),
        thread: ThreadRef::Direct(PeerRef::parse(sender, "").unwrap()),
        direction: MessageDirection::Incoming,
        sender: PeerRef::parse(sender, "").unwrap(),
        receiver: Some(PeerRef::parse("did:human:alice", "").unwrap()),
        group: None,
        body: MessageBodyView::Text {
            text: text.to_string(),
            kind: MessageKind::Text,
        },
        sent_at: Some("2026-06-09T00:00:00Z".to_string()),
        received_at: Some("2026-06-09T00:00:01Z".to_string()),
        metadata: MessageMetadata {
            content_type: Some("text/plain".to_string()),
            attributes: vec![
                MessageMetadataAttribute {
                    key: "peer_user_id".to_string(),
                    value: "user-bob".to_string(),
                },
                MessageMetadataAttribute {
                    key: "peer_full_handle".to_string(),
                    value: "bob.example.com".to_string(),
                },
            ],
            ..MessageMetadata::default()
        },
    }
}

fn direct_payload_message(id: &str, sender: &str, payload: Value) -> Message {
    Message {
        id: MessageId::parse(id).unwrap(),
        thread: ThreadRef::Direct(PeerRef::parse(sender, "").unwrap()),
        direction: MessageDirection::Incoming,
        sender: PeerRef::parse(sender, "").unwrap(),
        receiver: Some(PeerRef::parse("did:human:alice", "").unwrap()),
        group: None,
        body: MessageBodyView::Payload { payload },
        sent_at: Some("2026-06-09T00:00:00Z".to_string()),
        received_at: Some("2026-06-09T00:00:01Z".to_string()),
        metadata: MessageMetadata {
            content_type: Some("application/json".to_string()),
            ..MessageMetadata::default()
        },
    }
}

fn e2ee_message(id: &str, sender: &str) -> Message {
    Message {
        id: MessageId::parse(id).unwrap(),
        thread: ThreadRef::Direct(PeerRef::parse(sender, "").unwrap()),
        direction: MessageDirection::Incoming,
        sender: PeerRef::parse(sender, "").unwrap(),
        receiver: Some(PeerRef::parse("did:human:alice", "").unwrap()),
        group: None,
        body: MessageBodyView::Unsupported {
            content_type: Some("application/anp-direct-cipher+json".to_string()),
        },
        sent_at: Some("2026-06-09T00:00:00Z".to_string()),
        received_at: Some("2026-06-09T00:00:01Z".to_string()),
        metadata: MessageMetadata {
            content_type: Some("application/anp-direct-cipher+json".to_string()),
            attributes: vec![MessageMetadataAttribute {
                key: "security".to_string(),
                value: "direct-e2ee".to_string(),
            }],
            ..MessageMetadata::default()
        },
    }
}

fn system_payload_message(id: &str, sender: &str) -> Message {
    Message {
        id: MessageId::parse(id).unwrap(),
        thread: ThreadRef::Group(GroupRef::parse("group_1").unwrap()),
        direction: MessageDirection::Incoming,
        sender: PeerRef::parse(sender, "").unwrap(),
        receiver: None,
        group: Some(GroupRef::parse("group_1").unwrap()),
        body: MessageBodyView::Payload {
            payload: json!({
                "schema": "awiki.daemon.bootstrap.v1",
                "private_key_multibase": "should-not-dispatch"
            }),
        },
        sent_at: Some("2026-06-09T00:00:00Z".to_string()),
        received_at: Some("2026-06-09T00:00:01Z".to_string()),
        metadata: MessageMetadata {
            content_type: Some("application/json".to_string()),
            ..MessageMetadata::default()
        },
    }
}

fn install_test_im_core_vault_root_key() {
    std::env::set_var(
        "AWIKI_IM_CORE_VAULT_ROOT_KEY_B64",
        URL_SAFE_NO_PAD.encode([31_u8; 32]),
    );
}

fn mention_payload_message(id: &str, sender: &str, payload: Value) -> Message {
    Message {
        id: MessageId::parse(id).unwrap(),
        thread: ThreadRef::Group(GroupRef::parse("group_1").unwrap()),
        direction: MessageDirection::Incoming,
        sender: PeerRef::parse(sender, "").unwrap(),
        receiver: None,
        group: Some(GroupRef::parse("group_1").unwrap()),
        body: MessageBodyView::Payload { payload },
        sent_at: Some("2026-06-09T00:00:00Z".to_string()),
        received_at: Some("2026-06-09T00:00:01Z".to_string()),
        metadata: MessageMetadata {
            content_type: Some("application/json".to_string()),
            ..MessageMetadata::default()
        },
    }
}

fn group_plain_message(id: &str, sender: &str, text: &str) -> Message {
    Message {
        id: MessageId::parse(id).unwrap(),
        thread: ThreadRef::Group(GroupRef::parse("group_1").unwrap()),
        direction: MessageDirection::Incoming,
        sender: PeerRef::parse(sender, "").unwrap(),
        receiver: None,
        group: Some(GroupRef::parse("group_1").unwrap()),
        body: MessageBodyView::Text {
            text: text.to_string(),
            kind: MessageKind::Text,
        },
        sent_at: Some("2026-06-09T00:00:00Z".to_string()),
        received_at: Some("2026-06-09T00:00:01Z".to_string()),
        metadata: MessageMetadata {
            content_type: Some("text/plain".to_string()),
            ..MessageMetadata::default()
        },
    }
}

fn group_e2ee_mention_cipher_message(
    id: &str,
    sender: &str,
    binding: &AppPersonalAgentBindingRecord,
) -> Message {
    Message {
        id: MessageId::parse(id).unwrap(),
        thread: ThreadRef::Group(GroupRef::parse("group_1").unwrap()),
        direction: MessageDirection::Incoming,
        sender: PeerRef::parse(sender, "").unwrap(),
        receiver: None,
        group: Some(GroupRef::parse("group_1").unwrap()),
        body: MessageBodyView::Payload {
            payload: json!({
                "text": "@Hermes encrypted",
                "mentions": [{
                    "id": "men_e2ee",
                    "range": {"start": 0, "end": 7, "unit": "unicode_code_point"},
                    "target": {"kind": "agent", "did": binding.runtime_agent_did}
                }]
            }),
        },
        sent_at: Some("2026-06-09T00:00:00Z".to_string()),
        received_at: Some("2026-06-09T00:00:01Z".to_string()),
        metadata: MessageMetadata {
            content_type: Some("application/anp-group-cipher+json".to_string()),
            attributes: vec![MessageMetadataAttribute {
                key: "security".to_string(),
                value: "group-e2ee".to_string(),
            }],
            ..MessageMetadata::default()
        },
    }
}

fn plain_dispatch(text: &str) -> AgentDispatchContent {
    AgentDispatchContent {
        text: text.to_string(),
        message_kind: "text",
    }
}
