use super::*;
use crate::internal::auth::session::SessionProvider;
use crate::internal::transport::{
    AsyncAuthenticatedRpcTransport, AsyncRpcTransport, AuthenticatedRpcTransport, RpcTransport,
};
use crate::vault::{
    FileSecretVault, FileSecretVaultStore, SealSecretRequest, SecretAccessPolicy, SecretBytes,
    SecretKind, SecretMetadata, SecretVault,
};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

#[test]
fn messages_read_runtime_builds_inbox_rpc_and_maps_page() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let runtime = MessageReadRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::clone(&calls),
            response: json!({
                "messages": [{
                    "id": "msg-inbox-1",
                    "sender_did": "did:example:bob",
                    "receiver_did": "did:example:alice",
                    "content": "hello alice",
                    "content_type": "text/plain",
                    "sent_at": "2026-05-21T00:00:00Z",
                    "server_seq": 7
                }],
                "has_more": false
            }),
        },
        NoopDirectoryTransport,
    );

    let result = runtime
        .inbox(InboxRead {
            query: crate::messages::InboxQuery {
                scope: crate::messages::InboxScope::DirectOnly,
                limit: crate::ids::PageLimit(20),
                cursor: None,
                unread_only: false,
                inbox_history_options: None,
            },
        })
        .unwrap();

    assert_eq!(result.page.items.len(), 1);
    assert_eq!(result.page.items[0].id.as_str(), "msg-inbox-1");
    assert_eq!(result.page.items[0].metadata.server_sequence, Some(7));
    let calls = calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].endpoint, MESSAGE_RPC_ENDPOINT);
    assert_eq!(calls[0].method, "inbox.get");
    assert_eq!(calls[0].params["meta"]["sender_did"], "did:example:alice");
    assert_eq!(calls[0].params["body"]["user_did"], "did:example:alice");
    assert_eq!(calls[0].params["body"]["limit"], 20);
}

#[test]
fn messages_read_runtime_group_inbox_scope_lists_groups_and_messages() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let runtime = MessageReadRuntime::new(
        &client,
        ReadyGroupSessionProvider,
        RecordingTransport {
            calls: Rc::clone(&calls),
            response: json!({
                "groups": [{"group_did": "did:example:group"}],
                "messages": [{
                    "id": "msg-group-inbox-1",
                    "sender_did": "did:example:bob",
                    "content": "hello group inbox",
                    "content_type": "text/plain",
                    "group_event_seq": 12
                }],
                "has_more": false
            }),
        },
        NoopDirectoryTransport,
    );

    let result = runtime
        .inbox(InboxRead {
            query: crate::messages::InboxQuery {
                scope: crate::messages::InboxScope::GroupOnly,
                limit: crate::ids::PageLimit(20),
                cursor: None,
                unread_only: false,
                inbox_history_options: None,
            },
        })
        .unwrap();

    assert_eq!(result.page.items.len(), 1);
    let message = &result.page.items[0];
    assert_eq!(message.id.as_str(), "did:example:group:12");
    assert_eq!(
        message.thread,
        crate::messages::ThreadRef::Group(
            crate::ids::GroupRef::parse("did:example:group").unwrap()
        )
    );
    let calls = calls.borrow();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].method, "group.list");
    assert_eq!(calls[1].method, "group.list_messages");
    assert_eq!(calls[1].params["body"]["group_did"], "did:example:group");
}

#[test]
fn messages_read_runtime_builds_delegated_inbox_auth_and_filters_e2ee() {
    let fixture = Fixture::new();
    let delegated = fixture.write_delegated_identity();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let runtime = MessageReadRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::clone(&calls),
            response: json!({
                "messages": [
                    {
                        "id": "msg-plain-delegated",
                        "sender_did": "did:example:bob",
                        "receiver_did": delegated.user_did,
                        "content": "plain delegated",
                        "content_type": "text/plain",
                        "server_seq": 8
                    },
                    {
                        "id": "msg-e2ee-delegated",
                        "sender_did": "did:example:bob",
                        "receiver_did": delegated.user_did,
                        "content_type": "application/anp-direct-cipher+json",
                        "security_profile": "direct-e2ee",
                        "content": {"ciphertext": "opaque"}
                    }
                ],
                "has_more": false
            }),
        },
        NoopDirectoryTransport,
    );

    let result = runtime
        .inbox(InboxRead {
            query: crate::messages::InboxQuery {
                scope: crate::messages::InboxScope::All,
                limit: crate::ids::PageLimit(20),
                cursor: None,
                unread_only: false,
                inbox_history_options: Some(crate::messages::InboxHistoryOptions {
                    inbox_owner_did: Some(delegated.user_did.clone()),
                    inbox_auth_verification_method: Some(delegated.verification_method.clone()),
                    inbox_auth_key_ref: Some(format!(
                        "file:{}",
                        delegated.private_key_path.display()
                    )),
                    inbox_auth: None,
                }),
            },
        })
        .unwrap();

    assert_eq!(result.page.items.len(), 1);
    assert_eq!(result.page.items[0].id.as_str(), "msg-plain-delegated");
    let calls = calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "inbox.get");
    assert_eq!(calls[0].params["meta"]["sender_did"], delegated.user_did);
    assert_eq!(calls[0].params["body"]["user_did"], delegated.user_did);
    assert_eq!(
        calls[0].params["body"]["inbox_auth_verification_method"],
        delegated.verification_method
    );
    assert!(calls[0].params["auth"]["origin_proof"]["signatureInput"]
        .as_str()
        .expect("signature input")
        .contains(&format!("keyid=\"{}\"", delegated.verification_method)));
}

#[test]
fn messages_read_runtime_uses_vault_delegated_inbox_key_ref() {
    install_test_im_core_vault_root_key();
    let fixture = Fixture::new();
    let delegated = fixture.write_delegated_identity();
    let client = fixture.client();
    let inbox_auth_key_ref = delegated.seal_to_vault_key_ref(&client);
    let calls = Rc::new(RefCell::new(Vec::new()));
    let runtime = MessageReadRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::clone(&calls),
            response: json!({
                "messages": [{
                    "id": "msg-vault-delegated",
                    "sender_did": "did:example:bob",
                    "receiver_did": delegated.user_did,
                    "content": "plain vault delegated",
                    "content_type": "text/plain",
                    "server_seq": 18
                }],
                "has_more": false
            }),
        },
        NoopDirectoryTransport,
    );

    let result = runtime
        .inbox(InboxRead {
            query: crate::messages::InboxQuery {
                scope: crate::messages::InboxScope::All,
                limit: crate::ids::PageLimit(20),
                cursor: None,
                unread_only: false,
                inbox_history_options: Some(crate::messages::InboxHistoryOptions {
                    inbox_owner_did: Some(delegated.user_did.clone()),
                    inbox_auth_verification_method: Some(delegated.verification_method.clone()),
                    inbox_auth_key_ref: Some(inbox_auth_key_ref),
                    inbox_auth: None,
                }),
            },
        })
        .unwrap();

    assert_eq!(result.page.items.len(), 1);
    assert_eq!(result.page.items[0].id.as_str(), "msg-vault-delegated");
    let calls = calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].params["meta"]["sender_did"], delegated.user_did);
    assert_eq!(calls[0].params["body"]["user_did"], delegated.user_did);
    assert!(calls[0].params["auth"]["origin_proof"]["signatureInput"]
        .as_str()
        .expect("signature input")
        .contains(&format!("keyid=\"{}\"", delegated.verification_method)));
}

#[test]
fn messages_read_runtime_annotates_delegated_inbox_peer_scope_from_handle_lookup() {
    let fixture = Fixture::new();
    let delegated = fixture.write_delegated_identity();
    let client = fixture.client();
    let runtime = MessageReadRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response: json!({
                "messages": [{
                    "id": "msg-plain-delegated-scope",
                    "sender_did": "did:example:bob-new",
                    "receiver_did": delegated.user_did,
                    "content": "plain delegated scope",
                    "content_type": "text/plain",
                    "server_seq": 9
                }],
                "has_more": false
            }),
        },
        StaticHandleDirectoryTransport,
    );

    let result = runtime
        .inbox(InboxRead {
            query: crate::messages::InboxQuery {
                scope: crate::messages::InboxScope::All,
                limit: crate::ids::PageLimit(20),
                cursor: None,
                unread_only: false,
                inbox_history_options: Some(crate::messages::InboxHistoryOptions {
                    inbox_owner_did: Some(delegated.user_did.clone()),
                    inbox_auth_verification_method: Some(delegated.verification_method.clone()),
                    inbox_auth_key_ref: Some(format!(
                        "file:{}",
                        delegated.private_key_path.display()
                    )),
                    inbox_auth: None,
                }),
            },
        })
        .unwrap();

    assert_eq!(result.page.items.len(), 1);
    let message = &result.page.items[0];
    assert_eq!(
        message
            .metadata
            .attributes
            .iter()
            .find(|attribute| attribute.key == "peer_user_id")
            .map(|attribute| attribute.value.as_str()),
        Some("user-bob")
    );
    assert_eq!(
        message
            .metadata
            .attributes
            .iter()
            .find(|attribute| attribute.key == "peer_full_handle")
            .map(|attribute| attribute.value.as_str()),
        Some("bob.anpclaw.com")
    );
    assert_eq!(
        message
            .metadata
            .attributes
            .iter()
            .find(|attribute| attribute.key == "peer_current_did")
            .map(|attribute| attribute.value.as_str()),
        Some("did:example:bob-new")
    );
}

#[test]
fn messages_read_runtime_projects_direct_thread_as_current_identity_peer() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let runtime = MessageReadRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response: json!({
                "messages": [
                    {
                        "id": "msg-incoming",
                        "sender_did": "did:example:bob",
                        "receiver_did": "did:example:alice",
                        "content": "hello alice",
                        "content_type": "text/plain"
                    },
                    {
                        "id": "msg-outgoing",
                        "sender_did": "did:example:alice",
                        "receiver_did": "did:example:bob",
                        "content": "hello bob",
                        "content_type": "text/plain",
                        "direction": 1
                    }
                ],
                "has_more": false
            }),
        },
        NoopDirectoryTransport,
    );

    let result = runtime
        .inbox(InboxRead {
            query: crate::messages::InboxQuery {
                scope: crate::messages::InboxScope::DirectOnly,
                limit: crate::ids::PageLimit(20),
                cursor: None,
                unread_only: false,
                inbox_history_options: None,
            },
        })
        .unwrap();

    assert_eq!(result.page.items.len(), 2);
    for message in &result.page.items {
        assert!(matches!(
            &message.thread,
            crate::messages::ThreadRef::Direct(peer)
                if peer.as_str() == "did:example:bob"
        ));
    }
}

#[test]
fn messages_read_runtime_rejects_wrong_delegated_history_owner_locally() {
    let fixture = Fixture::new();
    let delegated = fixture.write_delegated_identity();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let runtime = MessageReadRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::clone(&calls),
            response: json!({}),
        },
        NoopDirectoryTransport,
    );

    let error = runtime
        .history(HistoryRead {
            thread: crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
            query: crate::messages::HistoryQuery {
                limit: crate::ids::PageLimit(5),
                cursor: None,
                inbox_history_options: Some(crate::messages::InboxHistoryOptions {
                    inbox_owner_did: Some("did:example:other".to_owned()),
                    inbox_auth_verification_method: Some(delegated.verification_method),
                    inbox_auth_key_ref: Some(format!(
                        "file:{}",
                        delegated.private_key_path.display()
                    )),
                    inbox_auth: None,
                }),
            },
            resolved_peer_did: None,
            peer_scope: None,
        })
        .unwrap_err();

    assert!(matches!(
        error,
        crate::ImError::InvalidInput {
            field: Some(field),
            ..
        } if field == "inbox_owner_did"
    ));
    assert!(calls.borrow().is_empty());
}

#[test]
fn messages_read_runtime_rejects_missing_delegated_inbox_key_locally() {
    let fixture = Fixture::new();
    let delegated = fixture.write_delegated_identity();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let runtime = MessageReadRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::clone(&calls),
            response: json!({}),
        },
        NoopDirectoryTransport,
    );

    let error = runtime
        .inbox(InboxRead {
            query: crate::messages::InboxQuery {
                scope: crate::messages::InboxScope::All,
                limit: crate::ids::PageLimit(20),
                cursor: None,
                unread_only: false,
                inbox_history_options: Some(crate::messages::InboxHistoryOptions {
                    inbox_owner_did: Some(delegated.user_did),
                    inbox_auth_verification_method: Some(delegated.verification_method),
                    inbox_auth_key_ref: Some("local:missing-daemon-key".to_owned()),
                    inbox_auth: None,
                }),
            },
        })
        .unwrap_err();

    assert!(matches!(
        error,
        crate::ImError::CredentialFileUnreadable { path_kind, .. }
            if path_kind == "delegated_private_key"
    ));
    assert!(calls.borrow().is_empty());
}

#[test]
fn messages_read_runtime_rejects_delegated_group_history_locally() {
    let fixture = Fixture::new();
    let delegated = fixture.write_delegated_identity();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let runtime = MessageReadRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::clone(&calls),
            response: json!({}),
        },
        NoopDirectoryTransport,
    );

    let error = runtime
        .history(HistoryRead {
            thread: crate::messages::ThreadRef::Group(
                crate::ids::GroupRef::parse("did:example:groups:team").unwrap(),
            ),
            query: crate::messages::HistoryQuery {
                limit: crate::ids::PageLimit(5),
                cursor: None,
                inbox_history_options: Some(crate::messages::InboxHistoryOptions {
                    inbox_owner_did: Some(delegated.user_did),
                    inbox_auth_verification_method: Some(delegated.verification_method),
                    inbox_auth_key_ref: Some(format!(
                        "file:{}",
                        delegated.private_key_path.display()
                    )),
                    inbox_auth: None,
                }),
            },
            resolved_peer_did: None,
            peer_scope: None,
        })
        .unwrap_err();

    assert!(matches!(
        error,
        crate::ImError::UnsupportedCapability { capability }
            if capability == "delegated-group-history"
    ));
    assert!(calls.borrow().is_empty());
}

#[test]
fn messages_read_runtime_rejects_scoped_inbox_token_until_enabled() {
    let fixture = Fixture::new();
    let delegated = fixture.write_delegated_identity();
    let client = fixture.client();
    let runtime = MessageReadRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response: json!({}),
        },
        NoopDirectoryTransport,
    );

    let error = runtime
        .inbox(InboxRead {
            query: crate::messages::InboxQuery {
                scope: crate::messages::InboxScope::All,
                limit: crate::ids::PageLimit(20),
                cursor: None,
                unread_only: false,
                inbox_history_options: Some(crate::messages::InboxHistoryOptions {
                    inbox_owner_did: Some(delegated.user_did),
                    inbox_auth_verification_method: Some(delegated.verification_method),
                    inbox_auth_key_ref: Some(format!(
                        "file:{}",
                        delegated.private_key_path.display()
                    )),
                    inbox_auth: Some(crate::messages::InboxAuth::ScopedInboxToken {
                        token: crate::messages::ScopedInboxToken {
                            token: "token".to_owned(),
                        },
                    }),
                }),
            },
        })
        .unwrap_err();

    assert!(matches!(
        error,
        crate::ImError::UnsupportedCapability { capability }
            if capability == "scoped-inbox-token"
    ));
}

#[test]
fn messages_read_runtime_persists_inbox_projection_for_conversations() {
    let fixture = Fixture::new();
    fixture.seed_verified_peer_identity("user-bob", "bob.awiki.test", &["did:example:bob"]);
    let client = fixture.client();
    let runtime = MessageReadRuntime::new(
        &client,
        ReadyAnyReadSessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response: json!({
                "messages": [{
                    "id": "msg-inbox-projected",
                    "sender_did": "did:example:bob",
                    "receiver_did": "did:example:alice",
                    "content": "restored from inbox",
                    "content_type": "text/plain",
                    "sent_at": "2026-05-21T00:00:00Z"
                }]
            }),
        },
        NoopDirectoryTransport,
    );

    runtime
        .inbox(InboxRead {
            query: crate::messages::InboxQuery {
                scope: crate::messages::InboxScope::All,
                limit: crate::ids::PageLimit(20),
                cursor: None,
                unread_only: false,
                inbox_history_options: None,
            },
        })
        .unwrap();

    let conversations =
        crate::internal::message_runtime::conversations::MessageConversationRuntime::new(&client)
            .conversations(crate::messages::ConversationQuery {
                limit: crate::ids::PageLimit(10),
                cursor: None,
                include_groups: true,
                include_direct: true,
                unread_only: false,
            })
            .unwrap();

    assert_eq!(conversations.items.len(), 1);
    let conversation = &conversations.items[0];
    assert_eq!(
        conversation.last_message.as_ref().unwrap().id.as_str(),
        "msg-inbox-projected"
    );
    assert_eq!(
        conversation.last_message_at.as_deref(),
        Some("2026-05-21T00:00:00Z")
    );
    assert_eq!(conversation.unread_count, 1);
    assert!(matches!(
        conversation.thread,
        crate::messages::ThreadRef::Direct(_)
    ));
}

#[test]
fn messages_read_runtime_projects_direct_inbox_by_peer_scope() {
    let fixture = Fixture::new();
    fixture.seed_verified_peer_identity(
        "user-bob",
        "bob.anpclaw.com",
        &["did:example:bob-old", "did:example:bob-new"],
    );
    let client = fixture.client();
    let runtime = MessageReadRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response: json!({
                "messages": [
                    {
                        "id": "msg-bob-old",
                        "sender_did": "did:example:bob-old",
                        "receiver_did": "did:example:alice",
                        "content": "old did",
                        "content_type": "text/plain",
                        "sent_at": "2026-05-21T00:00:00Z"
                    },
                    {
                        "id": "msg-bob-new",
                        "sender_did": "did:example:bob-new",
                        "receiver_did": "did:example:alice",
                        "content": "new did",
                        "content_type": "text/plain",
                        "sent_at": "2026-05-21T00:00:01Z"
                    }
                ]
            }),
        },
        StaticHandleDirectoryTransport,
    );

    runtime
        .inbox(InboxRead {
            query: crate::messages::InboxQuery {
                scope: crate::messages::InboxScope::DirectOnly,
                limit: crate::ids::PageLimit(20),
                cursor: None,
                unread_only: false,
                inbox_history_options: None,
            },
        })
        .unwrap();

    let conversations =
        crate::internal::message_runtime::conversations::MessageConversationRuntime::new(&client)
            .conversations(crate::messages::ConversationQuery {
                limit: crate::ids::PageLimit(10),
                cursor: None,
                include_groups: false,
                include_direct: true,
                unread_only: false,
            })
            .unwrap();

    assert_eq!(conversations.items.len(), 1);
    let conversation = &conversations.items[0];
    assert_eq!(conversation.message_count, 2);
    assert!(matches!(
        &conversation.thread,
        crate::messages::ThreadRef::Thread(thread)
            if thread.as_str().starts_with("dm:peer-scope:v1:")
    ));
    assert_eq!(conversation.participants[0].as_str(), "bob.anpclaw.com");
    assert_eq!(
        conversation
            .last_message
            .as_ref()
            .unwrap()
            .metadata
            .attributes
            .iter()
            .find(|attribute| attribute.key == "peer_user_id")
            .map(|attribute| attribute.value.as_str()),
        Some("user-bob")
    );
}

#[test]
fn messages_read_runtime_preserves_remote_read_state_in_projection() {
    let fixture = Fixture::new();
    fixture.seed_verified_peer_identity("user-bob", "bob.awiki.test", &["did:example:bob"]);
    let client = fixture.client();
    let runtime = MessageReadRuntime::new(
        &client,
        ReadyAnyReadSessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response: json!({
                "messages": [{
                    "id": "msg-inbox-read",
                    "sender_did": "did:example:bob",
                    "receiver_did": "did:example:alice",
                    "content": "already read",
                    "content_type": "text/plain",
                    "sent_at": "2026-05-21T00:00:01Z",
                    "is_read": true
                }]
            }),
        },
        NoopDirectoryTransport,
    );

    runtime
        .inbox(InboxRead {
            query: crate::messages::InboxQuery {
                scope: crate::messages::InboxScope::All,
                limit: crate::ids::PageLimit(20),
                cursor: None,
                unread_only: false,
                inbox_history_options: None,
            },
        })
        .unwrap();

    let conversations =
        crate::internal::message_runtime::conversations::MessageConversationRuntime::new(&client)
            .conversations(crate::messages::ConversationQuery {
                limit: crate::ids::PageLimit(10),
                cursor: None,
                include_groups: true,
                include_direct: true,
                unread_only: false,
            })
            .unwrap();

    assert_eq!(conversations.items.len(), 1);
    assert_eq!(conversations.items[0].unread_count, 0);
}

#[test]
fn message_state_read_projection_maps_failed_retry_plan() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let runtime = MessageReadRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response: json!({
                "messages": [{
                    "id": "msg-read-failed",
                    "sender_did": "did:example:alice",
                    "receiver_did": "did:example:bob",
                    "content": "hello bob",
                    "content_type": "text/plain",
                    "operation_id": "op-read-failed",
                    "delivery_state": "failed",
                    "failure_reason": "timeout"
                }]
            }),
        },
        NoopDirectoryTransport,
    );

    let result = runtime
        .history(HistoryRead {
            thread: crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
            query: crate::messages::HistoryQuery {
                limit: crate::ids::PageLimit(5),
                cursor: None,
                inbox_history_options: None,
            },
            resolved_peer_did: None,
            peer_scope: None,
        })
        .unwrap();

    let metadata = &result.page.items[0].metadata;
    let send_state = metadata.send_state.as_ref().unwrap();
    assert_eq!(
        send_state.state,
        crate::messages::MessageSendStateKind::Failed
    );
    assert_eq!(send_state.reason.as_deref(), Some("timeout"));
    let retry_plan = metadata.retry_plan.as_ref().unwrap();
    assert!(retry_plan.retryable);
    assert_eq!(
        retry_plan.action,
        crate::messages::MessageRetryAction::RetryDirectText
    );
    assert_eq!(retry_plan.operation_id.as_deref(), Some("op-read-failed"));
}

#[test]
fn messages_read_runtime_builds_direct_history_rpc() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let runtime = MessageReadRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::clone(&calls),
            response: json!({
                "messages": [{
                    "id": "msg-history-1",
                    "sender_did": "did:example:alice",
                    "receiver_did": "did:example:bob",
                    "content": "hello bob",
                    "content_type": "text/plain"
                }]
            }),
        },
        NoopDirectoryTransport,
    );

    let result = runtime
        .history(HistoryRead {
            thread: crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("bob.awiki.test", "").unwrap(),
            ),
            query: crate::messages::HistoryQuery {
                limit: crate::ids::PageLimit(5),
                cursor: Some(crate::ids::Cursor::parse("42").unwrap()),
                inbox_history_options: None,
            },
            resolved_peer_did: Some("did:example:bob".to_string()),
            peer_scope: None,
        })
        .unwrap();

    assert_eq!(result.page.items.len(), 1);
    let calls = calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "direct.get_history");
    assert_eq!(calls[0].params["body"]["peer_did"], "did:example:bob");
    assert_eq!(calls[0].params["body"]["limit"], 5);
    assert_eq!(calls[0].params["body"]["since_seq"], "42");
}

#[test]
fn messages_read_runtime_group_history_merges_committed_local_projection() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let connection = crate::internal::local_state::open_writable(&fixture.sqlite_path()).unwrap();
    crate::internal::local_state::messages::upsert_message(
        &connection,
        &crate::internal::local_state::messages::MessageRecord {
            msg_id: "msg-local-group-send".to_owned(),
            owner_identity_id: "alice-id".to_owned(),
            owner_did: "did:example:alice".to_owned(),
            conversation_id: "group:did:example:group".to_owned(),
            thread_id: "group:did:example:group".to_owned(),
            direction: 1,
            sender_did: "did:example:alice".to_owned(),
            group_id: "did:example:group".to_owned(),
            group_did: "did:example:group".to_owned(),
            content_type: "text/plain".to_owned(),
            content: "local group send".to_owned(),
            sent_at: "2026-05-21T00:00:03Z".to_owned(),
            stored_at: "2026-05-21T00:00:03Z".to_owned(),
            server_seq: Some(77),
            is_read: true,
            metadata: json!({
                "content_type": "text/plain",
                "server_sequence": 77,
                "delivery_state": "sent"
            })
            .to_string(),
            ..crate::internal::local_state::messages::MessageRecord::default()
        },
    )
    .unwrap();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let runtime = MessageReadRuntime::new(
        &client,
        ReadyGroupSessionProvider,
        RecordingTransport {
            calls: Rc::clone(&calls),
            response: json!({
                "messages": [],
                "has_more": false
            }),
        },
        NoopDirectoryTransport,
    );

    let result = runtime
        .history(HistoryRead {
            thread: crate::messages::ThreadRef::Group(
                crate::ids::GroupRef::parse("did:example:group").unwrap(),
            ),
            query: crate::messages::HistoryQuery {
                limit: crate::ids::PageLimit(20),
                cursor: None,
                inbox_history_options: None,
            },
            resolved_peer_did: None,
            peer_scope: None,
        })
        .unwrap();

    assert_eq!(result.page.items.len(), 1);
    let message = &result.page.items[0];
    assert_eq!(message.id.as_str(), "did:example:group:77");
    assert_eq!(
        message
            .metadata
            .conversation_identity
            .as_ref()
            .unwrap()
            .conversation_id,
        "group:did:example:group"
    );
    assert_eq!(message.metadata.server_sequence, Some(77));
    assert_eq!(
        calls.borrow()[0].method,
        "group.list_messages",
        "remote sync should still run before the committed projection is read"
    );
}

#[test]
fn messages_read_runtime_local_history_reads_projection_without_rpc() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let connection = crate::internal::local_state::open_writable(&fixture.sqlite_path()).unwrap();
    for (msg_id, sent_at) in [
        ("msg-local-old", "2026-05-21T00:00:00Z"),
        ("msg-local-new", "2026-05-21T00:00:01Z"),
    ] {
        crate::internal::local_state::messages::upsert_message(
            &connection,
            &crate::internal::local_state::messages::MessageRecord {
                msg_id: msg_id.to_owned(),
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:example:alice".to_owned(),
                conversation_id: "dm:did:example:bob".to_owned(),
                thread_id: "dm:did:example:bob".to_owned(),
                direction: 0,
                sender_did: "did:example:bob".to_owned(),
                receiver_did: "did:example:alice".to_owned(),
                content_type: "text/plain".to_owned(),
                content: msg_id.to_owned(),
                sent_at: sent_at.to_owned(),
                stored_at: sent_at.to_owned(),
                ..crate::internal::local_state::messages::MessageRecord::default()
            },
        )
        .unwrap();
    }
    let calls = Rc::new(RefCell::new(Vec::new()));
    let runtime = MessageReadRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::clone(&calls),
            response: json!({"messages": [{"id": "remote-should-not-be-used"}]}),
        },
        NoopDirectoryTransport,
    );

    let result = runtime
        .local_history(LocalHistoryRead {
            thread: crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
            query: crate::messages::LocalHistoryQuery {
                limit: crate::ids::PageLimit(1),
                cursor: None,
            },
        })
        .unwrap();

    assert!(calls.borrow().is_empty());
    assert_eq!(result.raw["source"], "local");
    assert_eq!(result.page.items.len(), 1);
    assert_eq!(result.page.items[0].id.as_str(), "msg-local-new");
    assert!(result.page.has_more);
    let cursor = result.page.next_cursor.clone().expect("local cursor");
    let next = MessageReadRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::clone(&calls),
            response: json!({"messages": [{"id": "remote-should-not-be-used"}]}),
        },
        NoopDirectoryTransport,
    )
    .local_history(LocalHistoryRead {
        thread: crate::messages::ThreadRef::Direct(
            crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
        ),
        query: crate::messages::LocalHistoryQuery {
            limit: crate::ids::PageLimit(1),
            cursor: Some(cursor),
        },
    })
    .unwrap();

    assert!(calls.borrow().is_empty());
    assert_eq!(next.page.items.len(), 1);
    assert_eq!(next.page.items[0].id.as_str(), "msg-local-old");
    assert!(!next.page.has_more);
}

#[test]
fn messages_read_runtime_local_history_preserves_peer_scope_without_metadata() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let connection = crate::internal::local_state::open_writable(&fixture.sqlite_path()).unwrap();
    let peer_scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
        "user-runtime-agent",
        "runtime-agent.awiki.test",
    )
    .unwrap();
    let conversation_id =
        crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(
            &peer_scope,
        );
    crate::internal::local_state::messages::upsert_message(
        &connection,
        &crate::internal::local_state::messages::MessageRecord {
            msg_id: "runtime-final:msg-local-runtime-reply".to_owned(),
            owner_identity_id: "alice-id".to_owned(),
            owner_did: "did:example:alice".to_owned(),
            conversation_id: conversation_id.clone(),
            thread_id: conversation_id.clone(),
            direction: 0,
            sender_did: "did:example:agent-runtime:e1_current".to_owned(),
            receiver_did: "did:example:alice".to_owned(),
            content_type: "text/plain".to_owned(),
            content: "runtime reply".to_owned(),
            sent_at: "2026-05-21T00:00:02Z".to_owned(),
            stored_at: "2026-05-21T00:00:02Z".to_owned(),
            server_seq: Some(42),
            metadata: r#"{"content_type":"text/plain","server_sequence":42}"#.to_owned(),
            ..crate::internal::local_state::messages::MessageRecord::default()
        },
    )
    .unwrap();
    let runtime = MessageReadRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response: json!({"messages": [{"id": "remote-should-not-be-used"}]}),
        },
        NoopDirectoryTransport,
    );

    let result = runtime
        .local_history(LocalHistoryRead {
            thread: crate::messages::ThreadRef::Thread(
                crate::ids::ThreadId::parse(&conversation_id).unwrap(),
            ),
            query: crate::messages::LocalHistoryQuery {
                limit: crate::ids::PageLimit(10),
                cursor: None,
            },
        })
        .unwrap();

    assert_eq!(result.page.items.len(), 1);
    let message = &result.page.items[0];
    assert_eq!(message.id.as_str(), "runtime-final:msg-local-runtime-reply");
    assert!(matches!(
        &message.thread,
        crate::messages::ThreadRef::Thread(thread) if thread.as_str() == conversation_id
    ));
}

#[test]
fn messages_read_runtime_local_history_restores_legacy_attachment_manifest_projection() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let connection = crate::internal::local_state::open_writable(&fixture.sqlite_path()).unwrap();
    let manifest = json!({
        "attachments": [{
            "attachment_id": "att-legacy-1",
            "filename": "report.md",
            "mime_type": "text/markdown",
            "size": "24",
            "access_info": {
                "object_uri": "https://objects.example/att-legacy-1"
            }
        }],
        "caption": "@codex 看看这个文件",
        "primary_attachment_id": "att-legacy-1"
    });
    crate::internal::local_state::messages::upsert_message(
        &connection,
        &crate::internal::local_state::messages::MessageRecord {
            msg_id: "msg-local-attachment".to_owned(),
            owner_identity_id: "alice-id".to_owned(),
            owner_did: "did:example:alice".to_owned(),
            conversation_id: "group:did:example:group".to_owned(),
            thread_id: "group:did:example:group".to_owned(),
            direction: 0,
            sender_did: "did:example:bob".to_owned(),
            group_id: "did:example:group".to_owned(),
            group_did: "did:example:group".to_owned(),
            content_type: "application/json".to_owned(),
            content: manifest.to_string(),
            metadata: json!({
                "content_type": crate::attachments::manifest::attachment_manifest_content_type(),
                "group_event_seq": "44",
                "server_sequence": 44
            })
            .to_string(),
            server_seq: Some(44),
            sent_at: "2026-05-21T00:00:02Z".to_owned(),
            stored_at: "2026-05-21T00:00:02Z".to_owned(),
            ..crate::internal::local_state::messages::MessageRecord::default()
        },
    )
    .unwrap();

    let runtime = MessageReadRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response: json!({"messages": [{"id": "remote-should-not-be-used"}]}),
        },
        NoopDirectoryTransport,
    );
    let result = runtime
        .local_history(LocalHistoryRead {
            thread: crate::messages::ThreadRef::Group(
                crate::ids::GroupRef::parse("did:example:group").unwrap(),
            ),
            query: crate::messages::LocalHistoryQuery {
                limit: crate::ids::PageLimit(10),
                cursor: None,
            },
        })
        .unwrap();

    assert_eq!(result.page.items.len(), 1);
    let message = &result.page.items[0];
    assert_eq!(
        message.metadata.content_type.as_deref(),
        Some(crate::attachments::manifest::attachment_manifest_content_type())
    );
    assert!(matches!(
        &message.body,
        crate::messages::MessageBodyView::Payload { payload }
            if payload["attachments"][0]["attachment_id"] == "att-legacy-1"
                && payload["caption"] == "@codex 看看这个文件"
    ));
}

#[test]
fn messages_read_runtime_merges_local_direct_projection_into_history() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let connection = crate::internal::local_state::open_writable(&fixture.sqlite_path()).unwrap();
    let peer_scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
        "user-bob",
        "bob.awiki.test",
    )
    .unwrap();
    let peer_scope_conversation_id =
        crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(
            &peer_scope,
        );
    crate::internal::local_state::messages::upsert_message(
        &connection,
        &crate::internal::local_state::messages::MessageRecord {
            msg_id: "msg-local-outgoing".to_owned(),
            owner_identity_id: "alice-id".to_owned(),
            owner_did: "did:example:alice".to_owned(),
            conversation_id: "dm:did:example:bob".to_owned(),
            thread_id: "dm:did:example:bob".to_owned(),
            direction: 1,
            sender_did: "did:example:alice".to_owned(),
            receiver_did: "did:example:bob".to_owned(),
            content_type: "text/plain".to_owned(),
            content: "question from local projection".to_owned(),
            sent_at: "2026-05-21T00:00:00Z".to_owned(),
            stored_at: "2026-05-21T00:00:00Z".to_owned(),
            is_read: true,
            ..crate::internal::local_state::messages::MessageRecord::default()
        },
    )
    .unwrap();
    crate::internal::local_state::messages::upsert_message(
        &connection,
        &crate::internal::local_state::messages::MessageRecord {
            msg_id: "msg-local-scoped-outgoing".to_owned(),
            owner_identity_id: "alice-id".to_owned(),
            owner_did: "did:example:alice".to_owned(),
            conversation_id: peer_scope_conversation_id.clone(),
            thread_id: peer_scope_conversation_id,
            direction: 1,
            sender_did: "did:example:alice".to_owned(),
            receiver_did: "did:example:bob".to_owned(),
            content_type: "text/plain".to_owned(),
            content: "scoped question from local projection".to_owned(),
            sent_at: "2026-05-21T00:00:02Z".to_owned(),
            stored_at: "2026-05-21T00:00:02Z".to_owned(),
            metadata: r#"{"peer_user_id":"user-bob","peer_full_handle":"bob.awiki.test","peer_current_did":"did:example:bob"}"#.to_owned(),
            is_read: true,
            ..crate::internal::local_state::messages::MessageRecord::default()
        },
    )
    .unwrap();
    let runtime = MessageReadRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response: json!({
                "messages": [{
                    "id": "msg-remote-incoming",
                    "sender_did": "did:example:bob",
                    "receiver_did": "did:example:alice",
                    "content": "answer from remote history",
                    "content_type": "text/plain",
                    "sent_at": "2026-05-21T00:00:01Z"
                }],
                "has_more": false
            }),
        },
        NoopDirectoryTransport,
    );

    let result = runtime
        .history(HistoryRead {
            thread: crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
            query: crate::messages::HistoryQuery {
                limit: crate::ids::PageLimit(10),
                cursor: None,
                inbox_history_options: None,
            },
            resolved_peer_did: None,
            peer_scope: Some(peer_scope),
        })
        .unwrap();

    assert_eq!(
        result
            .page
            .items
            .iter()
            .map(|message| message.id.as_str())
            .collect::<Vec<_>>(),
        vec![
            "msg-local-scoped-outgoing",
            "msg-remote-incoming",
            "msg-local-outgoing"
        ]
    );
}

#[test]
fn messages_read_runtime_uses_committed_direct_projection_for_remote_duplicate() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let connection = crate::internal::local_state::open_writable(&fixture.sqlite_path()).unwrap();
    let peer_scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
        "user-bob",
        "bob.awiki.test",
    )
    .unwrap();
    let conversation_id =
        crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(
            &peer_scope,
        );
    crate::internal::local_state::messages::upsert_message(
        &connection,
        &crate::internal::local_state::messages::MessageRecord {
            msg_id: "msg-overlap".to_owned(),
            owner_identity_id: "alice-id".to_owned(),
            owner_did: "did:example:alice".to_owned(),
            conversation_id: conversation_id.clone(),
            thread_id: conversation_id.clone(),
            direction: 1,
            sender_did: "did:example:alice".to_owned(),
            receiver_did: "did:example:bob".to_owned(),
            content_type: "text/plain".to_owned(),
            content: "one committed message".to_owned(),
            sent_at: "2026-05-21T00:00:00Z".to_owned(),
            stored_at: "2026-05-21T00:00:00Z".to_owned(),
            metadata: r#"{"peer_user_id":"user-bob","peer_full_handle":"bob.awiki.test","peer_current_did":"did:example:bob"}"#.to_owned(),
            is_read: true,
            ..crate::internal::local_state::messages::MessageRecord::default()
        },
    )
    .unwrap();
    let runtime = MessageReadRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response: json!({
                "messages": [{
                    "id": "msg-overlap",
                    "sender_did": "did:example:alice",
                    "receiver_did": "did:example:bob",
                    "content": "one committed message",
                    "content_type": "text/plain",
                    "sent_at": "2026-05-21T00:00:00Z",
                    "direction": 1
                }],
                "has_more": false
            }),
        },
        NoopDirectoryTransport,
    );

    let result = runtime
        .history(HistoryRead {
            thread: crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
            query: crate::messages::HistoryQuery {
                limit: crate::ids::PageLimit(10),
                cursor: None,
                inbox_history_options: None,
            },
            resolved_peer_did: None,
            peer_scope: Some(peer_scope),
        })
        .unwrap();

    assert_eq!(result.page.items.len(), 1);
    let message = &result.page.items[0];
    assert_eq!(message.id.as_str(), "msg-overlap");
    assert_eq!(
        message
            .metadata
            .conversation_identity
            .as_ref()
            .map(|identity| identity.conversation_id.as_str()),
        Some(conversation_id.as_str())
    );
}

#[test]
fn messages_read_runtime_uses_remote_created_at_as_sent_at() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let runtime = MessageReadRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response: json!({
                "messages": [{
                    "id": "msg-history-created-at",
                    "sender_did": "did:example:bob",
                    "receiver_did": "did:example:alice",
                    "content": "created timestamp",
                    "content_type": "text/plain",
                    "created_at": "2026-05-21T03:04:05Z"
                }]
            }),
        },
        NoopDirectoryTransport,
    );

    let result = runtime
        .history(HistoryRead {
            thread: crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
            query: crate::messages::HistoryQuery {
                limit: crate::ids::PageLimit(5),
                cursor: None,
                inbox_history_options: None,
            },
            resolved_peer_did: None,
            peer_scope: None,
        })
        .unwrap();

    assert_eq!(
        result.page.items[0].sent_at.as_deref(),
        Some("2026-05-21T03:04:05Z")
    );
}

#[tokio::test]
async fn messages_read_runtime_emits_conversation_patch_after_history_projection_commit() {
    let fixture = Fixture::new();
    fixture.seed_verified_peer_identity("user-bob", "bob.awiki.test", &["did:example:bob"]);
    let client = fixture.client();
    let mut session = client
        .messages()
        .watch_conversation_patches()
        .expect("conversation patch session");
    assert!(matches!(
        session.next_patch().await,
        Some(crate::messages::ConversationStorePatch::Reset { .. })
    ));
    let runtime = MessageReadRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response: json!({
                "messages": [{
                    "id": "msg-history-patch",
                    "sender_did": "did:example:bob",
                    "receiver_did": "did:example:alice",
                    "content": "history patch text",
                    "content_type": "text/plain",
                    "sent_at": "2026-05-21T03:04:05Z",
                    "server_seq": 42
                }],
                "has_more": false
            }),
        },
        NoopDirectoryTransport,
    );

    runtime
        .history(HistoryRead {
            thread: crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
            query: crate::messages::HistoryQuery {
                limit: crate::ids::PageLimit(5),
                cursor: None,
                inbox_history_options: None,
            },
            resolved_peer_did: None,
            peer_scope: None,
        })
        .unwrap();

    let patch = tokio::time::timeout(std::time::Duration::from_secs(1), session.next_patch())
        .await
        .expect("history projection should emit a conversation patch")
        .expect("conversation patch");
    let item = match patch {
        crate::messages::ConversationStorePatch::Upsert { item, .. } => item,
        crate::messages::ConversationStorePatch::Reset { mut items, .. } => {
            assert_eq!(items.len(), 1);
            items.remove(0)
        }
        other => panic!("unexpected patch: {other:?}"),
    };
    assert_eq!(
        item.last_message.as_ref().unwrap().body.text.as_deref(),
        Some("history patch text")
    );
}

#[test]
fn messages_read_runtime_maps_application_json_content_to_payload_body() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let runtime = MessageReadRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response: json!({
                "messages": [{
                    "id": "msg-history-payload",
                    "sender_did": "did:example:bob",
                    "receiver_did": "did:example:alice",
                    "content": {
                        "schema": "awiki.agent.command.v1",
                        "command": "runtime.agent.create"
                    },
                    "content_type": "application/json",
                    "created_at": "2026-05-21T03:04:05Z"
                }]
            }),
        },
        NoopDirectoryTransport,
    );

    let result = runtime
        .history(HistoryRead {
            thread: crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
            query: crate::messages::HistoryQuery {
                limit: crate::ids::PageLimit(5),
                cursor: None,
                inbox_history_options: None,
            },
            resolved_peer_did: None,
            peer_scope: None,
        })
        .unwrap();

    assert_eq!(
        result.page.items[0].body,
        crate::messages::MessageBodyView::Payload {
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command": "runtime.agent.create"
            })
        }
    );
    assert_eq!(
        result.page.items[0].metadata.content_type.as_deref(),
        Some("application/json")
    );
}

#[test]
fn messages_read_runtime_builds_group_history_rpc() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let group = crate::ids::GroupRef::parse("did:example:group").unwrap();
    let runtime = MessageReadRuntime::new(
        &client,
        ReadyGroupSessionProvider,
        RecordingTransport {
            calls: Rc::clone(&calls),
            response: json!({
                "messages": [{
                    "id": "msg-group-history-1",
                    "sender_did": "did:example:bob",
                    "content": "hello group",
                    "content_type": "text/plain",
                    "group_event_seq": 9
                }],
                "has_more": false
            }),
        },
        NoopDirectoryTransport,
    );

    let result = runtime
        .history(HistoryRead {
            thread: crate::messages::ThreadRef::Group(group.clone()),
            query: crate::messages::HistoryQuery {
                limit: crate::ids::PageLimit(5),
                cursor: Some(crate::ids::Cursor::parse("42").unwrap()),
                inbox_history_options: None,
            },
            resolved_peer_did: None,
            peer_scope: None,
        })
        .unwrap();

    assert_eq!(result.page.items.len(), 1);
    let message = &result.page.items[0];
    assert_eq!(message.id.as_str(), "did:example:group:9");
    assert_eq!(message.group.as_ref(), Some(&group));
    assert_eq!(
        message.thread,
        crate::messages::ThreadRef::Group(group.clone())
    );
    assert_eq!(message.metadata.server_sequence, Some(9));
    assert!(message.metadata.attributes.iter().any(|attribute| {
        attribute.key == "raw_message_id" && attribute.value == "msg-group-history-1"
    }));
    assert!(message
        .metadata
        .attributes
        .iter()
        .any(|attribute| { attribute.key == "group_event_seq" && attribute.value == "9" }));
    let calls = calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].endpoint, MESSAGE_RPC_ENDPOINT);
    assert_eq!(calls[0].method, "group.list_messages");
    assert_eq!(calls[0].params["meta"]["sender_did"], "did:example:alice");
    assert_eq!(
        calls[0].params["meta"]["target"],
        json!({"kind": "group", "did": "did:example:group"})
    );
    assert_eq!(calls[0].params["body"]["group_did"], "did:example:group");
    assert_eq!(calls[0].params["body"]["limit"], 5);
    assert_eq!(calls[0].params["body"]["since_seq"], "42");
}

#[tokio::test]
async fn messages_read_runtime_builds_group_history_rpc_async() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let group = crate::ids::GroupRef::parse("did:example:group").unwrap();
    let runtime = MessageReadRuntime::new(
        &client,
        ReadyGroupSessionProvider,
        RecordingTransport {
            calls: Rc::clone(&calls),
            response: json!({
                "messages": [{
                    "id": "msg-group-history-async-1",
                    "sender_did": "did:example:bob",
                    "content": "hello group async",
                    "content_type": "text/plain",
                    "group_event_seq": 10
                }],
                "has_more": false
            }),
        },
        NoopDirectoryTransport,
    );

    let result = runtime
        .history_async(HistoryRead {
            thread: crate::messages::ThreadRef::Group(group.clone()),
            query: crate::messages::HistoryQuery {
                limit: crate::ids::PageLimit(6),
                cursor: Some(crate::ids::Cursor::parse("43").unwrap()),
                inbox_history_options: None,
            },
            resolved_peer_did: None,
            peer_scope: None,
        })
        .await
        .unwrap();

    assert_eq!(result.page.items.len(), 1);
    let message = &result.page.items[0];
    assert_eq!(message.id.as_str(), "did:example:group:10");
    assert_eq!(message.group.as_ref(), Some(&group));
    assert_eq!(message.metadata.server_sequence, Some(10));
    let calls = calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].endpoint, MESSAGE_RPC_ENDPOINT);
    assert_eq!(calls[0].method, "group.list_messages");
    assert_eq!(calls[0].params["body"]["group_did"], "did:example:group");
    assert_eq!(calls[0].params["body"]["limit"], 6);
    assert_eq!(calls[0].params["body"]["since_seq"], "43");
}

#[test]
fn direct_e2ee_projection_helper_returns_plaintext_and_filters_controls() {
    let messages = vec![
        json!({
            "id": "msg-secure",
            "sender_did": "did:example:bob",
            "receiver_did": "did:example:alice",
            "content_type": "application/anp-direct-cipher+json",
            "server_seq": 2,
            "content": {
                "session_id": "session-1",
                "ratchet_header": {"dh_pub_b64u": "dh", "pn": "0", "n": "1"},
                "ciphertext_b64u": "CIPHER"
            }
        }),
        json!({
            "id": "ack-session-1",
            "sender_did": "did:example:bob",
            "receiver_did": "did:example:alice",
            "content_type": "application/anp-direct-cipher+json",
            "server_seq": 3,
            "content": {
                "session_id": "session-1",
                "ratchet_header": {"dh_pub_b64u": "dh", "pn": "0", "n": "2"},
                "ciphertext_b64u": "ACK-CIPHER"
            }
        }),
    ];

    let (projected, warnings) =
        crate::internal::secure_direct::incoming::project_direct_e2ee_message_values_with_processor(
            messages,
            |notification| {
                let message_id = notification
                    .get("meta")
                    .and_then(Value::as_object)
                    .and_then(|meta| meta.get("message_id"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let plaintext = if message_id.starts_with("ack-") {
                    json!({
                        "application_content_type": "application/json",
                        "payload": {
                            "system_type": crate::internal::secure_direct::control::SECURE_ACK_SYSTEM_TYPE,
                            "session_id": "session-1",
                            "acked_message_id": "msg-secure"
                        }
                    })
                } else {
                    json!({
                        "application_content_type": "text/plain",
                        "text": "decrypted direct text"
                    })
                };
                Ok(serde_json::Map::from_iter([
                    ("state".to_owned(), json!("decrypted")),
                    ("plaintext".to_owned(), plaintext),
                ]))
            },
        );

    assert!(warnings.is_empty());
    assert_eq!(projected.len(), 1);
    let page = page_from_raw(
        &Fixture::new().client(),
        &json!({
            "messages": projected,
            "has_more": false
        }),
        crate::ids::PageLimit(20),
    )
    .unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(
        page.items[0].body,
        crate::messages::MessageBodyView::Text {
            text: "decrypted direct text".to_owned(),
            kind: crate::messages::MessageKind::Text,
        }
    );
    assert!(page.items[0]
        .metadata
        .attributes
        .iter()
        .any(|attribute| attribute.key == "security" && attribute.value == "direct-e2ee"));
    assert!(!serde_json::to_string(&page).unwrap().contains("CIPHER"));
}

#[test]
fn direct_e2ee_projection_helper_redacts_failed_ciphertext() {
    let messages = vec![json!({
        "id": "msg-secure-failed",
        "sender_did": "did:example:bob",
        "receiver_did": "did:example:alice",
        "content_type": "application/anp-direct-cipher+json",
        "server_seq": 1,
        "content": {
            "session_id": "session-1",
            "ratchet_header": {"dh_pub_b64u": "dh", "pn": "0", "n": "1"},
            "ciphertext_b64u": "FAILED-CIPHER"
        }
    })];

    let (projected, warnings) =
        crate::internal::secure_direct::incoming::project_direct_e2ee_message_values_with_processor(
            messages,
            |_notification| {
                Err(crate::ImError::Serialization {
                    detail: "decrypt failed".to_owned(),
                })
            },
        );

    assert_eq!(warnings.len(), 1);
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0]["content"], Value::Null);
    let page = page_from_raw(
        &Fixture::new().client(),
        &json!({
            "messages": projected,
            "has_more": false
        }),
        crate::ids::PageLimit(20),
    )
    .unwrap();
    assert_eq!(page.items.len(), 1);
    assert!(matches!(
        page.items[0].body,
        crate::messages::MessageBodyView::Unsupported { .. }
    ));
    assert!(!serde_json::to_string(&page)
        .unwrap()
        .contains("FAILED-CIPHER"));
}

#[test]
fn public_direct_e2ee_attachment_projection_redacts_object_key_nonce() {
    let messages = vec![json!({
        "id": "msg-secure-attachment",
        "sender_did": "did:example:bob",
        "receiver_did": "did:example:alice",
        "content_type": "application/anp-direct-cipher+json",
        "server_seq": 1,
        "content": {
            "session_id": "session-1",
            "ratchet_header": {"dh_pub_b64u": "dh", "pn": "0", "n": "1"},
            "ciphertext_b64u": "ATTACHMENT-CIPHER"
        }
    })];
    let (mut projected, warnings) =
        crate::internal::secure_direct::incoming::project_direct_e2ee_message_values_with_processor(
            messages,
            |_notification| {
                Ok(serde_json::Map::from_iter([
                    ("state".to_owned(), json!("decrypted")),
                    ("plaintext".to_owned(), direct_e2ee_attachment_plaintext()),
                ]))
            },
        );

    assert!(warnings.is_empty());
    let full = serde_json::to_string(&projected).unwrap();
    assert!(full.contains("object_key_b64u"));
    assert!(full.contains("nonce_b64u"));
    redact_attachment_manifests_for_public_projection(&mut projected);
    let public = serde_json::to_string(&projected).unwrap();
    assert!(!public.contains("object_key_b64u"));
    assert!(!public.contains("nonce_b64u"));
    assert!(!public.contains("OBJECT-KEY-SECRET"));
    assert!(!public.contains("NONCE-SECRET"));
    assert_eq!(
        projected[0]["content"]["attachments"][0]["encryption_info"]["mode"],
        "object-e2ee"
    );
    assert_eq!(
        projected[0]["content"]["attachments"][0]["encryption_info"]["plaintext_size"],
        "11"
    );
}

#[test]
fn inbox_projection_preserves_attachment_manifest_content() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let runtime = MessageReadRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response: json!({
                "messages": [{
                    "id": "msg-attachment-1",
                    "sender_did": "did:example:bob",
                    "receiver_did": "did:example:alice",
                    "content_type": "application/anp-attachment-manifest+json",
                    "content": {
                        "attachments": [{
                            "attachment_id": "att-1",
                            "filename": "report.txt",
                            "mime_type": "text/plain",
                            "size": "12",
                            "digest": {
                                "alg": "sha-256",
                                "value_b64u": "digest"
                            },
                            "access_info": {
                                "object_uri": "https://objects.example/att-1"
                            },
                            "encryption_info": {
                                "mode": "none"
                            }
                        }],
                        "caption": "direct attachment",
                        "primary_attachment_id": "att-1"
                    },
                    "server_seq": 42
                }],
                "has_more": false
            }),
        },
        NoopDirectoryTransport,
    );

    let result = runtime
        .inbox(InboxRead {
            query: crate::messages::InboxQuery {
                scope: crate::messages::InboxScope::DirectOnly,
                limit: crate::ids::PageLimit(20),
                cursor: None,
                unread_only: true,
                inbox_history_options: None,
            },
        })
        .unwrap();

    let message = &result.page.items[0];
    assert_eq!(
        message.metadata.content_type.as_deref(),
        Some("application/anp-attachment-manifest+json")
    );
    assert!(matches!(
        &message.body,
        crate::messages::MessageBodyView::Payload { payload }
            if payload["attachments"][0]["attachment_id"] == "att-1"
    ));
    let raw_content = message
        .metadata
        .attributes
        .iter()
        .find(|attribute| attribute.key == "raw_content")
        .expect("raw content attribute");
    let content: Value = serde_json::from_str(&raw_content.value).unwrap();
    assert_eq!(content["attachments"][0]["attachment_id"], "att-1");
    assert_eq!(content["caption"], "direct attachment");
}

#[test]
fn secure_group_attachment_public_projection_redacts_and_sets_group_profile() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let mut messages = vec![json!({
        "id": "msg-group-attachment",
        "sender_did": "did:example:bob",
        "group_did": "did:example:group:e2ee",
        "content_type": crate::attachments::manifest::attachment_manifest_content_type(),
        "secure": true,
        "decryption_state": "decrypted",
        "content": direct_e2ee_attachment_manifest()
    })];

    let page_full = page_from_raw_with_group(
        &client,
        &json!({
            "messages": messages.clone(),
            "has_more": false
        }),
        crate::ids::PageLimit(20),
        Some(&crate::ids::GroupRef::parse("did:example:group:e2ee").unwrap()),
    )
    .unwrap();
    let full = serde_json::to_string(&page_full).unwrap();
    assert!(full.contains("object_key_b64u"));
    assert!(full.contains("OBJECT-KEY-SECRET"));

    redact_attachment_manifests_for_public_projection(&mut messages);
    let page_public = page_from_raw_with_group(
        &client,
        &json!({
            "messages": messages,
            "has_more": false
        }),
        crate::ids::PageLimit(20),
        Some(&crate::ids::GroupRef::parse("did:example:group:e2ee").unwrap()),
    )
    .unwrap();
    assert_eq!(page_public.items.len(), 1);
    assert!(page_public.items[0]
        .metadata
        .attributes
        .iter()
        .any(|attribute| attribute.key == "security" && attribute.value == "group-e2ee"));
    let public = serde_json::to_string(&page_public).unwrap();
    assert!(!public.contains("object_key_b64u"));
    assert!(!public.contains("nonce_b64u"));
    assert!(!public.contains("OBJECT-KEY-SECRET"));
    assert!(!public.contains("NONCE-SECRET"));
}

#[test]
fn message_body_projects_attachment_manifest_as_payload() {
    let body = message_body(&json!({
        "content_type": crate::attachments::manifest::attachment_manifest_content_type(),
        "content": direct_e2ee_attachment_manifest()
    }));

    assert!(matches!(
        body,
        crate::messages::MessageBodyView::Payload { payload }
            if payload["attachments"][0]["attachment_id"] == "att-secure-1"
    ));
}

#[test]
fn group_attachment_manifest_cache_keeps_internal_full_manifest_while_public_redacts() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let mut messages = vec![json!({
        "id": "did:example:group:e2ee:9",
        "message_id": "did:example:group:e2ee:9",
        "sender_did": "did:example:alice",
        "group_did": "did:example:group:e2ee",
        "content_type": crate::attachments::manifest::attachment_manifest_content_type(),
        "message_security_profile": "group-e2ee",
        "secure": true,
        "decryption_state": "decrypted",
        "content": direct_e2ee_attachment_manifest()
    })];

    cache_attachment_manifests_for_internal_download(&client, &messages);
    redact_attachment_manifests_for_public_projection(&mut messages);

    let public = serde_json::to_string(&messages).unwrap();
    assert!(!public.contains("object_key_b64u"));
    assert!(!public.contains("nonce_b64u"));
    assert!(!public.contains("OBJECT-KEY-SECRET"));
    assert!(!public.contains("NONCE-SECRET"));

    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )
    .unwrap();
    let cached =
        crate::internal::local_state::attachment_manifest_cache::get_attachment_manifest_cache_message(
            &connection,
            client.current_identity().id.as_str(),
            "group",
            "did:example:group:e2ee",
            "did:example:group:e2ee:9",
        )
        .unwrap()
        .unwrap();
    assert_eq!(cached["message_security_profile"], "group-e2ee");
    assert_eq!(
        cached["content"]["attachments"][0]["encryption_info"]["object_key_b64u"],
        "OBJECT-KEY-SECRET"
    );
    assert_eq!(
        cached["content"]["attachments"][0]["encryption_info"]["nonce_b64u"],
        "NONCE-SECRET"
    );
}

#[cfg(feature = "group-e2ee")]
#[test]
fn cached_group_e2ee_plaintext_prevents_redecrypting_consumed_ciphertext() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let group_did = "did:example:group:e2ee";
    let message_id = "did:example:group:e2ee:3";
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )
    .unwrap();
    crate::internal::local_state::messages::upsert_message(
        &connection,
        &crate::internal::local_state::messages::MessageRecord {
            msg_id: message_id.to_owned(),
            owner_identity_id: client.current_identity().id.as_str().to_owned(),
            owner_did: client.did().as_str().to_owned(),
            conversation_id: format!("group:{group_did}"),
            thread_id: format!("group:{group_did}"),
            direction: -1,
            sender_did: "did:example:alice".to_owned(),
            group_id: group_did.to_owned(),
            group_did: group_did.to_owned(),
            content_type: "text/plain".to_owned(),
            content: "cached group plaintext".to_owned(),
            server_seq: Some(3),
            is_e2ee: true,
            metadata: json!({
                "decryption_state": "decrypted",
                "security": "group-e2ee"
            })
            .to_string(),
            ..crate::internal::local_state::messages::MessageRecord::default()
        },
    )
    .unwrap();

    let mut messages = vec![json!({
        "id": "msg-raw-3",
        "message_id": "msg-raw-3",
        "sender_did": "did:example:alice",
        "group_did": group_did,
        "group_event_seq": "3",
        "content_type": crate::internal::group_e2ee::wire::GROUP_E2EE_CIPHER_CONTENT_TYPE,
        "group_cipher_object": {"consumed": true},
        "content": {"group_cipher_object": {"consumed": true}}
    })];

    apply_cached_group_e2ee_messages(&client, &mut messages);

    assert_eq!(messages[0]["content"], "cached group plaintext");
    assert_eq!(messages[0]["content_type"], "text/plain");
    assert_eq!(messages[0]["decryption_state"], "decrypted");
    assert_eq!(messages[0]["decrypted"], true);
    assert!(messages[0].get("group_cipher_object").is_none());
}

#[test]
fn direct_attachment_manifest_cache_uses_peer_did_while_public_projection_redacts() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let mut messages = vec![json!({
        "id": "msg-direct-e2ee-9",
        "message_id": "msg-direct-e2ee-9",
        "sender_did": "did:example:alice",
        "receiver_did": client.did().as_str(),
        "content_type": crate::attachments::manifest::attachment_manifest_content_type(),
        "message_security_profile": "direct-e2ee",
        "secure": true,
        "decryption_state": "decrypted",
        "content": direct_e2ee_attachment_manifest()
    })];

    cache_attachment_manifests_for_internal_download(&client, &messages);
    redact_attachment_manifests_for_public_projection(&mut messages);

    let public = serde_json::to_string(&messages).unwrap();
    assert!(!public.contains("object_key_b64u"));
    assert!(!public.contains("nonce_b64u"));
    assert!(!public.contains("OBJECT-KEY-SECRET"));
    assert!(!public.contains("NONCE-SECRET"));

    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )
    .unwrap();
    let cached = crate::internal::local_state::attachment_manifest_cache::get_attachment_manifest_cache_message(
        &connection,
        client.current_identity().id.as_str(),
        "direct",
        "did:example:alice",
        "msg-direct-e2ee-9",
    )
    .unwrap()
    .unwrap();
    assert_eq!(cached["message_security_profile"], "direct-e2ee");
    assert_eq!(
        cached["content"]["attachments"][0]["encryption_info"]["object_key_b64u"],
        "OBJECT-KEY-SECRET"
    );
}

fn direct_e2ee_attachment_plaintext() -> Value {
    json!({
        "application_content_type": crate::attachments::manifest::attachment_manifest_content_type(),
        "payload": direct_e2ee_attachment_manifest()
    })
}

fn direct_e2ee_attachment_manifest() -> Value {
    json!({
        "attachments": [{
            "attachment_id": "att-secure-1",
            "filename": "secret.txt",
            "mime_type": "text/plain",
            "size": "27",
            "digest": {
                "alg": "sha-256",
                "value_b64u": "ciphertext-digest"
            },
            "access_info": {
                "object_uri": "https://objects.example/secure"
            },
            "encryption_info": {
                "mode": "object-e2ee",
                "object_cipher": "chacha20-poly1305",
                "object_key_b64u": "OBJECT-KEY-SECRET",
                "nonce_b64u": "NONCE-SECRET",
                "plaintext_size": "11"
            }
        }],
        "caption": "secure attachment",
        "primary_attachment_id": "att-secure-1"
    })
}

#[tokio::test]
async fn messages_read_async_projects_direct_init_without_legacy_fallback() {
    let fixture = Fixture::new();
    let exchange =
        crate::internal::secure_direct::async_receive::test_support::incoming_init_exchange();
    fixture.seed_verified_peer_identity(
        "user-secure-sender",
        "secure-sender.awiki.test",
        &[exchange.sender_did.as_str()],
    );
    fixture.write_direct_credentials(&exchange);
    fixture.write_peer_document("bob", &exchange.sender_did, &exchange.sender_document);
    fixture.seed_direct_prekeys(&exchange);
    let client = fixture.client();
    let response = json!({
        "messages": [
            {
                "id": "msg-init-async",
                "sender_did": exchange.sender_did.clone(),
                "receiver_did": exchange.recipient_did.clone(),
                "content_type": "application/anp-direct-init+json",
                "server_seq": 1,
                "content": anp::direct_e2ee::direct_init_body_to_value(&exchange.init_body),
            }
        ],
        "has_more": false
    });
    let runtime = MessageReadRuntime::new(
        &client,
        ReadyAnyReadSessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response,
        },
        NoopDirectoryTransport,
    );

    let result = runtime
        .inbox_async(InboxRead {
            query: crate::messages::InboxQuery {
                scope: crate::messages::InboxScope::All,
                limit: crate::ids::PageLimit(20),
                cursor: None,
                unread_only: false,
                inbox_history_options: None,
            },
        })
        .await
        .unwrap();

    assert_eq!(result.page.items.len(), 1);
    assert_eq!(
        result.page.items[0].body,
        crate::messages::MessageBodyView::Text {
            text: "hello from init".to_owned(),
            kind: crate::messages::MessageKind::Text,
        }
    );
    assert!(result
        .raw
        .get("warnings")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty));
    let saved = client
        .core_inner()
        .local_state_db()
        .await
        .unwrap()
        .get_direct_secure_session("alice-id", exchange.sender_did.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.revision, 0);
    let saved_session =
        crate::internal::secure_direct::sqlite_store::direct_session_from_blob(&saved.state_blob)
            .unwrap();
    assert_eq!(saved_session.recv_n, 1);
}

#[tokio::test]
async fn messages_read_async_replays_pending_direct_cipher_after_init() {
    use anp::direct_e2ee::{ApplicationPlaintext, DirectE2eeSession, DirectEnvelopeMetadata};

    let fixture = Fixture::new();
    let exchange =
        crate::internal::secure_direct::async_receive::test_support::incoming_init_exchange();
    fixture.seed_verified_peer_identity(
        "user-secure-sender",
        "secure-sender.awiki.test",
        &[exchange.sender_did.as_str()],
    );
    fixture.write_direct_credentials(&exchange);
    fixture.write_peer_document("bob", &exchange.sender_did, &exchange.sender_document);
    fixture.seed_direct_prekeys(&exchange);
    let client = fixture.client();
    let mut sender_session = exchange.sender_session.clone();
    sender_session.status = anp::direct_e2ee::models::SESSION_STATUS_ESTABLISHED.to_owned();
    sender_session.recv_chain_key_b64u = sender_session.send_chain_key_b64u.clone();
    sender_session.peer_ratchet_public_key_b64u =
        Some(sender_session.ratchet_public_key_b64u.clone());
    sender_session.send_n = 1;
    let follow_up_metadata = DirectEnvelopeMetadata {
        sender_did: exchange.sender_did.clone(),
        recipient_did: exchange.recipient_did.clone(),
        message_id: "msg-pending-follow-up".to_owned(),
        profile: "anp.direct.e2ee.v1".to_owned(),
        security_profile: "direct-e2ee".to_owned(),
    };
    let (_, follow_up_body) = DirectE2eeSession::encrypt_follow_up(
        &mut sender_session,
        &follow_up_metadata,
        "msg-pending-follow-up",
        &ApplicationPlaintext::new_text("text/plain", "follow-up after init"),
    )
    .unwrap();
    let response = json!({
        "messages": [
            {
                "id": "msg-pending-follow-up",
                "sender_did": exchange.sender_did.clone(),
                "receiver_did": exchange.recipient_did.clone(),
                "content_type": "application/anp-direct-cipher+json",
                "server_seq": 1,
                "content": anp::direct_e2ee::direct_cipher_body_to_value(&follow_up_body),
            },
            {
                "id": "msg-init-async",
                "sender_did": exchange.sender_did.clone(),
                "receiver_did": exchange.recipient_did.clone(),
                "content_type": "application/anp-direct-init+json",
                "server_seq": 2,
                "content": anp::direct_e2ee::direct_init_body_to_value(&exchange.init_body),
            }
        ],
        "has_more": false
    });
    let repeated_response = response.clone();
    let runtime = MessageReadRuntime::new(
        &client,
        ReadyAnyReadSessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response,
        },
        NoopDirectoryTransport,
    );

    let result = runtime
        .inbox_async(InboxRead {
            query: crate::messages::InboxQuery {
                scope: crate::messages::InboxScope::All,
                limit: crate::ids::PageLimit(20),
                cursor: None,
                unread_only: false,
                inbox_history_options: None,
            },
        })
        .await
        .unwrap();

    assert_eq!(result.page.items.len(), 2);
    assert_eq!(
        result
            .page
            .items
            .iter()
            .map(|message| message.id.as_str())
            .collect::<Vec<_>>(),
        vec!["msg-pending-follow-up", "msg-init-async"]
    );
    assert_eq!(
        result.page.items[0].body,
        crate::messages::MessageBodyView::Text {
            text: "follow-up after init".to_owned(),
            kind: crate::messages::MessageKind::Text,
        }
    );
    assert_eq!(
        result.page.items[1].body,
        crate::messages::MessageBodyView::Text {
            text: "hello from init".to_owned(),
            kind: crate::messages::MessageKind::Text,
        }
    );
    assert!(result
        .raw
        .get("warnings")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty));
    let raw_messages = result
        .raw
        .get("messages")
        .and_then(Value::as_array)
        .expect("raw messages");
    assert_eq!(raw_messages[0]["decryption_state"], json!("decrypted"));
    assert_eq!(raw_messages[0]["content"], json!("follow-up after init"));
    let saved = client
        .core_inner()
        .local_state_db()
        .await
        .unwrap()
        .get_direct_secure_session("alice-id", exchange.sender_did.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.revision, 1);
    let saved_session =
        crate::internal::secure_direct::sqlite_store::direct_session_from_blob(&saved.state_blob)
            .unwrap();
    assert_eq!(saved_session.recv_n, 2);
    let cached = client
        .core_inner()
        .local_state_db()
        .await
        .unwrap()
        .list_decrypted_secure_messages("alice-id", vec!["msg-pending-follow-up".to_owned()])
        .await
        .unwrap();
    assert_eq!(
        cached.len(),
        1,
        "expected one committed decrypted projection"
    );
    assert_eq!(cached[0].content_type, "text/plain");
    assert_eq!(cached[0].content, "follow-up after init");

    let repeated_runtime = MessageReadRuntime::new(
        &client,
        ReadyAnyReadSessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response: repeated_response,
        },
        NoopDirectoryTransport,
    );
    let repeated = repeated_runtime
        .inbox_async(InboxRead {
            query: crate::messages::InboxQuery {
                scope: crate::messages::InboxScope::All,
                limit: crate::ids::PageLimit(20),
                cursor: None,
                unread_only: false,
                inbox_history_options: None,
            },
        })
        .await
        .unwrap();
    assert_eq!(
        repeated.page.items[0].body,
        crate::messages::MessageBodyView::Text {
            text: "follow-up after init".to_owned(),
            kind: crate::messages::MessageKind::Text,
        }
    );
    assert_eq!(repeated.raw["messages"][0]["decryption_state"], "decrypted");
    let saved_after_repeat = client
        .core_inner()
        .local_state_db()
        .await
        .unwrap()
        .get_direct_secure_session("alice-id", exchange.sender_did)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved_after_repeat.revision, saved.revision);
    let session_after_repeat =
        crate::internal::secure_direct::sqlite_store::direct_session_from_blob(
            &saved_after_repeat.state_blob,
        )
        .unwrap();
    assert_eq!(session_after_repeat.recv_n, saved_session.recv_n);
}

#[derive(Clone)]
struct ReadySessionProvider;

impl SessionProvider for ReadySessionProvider {
    fn ensure_session(
        &self,
        scope: crate::auth::AuthScope,
    ) -> crate::ImResult<crate::auth::SessionBundle> {
        assert_eq!(scope, crate::auth::AuthScope::Messaging);
        Ok(crate::auth::SessionBundle {
            subject: crate::ids::Did::parse("did:example:alice")?,
            scope,
            expires_at: None,
            refreshed: false,
            bearer_token: None,
        })
    }

    fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
        unreachable!("read runtime should not refresh through the session provider")
    }

    fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
        unreachable!("read runtime should not read status")
    }
}

impl crate::internal::auth::session::AsyncSessionProvider for ReadySessionProvider {
    async fn ensure_session(
        &self,
        scope: crate::auth::AuthScope,
    ) -> crate::ImResult<crate::auth::SessionBundle> {
        SessionProvider::ensure_session(self, scope)
    }

    async fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
        SessionProvider::refresh_session(self)
    }

    async fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
        SessionProvider::status(self)
    }
}

#[derive(Clone)]
struct ReadyGroupSessionProvider;

impl SessionProvider for ReadyGroupSessionProvider {
    fn ensure_session(
        &self,
        scope: crate::auth::AuthScope,
    ) -> crate::ImResult<crate::auth::SessionBundle> {
        assert_eq!(scope, crate::auth::AuthScope::GroupMessaging);
        Ok(crate::auth::SessionBundle {
            subject: crate::ids::Did::parse("did:example:alice")?,
            scope,
            expires_at: None,
            refreshed: false,
            bearer_token: None,
        })
    }

    fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
        unreachable!("read runtime should not refresh through the session provider")
    }

    fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
        unreachable!("read runtime should not read status")
    }
}

impl crate::internal::auth::session::AsyncSessionProvider for ReadyGroupSessionProvider {
    async fn ensure_session(
        &self,
        scope: crate::auth::AuthScope,
    ) -> crate::ImResult<crate::auth::SessionBundle> {
        SessionProvider::ensure_session(self, scope)
    }

    async fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
        SessionProvider::refresh_session(self)
    }

    async fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
        SessionProvider::status(self)
    }
}

#[derive(Clone)]
struct ReadyAnyReadSessionProvider;

impl SessionProvider for ReadyAnyReadSessionProvider {
    fn ensure_session(
        &self,
        scope: crate::auth::AuthScope,
    ) -> crate::ImResult<crate::auth::SessionBundle> {
        assert!(matches!(
            scope,
            crate::auth::AuthScope::Messaging | crate::auth::AuthScope::GroupMessaging
        ));
        Ok(crate::auth::SessionBundle {
            subject: crate::ids::Did::parse("did:example:alice")?,
            scope,
            expires_at: None,
            refreshed: false,
            bearer_token: None,
        })
    }

    fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
        unreachable!("read runtime should not refresh through the session provider")
    }

    fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
        unreachable!("read runtime should not read status")
    }
}

impl crate::internal::auth::session::AsyncSessionProvider for ReadyAnyReadSessionProvider {
    async fn ensure_session(
        &self,
        scope: crate::auth::AuthScope,
    ) -> crate::ImResult<crate::auth::SessionBundle> {
        SessionProvider::ensure_session(self, scope)
    }

    async fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
        SessionProvider::refresh_session(self)
    }

    async fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
        SessionProvider::status(self)
    }
}

struct RecordingTransport {
    calls: Rc<RefCell<Vec<RecordedCall>>>,
    response: Value,
}

impl AuthenticatedRpcTransport for RecordingTransport {
    fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value> {
        self.calls.borrow_mut().push(RecordedCall {
            endpoint: endpoint.to_string(),
            method: method.to_string(),
            params,
        });
        Ok(self.response.clone())
    }
}

impl AsyncAuthenticatedRpcTransport for RecordingTransport {
    async fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value> {
        AuthenticatedRpcTransport::authenticated_rpc(self, endpoint, method, params)
    }
}

struct RecordedCall {
    endpoint: String,
    method: String,
    params: Value,
}

struct NoopDirectoryTransport;

impl RpcTransport for NoopDirectoryTransport {
    fn rpc(&mut self, _endpoint: &str, _method: &str, _params: Value) -> crate::ImResult<Value> {
        Err(crate::ImError::PeerNotFound {
            peer: "noop-directory".to_owned(),
        })
    }
}

impl AsyncRpcTransport for NoopDirectoryTransport {
    async fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        RpcTransport::rpc(self, endpoint, method, params)
    }
}

struct StaticHandleDirectoryTransport;

impl RpcTransport for StaticHandleDirectoryTransport {
    fn rpc(&mut self, _endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        let did = params
            .get("did")
            .and_then(Value::as_str)
            .unwrap_or("did:example:bob-new");
        if method == "lookup" {
            return Ok(json!({
                "handle": "bob",
                "full_handle": "bob.anpclaw.com",
                "did": did,
                "domain": "anpclaw.com",
                "status": "active",
                "user_id": "user-bob"
            }));
        }
        Ok(json!({
            "did": did,
            "service_endpoints": []
        }))
    }
}

impl AsyncRpcTransport for StaticHandleDirectoryTransport {
    async fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        RpcTransport::rpc(self, endpoint, method, params)
    }
}

struct Fixture {
    root: PathBuf,
}

struct DelegatedIdentityFixture {
    user_did: String,
    verification_method: String,
    private_key_path: PathBuf,
    private_key_pem: String,
}

impl DelegatedIdentityFixture {
    fn seal_to_vault_key_ref(&self, client: &crate::core::ImClient) -> String {
        let vault = FileSecretVault::new(
            crate::internal::secure_direct::secret_store::im_core_vault_root_key_from_env()
                .unwrap(),
            FileSecretVaultStore::new(
                crate::internal::delegated_identity::delegated_vault_dir_for_client(client),
            ),
        );
        let secret_ref = vault
            .seal(SealSecretRequest {
                metadata: SecretMetadata {
                    workspace_id: "awiki-im-core".to_owned(),
                    device_id: "local-device".to_owned(),
                    identity_id: Some("alice-id".to_owned()),
                    did: Some(self.user_did.clone()),
                    kind: SecretKind::IdentityDaemonPrivate,
                    key_id: self.verification_method.clone(),
                    key_version: 1,
                    policy: SecretAccessPolicy::no_prompt_local_secret(),
                },
                plaintext: SecretBytes::from_vec(self.private_key_pem.as_bytes().to_vec()),
            })
            .unwrap();
        crate::internal::delegated_identity::encode_vault_key_ref(&secret_ref).unwrap()
    }
}

impl Fixture {
    fn new() -> Self {
        let root = unique_temp_root();
        let identities = root.join("identities");
        fs::create_dir_all(&identities).unwrap();
        fs::write(identities.join("default"), "alice\n").unwrap();
        fs::write(
            identities.join("registry.json"),
            r#"{
              "default_identity": "alice",
              "identities": [{
                "id": "alice-id",
                "did": "did:example:alice",
                "local_alias": "alice",
                "ready_for_auth": true,
                "ready_for_messaging": true,
                "missing": []
              }]
            }"#,
        )
        .unwrap();
        fs::create_dir_all(identities.join("alice")).unwrap();
        Self { root }
    }

    fn client(&self) -> crate::core::ImClient {
        crate::core::ImCore::new(
            crate::ImCoreConfig {
                service_base_url: crate::ServiceEndpoint::parse("https://example.test").unwrap(),
                did_domain: "awiki.test".to_string(),
                user_service_endpoint: None,
                message_service_endpoint: None,
                mail_service_endpoint: None,
                anp_service_endpoint: None,
                anp_service_did: None,
                ca_bundle: None,
                transport_policy: crate::MessageTransportPolicy::HttpOnly,
            },
            crate::ImCorePaths {
                identities: crate::paths::IdentityRegistryPaths {
                    identity_root_dir: self.root.join("identities"),
                    registry_path: self.root.join("identities").join("registry.json"),
                    default_identity_path: Some(self.root.join("identities").join("default")),
                },
                local_state: crate::paths::LocalStatePaths {
                    sqlite_path: self.root.join("local").join("im.sqlite"),
                },
                runtime: crate::paths::RuntimePaths {
                    cache_dir: self.root.join("cache"),
                    temp_dir: self.root.join("tmp"),
                },
            },
        )
        .unwrap()
        .client(crate::identity::IdentitySelector::LocalAlias(
            "alice".to_string(),
        ))
        .unwrap()
    }

    fn identity_dir(&self) -> PathBuf {
        self.root.join("identities").join("alice")
    }

    fn sqlite_path(&self) -> PathBuf {
        self.root.join("local").join("im.sqlite")
    }

    fn seed_verified_peer_identity(
        &self,
        user_id: &str,
        full_handle: &str,
        peer_dids: &[&str],
    ) -> String {
        assert!(
            !peer_dids.is_empty(),
            "verified peer requires at least one DID"
        );
        let authority = full_handle
            .split_once('.')
            .map(|(_, authority)| authority)
            .expect("verified peer Handle must include an authority");
        let mut connection =
            crate::internal::local_state::open_writable(&self.sqlite_path()).unwrap();
        let mut conversation_id = String::new();
        for (index, peer_did) in peer_dids.iter().enumerate() {
            conversation_id = crate::internal::local_state::peer_personas::project_verified_handle(
                &mut connection,
                "alice-id",
                "did:example:alice",
                &crate::directory::HandleLookupResult {
                    handle: crate::ids::Handle::parse(full_handle, "").unwrap(),
                    did: crate::ids::Did::parse(*peer_did).unwrap(),
                    user_id: user_id.to_owned(),
                    domain: Some(authority.to_owned()),
                    status: Some("active".to_owned()),
                    binding_generation: Some((index + 1).to_string()),
                    profile: None,
                    warnings: Vec::new(),
                },
            )
            .unwrap();
        }
        conversation_id
    }

    fn write_direct_credentials(
        &self,
        exchange: &crate::internal::secure_direct::async_receive::test_support::IncomingInitExchange,
    ) {
        let identity_dir = self.identity_dir();
        fs::write(
            identity_dir.join("did.json"),
            exchange.recipient_document.to_string(),
        )
        .unwrap();
        fs::write(identity_dir.join("private.key"), "test-key").unwrap();
        fs::write(
            identity_dir.join("e2ee-agreement-private.pem"),
            exchange.recipient_agreement_private.to_pem(),
        )
        .unwrap();
        fs::write(
            identity_dir.join("auth.json"),
            r#"{"jwt_token":"test-token"}"#,
        )
        .unwrap();
    }

    fn write_peer_document(&self, alias: &str, did: &str, document: &Value) {
        let identities = self.root.join("identities");
        let identity_dir = identities.join(alias);
        fs::create_dir_all(&identity_dir).unwrap();
        fs::write(
            identities.join("registry.json"),
            format!(
                r#"{{
                  "default_identity": "alice",
                  "identities": [
                    {{
                      "id": "alice-id",
                      "did": "did:example:alice",
                      "local_alias": "alice",
                      "ready_for_auth": true,
                      "ready_for_messaging": true,
                      "missing": []
                    }},
                    {{
                      "id": "{alias}-id",
                      "did": "{did}",
                      "local_alias": "{alias}",
                      "ready_for_auth": true,
                      "ready_for_messaging": true,
                      "missing": []
                    }}
                  ]
                }}"#
            ),
        )
        .unwrap();
        fs::write(identity_dir.join("did.json"), document.to_string()).unwrap();
    }

    fn seed_direct_prekeys(
        &self,
        exchange: &crate::internal::secure_direct::async_receive::test_support::IncomingInitExchange,
    ) {
        let connection = crate::internal::local_state::open_writable(&self.sqlite_path()).unwrap();
        let store =
            crate::internal::secure_direct::sqlite_store::SqliteDirectSecureStateStore::new(
                &connection,
            )
            .unwrap();
        store
            .upsert_signed_prekey(
                &crate::internal::secure_direct::sqlite_store::DirectSignedPrekeyRecord {
                    owner_identity_id: "alice-id".to_owned(),
                    owner_did: exchange.recipient_did.clone(),
                    key_id: exchange.recipient_signed_prekey.key_id.clone(),
                    private_key_blob: exchange
                        .recipient_signed_prekey_private
                        .to_pem()
                        .into_bytes(),
                    public_key_blob: exchange
                        .recipient_signed_prekey
                        .public_key_b64u
                        .as_bytes()
                        .to_vec(),
                    status:
                        crate::internal::secure_direct::sqlite_store::DirectPrekeyStatus::Active,
                    metadata_json: serde_json::to_string(&json!({
                        "metadata": exchange.recipient_signed_prekey,
                    }))
                    .unwrap(),
                    created_at: "2026-05-24T00:00:00Z".to_owned(),
                    updated_at: "2026-05-24T00:00:00Z".to_owned(),
                },
            )
            .unwrap();
        store
            .upsert_one_time_prekey(
                &crate::internal::secure_direct::sqlite_store::DirectOneTimePrekeyRecord {
                    owner_identity_id: "alice-id".to_owned(),
                    owner_did: exchange.recipient_did.clone(),
                    key_id: exchange.recipient_one_time_prekey.key_id.clone(),
                    private_key_blob: exchange
                        .recipient_one_time_prekey_private
                        .to_pem()
                        .into_bytes(),
                    public_key_blob: exchange
                        .recipient_one_time_prekey
                        .public_key_b64u
                        .as_bytes()
                        .to_vec(),
                    status:
                        crate::internal::secure_direct::sqlite_store::DirectPrekeyStatus::Available,
                    metadata_json: serde_json::to_string(&json!({
                        "metadata": exchange.recipient_one_time_prekey,
                    }))
                    .unwrap(),
                    created_at: "2026-05-24T00:00:00Z".to_owned(),
                    consumed_at: String::new(),
                },
            )
            .unwrap();
    }

    fn write_delegated_identity(&self) -> DelegatedIdentityFixture {
        let bundle = anp::authentication::create_did_wba_document(
            "awiki.test",
            anp::authentication::DidDocumentOptions {
                path_segments: vec!["user".to_owned()],
                domain: Some("awiki.test".to_owned()),
                challenge: Some("read-delegated-test".to_owned()),
                ..anp::authentication::DidDocumentOptions::default()
            },
        )
        .unwrap();
        let user_did = bundle.did().unwrap().to_owned();
        let delegated_private_key = bundle.private_key_pem("key-1").unwrap().to_owned();
        let verification_method = format!("{user_did}#daemon-key-1");
        let mut did_document = bundle.did_document;
        let mut delegated_method = did_document["verificationMethod"][0].clone();
        delegated_method["id"] = json!(verification_method);
        did_document["verificationMethod"]
            .as_array_mut()
            .unwrap()
            .push(delegated_method);
        did_document["authentication"]
            .as_array_mut()
            .unwrap()
            .push(json!(verification_method));
        let identity_dir = self.identity_dir();
        fs::write(
            identity_dir.join("did.json"),
            serde_json::to_vec_pretty(&did_document).unwrap(),
        )
        .unwrap();
        let private_key_path = identity_dir.join("daemon-key-1.pem");
        fs::write(&private_key_path, &delegated_private_key).unwrap();
        fs::write(
            self.root.join("identities").join("registry.json"),
            json!({
                "default_identity": "alice",
                "identities": [{
                    "id": "alice-id",
                    "did": user_did,
                    "local_alias": "alice",
                    "ready_for_auth": true,
                    "ready_for_messaging": true,
                    "missing": []
                }]
            })
            .to_string(),
        )
        .unwrap();
        DelegatedIdentityFixture {
            user_did,
            verification_method,
            private_key_path,
            private_key_pem: delegated_private_key,
        }
    }
}

fn install_test_im_core_vault_root_key() {
    std::env::set_var(
        crate::internal::secure_direct::secret_store::IM_CORE_VAULT_ROOT_KEY_ENV,
        "Hx8fHx8fHx8fHx8fHx8fHx8fHx8fHx8fHx8fHx8fHx8=",
    );
}

fn unique_temp_root() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let counter = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "im-core-read-runtime-{}-{nanos}-{counter}",
        std::process::id()
    ))
}
