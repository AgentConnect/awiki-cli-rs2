use super::*;
use crate::internal::auth::session::SessionProvider;
use crate::internal::transport::{
    AsyncAuthenticatedRpcTransport, AsyncRpcTransport, AuthenticatedRpcTransport, RpcTransport,
};
use crate::vault::{
    DeviceVaultRootKey, FileSecretVault, FileSecretVaultStore, SealSecretRequest,
    SecretAccessPolicy, SecretBytes, SecretKind, SecretMetadata, SecretVault,
};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

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

#[tokio::test]
async fn all_inbox_persists_direct_and_group_in_their_child_paths() {
    let fixture = VNextCacheFixture::new();
    let client = fixture.client(true);
    let calls = Rc::new(RefCell::new(Vec::new()));
    let runtime = MessageReadRuntime::new(
        &client,
        ReadyAnyReadSessionProvider,
        AllInboxRecordingTransport {
            calls: Rc::clone(&calls),
            direct_response: json!({
                "messages": [{
                    "id": "msg-all-direct",
                    "sender_did": "did:example:bob",
                    "receiver_did": &fixture.did,
                    "content": "direct child",
                    "content_type": "text/plain",
                    "sent_at": "2026-07-21T00:00:00Z"
                }],
                "has_more": false
            }),
            group_list_response: json!({
                "groups": [{"group_did": "did:example:group"}]
            }),
            group_messages_response: json!({
                "messages": [{
                    "id": "msg-all-group",
                    "sender_did": "did:example:bob",
                    "content": "group child",
                    "content_type": "text/plain",
                    "group_event_seq": 31,
                    "sent_at": "2026-07-21T00:00:01Z"
                }],
                "has_more": false
            }),
        },
        StaticHandleDirectoryTransport,
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
        calls
            .borrow()
            .iter()
            .map(|call| call.method.as_str())
            .collect::<Vec<_>>(),
        vec!["inbox.get", "group.list", "group.list_messages"]
    );
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )
    .unwrap();
    let peer_scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
        "user-bob",
        "bob.anpclaw.com",
    )
    .unwrap();
    let direct = crate::internal::local_state::messages::list_direct_messages_for_owner_identity(
        &connection,
        client.current_identity().id.as_str(),
        &[
            crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(
                &peer_scope,
            ),
        ],
        20,
    )
    .unwrap();
    let group = crate::internal::local_state::groups::list_group_messages_for_owner_identity(
        &connection,
        client.current_identity().id.as_str(),
        client.did().as_str(),
        "did:example:group",
        20,
        None,
    )
    .unwrap();
    assert_eq!(direct.len(), 1);
    assert_eq!(direct[0].msg_id, "msg-all-direct");
    assert_eq!(group.len(), 1);
    assert_eq!(group[0]["content"], "group child");
}

#[test]
fn messages_read_runtime_builds_delegated_inbox_auth_and_filters_e2ee() {
    let fixture = Fixture::new();
    let delegated = fixture.write_delegated_identity();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let runtime = MessageReadRuntime::new(
        &client,
        DelegatedProofOnlySessionProvider,
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
    fixture.seed_verified_peer_identity("user-bob", "bob.anpclaw.com", &["did:example:bob"]);
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
        StaticHandleDirectoryTransport,
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
        &conversation.thread,
        crate::messages::ThreadRef::Thread(thread)
            if thread.as_str().starts_with("dm:peer-scope:v1:")
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
        StaticHandleDirectoryTransport,
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
                "messages": [{
                    "id": "did:example:group:77",
                    "message_id": "did:example:group:77",
                    "group_did": "did:example:group",
                    "sender_did": "did:example:alice",
                    "content": "local group send",
                    "content_type": "text/plain",
                    "group_event_seq": "77",
                    "server_seq": 77,
                    "sent_at": "2026-05-21T00:00:03Z"
                }],
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
fn messages_read_runtime_keeps_canonical_group_id_and_remote_raw_id() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let connection = crate::internal::local_state::open_writable(&fixture.sqlite_path()).unwrap();
    crate::internal::local_state::messages::upsert_message(
        &connection,
        &crate::internal::local_state::messages::MessageRecord {
            msg_id: "did:example:group:77".to_owned(),
            owner_identity_id: "alice-id".to_owned(),
            owner_did: "did:example:alice".to_owned(),
            conversation_id: "group:did:example:group".to_owned(),
            thread_id: "group:did:example:group".to_owned(),
            direction: 0,
            sender_did: "did:example:bob".to_owned(),
            group_id: "did:example:group".to_owned(),
            group_did: "did:example:group".to_owned(),
            content_type: "text/plain".to_owned(),
            content: "same accepted message".to_owned(),
            sent_at: "2026-05-21T00:00:03Z".to_owned(),
            stored_at: "2026-05-21T00:00:03Z".to_owned(),
            server_seq: Some(77),
            metadata: json!({
                "content_type": "text/plain",
                "server_sequence": 77,
                "operation_id": "op-77"
            })
            .to_string(),
            ..crate::internal::local_state::messages::MessageRecord::default()
        },
    )
    .unwrap();
    let remote = crate::groups::GroupReadResult::from_raw_response(
        json!({
            "messages": [{
                "id": "msg-original-77",
                "message_id": "msg-original-77",
                "group_did": "did:example:group",
                "sender_did": "did:example:bob",
                "content": "same accepted message",
                "content_type": "text/plain",
                "operation_id": "op-77",
                "group_event_seq": "77",
                "server_seq": 77,
                "sent_at": "2026-05-21T00:00:03Z"
            }],
            "has_more": false
        }),
        Vec::new(),
    );
    let mut page = remote.messages;
    let rows = crate::internal::local_state::groups::list_group_messages_for_owner_identity(
        &connection,
        client.current_identity().id.as_str(),
        client.did().as_str(),
        "did:example:group",
        20,
        None,
    )
    .unwrap();

    merge_local_message_values_into_page(&mut page, rows, crate::ids::PageLimit(20));

    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id.as_str(), "did:example:group:77");
    assert_eq!(page.items[0].metadata.server_sequence, Some(77));
    assert!(page.items[0].metadata.attributes.iter().any(|attribute| {
        attribute.key == "raw_message_id" && attribute.value == "msg-original-77"
    }));
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
fn messages_read_runtime_local_history_hides_direct_e2ee_wire_messages() {
    let message_record = |msg_id: &str, content_type: &str, content: &str| {
        crate::internal::local_state::messages::MessageRecord {
            msg_id: msg_id.to_owned(),
            owner_identity_id: "alice-id".to_owned(),
            owner_did: "did:example:alice".to_owned(),
            conversation_id: "dm:did:example:bob".to_owned(),
            thread_id: "dm:did:example:bob".to_owned(),
            direction: 0,
            sender_did: "did:example:bob".to_owned(),
            receiver_did: "did:example:alice".to_owned(),
            content_type: content_type.to_owned(),
            content: content.to_owned(),
            is_e2ee: true,
            ..crate::internal::local_state::messages::MessageRecord::default()
        }
    };
    let page = local_history_records_to_page(
        crate::internal::local_state::messages::ThreadLocalHistoryRecords {
            records: vec![
                message_record("msg-plaintext", "text/plain", "decrypted message"),
                message_record(
                    "msg-init-wire",
                    "application/anp-direct-init+json",
                    r#"{"ciphertext_b64u":"init-ciphertext"}"#,
                ),
                message_record(
                    "msg-cipher-wire",
                    "application/anp-direct-cipher+json",
                    r#"{"ciphertext_b64u":"session-ciphertext"}"#,
                ),
            ],
            next_cursor: None,
            has_more: false,
        },
    )
    .unwrap();

    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id.as_str(), "msg-plaintext");
    assert_eq!(
        page.items[0].metadata.content_type.as_deref(),
        Some("text/plain")
    );
    assert!(matches!(
        &page.items[0].body,
        crate::messages::MessageBodyView::Text { text, .. } if text == "decrypted message"
    ));
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
        StaticHandleDirectoryTransport,
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
    assert!(message.metadata.attributes.iter().any(|attribute| {
        attribute.key == "raw_message_id" && attribute.value == "msg-group-history-async-1"
    }));
    assert!(message
        .metadata
        .attributes
        .iter()
        .any(|attribute| { attribute.key == "group_event_seq" && attribute.value == "10" }));
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
        "message_id": "logical-group-message-9",
        "raw_message_id": "logical-group-message-9",
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
    assert_eq!(cached["raw_message_id"], "logical-group-message-9");
    assert_eq!(
        cached["content"]["attachments"][0]["encryption_info"]["object_key_b64u"],
        "OBJECT-KEY-SECRET"
    );
    assert_eq!(
        cached["content"]["attachments"][0]["encryption_info"]["nonce_b64u"],
        "NONCE-SECRET"
    );

    let mut failed_replay = vec![json!({
        "id": "did:example:group:e2ee:9",
        "sender_did": "did:example:alice",
        "group_did": "did:example:group:e2ee",
        "content_type": "application/x-awiki-group-e2ee-redacted",
        "decryption_state": "failed",
        "content": null
    })];
    apply_cached_group_attachment_manifests(&client, &mut failed_replay);
    assert_eq!(
        failed_replay[0]["content_type"],
        crate::attachments::manifest::attachment_manifest_content_type()
    );
    assert_eq!(failed_replay[0]["type"], "attachment_manifest");
    assert_eq!(failed_replay[0]["decryption_state"], "decrypted");
    assert_eq!(
        failed_replay[0]["content"]["attachments"][0]["encryption_info"]["object_key_b64u"],
        "OBJECT-KEY-SECRET"
    );

    redact_attachment_manifests_for_public_projection(&mut failed_replay);
    assert_eq!(
        failed_replay[0]["content"]["attachments"][0]["encryption_info"]["mode"],
        "object-e2ee"
    );
    let public = serde_json::to_string(&failed_replay).unwrap();
    assert!(!public.contains("OBJECT-KEY-SECRET"));
    assert!(!public.contains("NONCE-SECRET"));
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
        "id": "logical-direct-e2ee-9",
        "message_id": "msg-direct-e2ee-9",
        "raw_message_id": "wire-direct-e2ee-9",
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
        "logical-direct-e2ee-9",
    )
    .unwrap()
    .unwrap();
    assert_eq!(cached["id"], "logical-direct-e2ee-9");
    assert_eq!(cached["message_id"], "logical-direct-e2ee-9");
    assert_eq!(cached["raw_message_id"], "wire-direct-e2ee-9");
    assert!(cached.get("wire_message_id").is_none());
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

#[test]
#[cfg(feature = "group-e2ee")]
fn v2_product_profiles_are_hidden_gate_independently_from_blocking_and_delegated_reads() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let p5 = json!({
        "id": "wire-p5-business",
        "sender_did": "did:example:bob",
        "receiver_did": "did:example:alice",
        "meta": {
            "profile": anp::direct_e2ee::DIRECT_E2EE_PROFILE_V2,
            "content_type": anp::direct_e2ee::CONTENT_TYPE_DIRECT_CIPHER_V2,
            "operation_id": "ordinary-product-operation"
        },
        "body": {"ciphertext_b64u": "SECRET-P5-CIPHERTEXT"}
    });
    let p6 = json!({
        "id": "wire-p6-business",
        "meta": {
            "profile": anp::group_e2ee::GROUP_E2EE_PROFILE_V2,
            "content_type": anp::group_e2ee::GROUP_CIPHER_CONTENT_TYPE_V2
        },
        "body": {"group_cipher_object": {"private_message_b64u": "SECRET-P6-CIPHERTEXT"}}
    });
    let p6_notice = json!({
        "method": anp::group_e2ee::METHOD_GROUP_NOTICE_V2,
        "params": {
            "meta": {
                "profile": anp::group_e2ee::GROUP_E2EE_PROFILE_V2,
                "security_profile": anp::group_e2ee::GROUP_E2EE_TRANSPORT_PROFILE_V2
            },
            "body": {
                "notice_type": "welcome-delivery",
                "welcome_b64u": "SECRET-P6-WELCOME"
            }
        }
    });

    let mut direct = json!({"messages": [p5.clone()]});
    project_secure_direct_messages(&client, &mut direct, &mut NoopDirectoryTransport);
    assert_eq!(direct["messages"], json!([]));

    let mut group = json!({"messages": [p6.clone(), p6_notice.clone()]});
    project_group_e2ee_messages(&client, &mut group);
    assert_eq!(group["messages"], json!([]));

    let mut delegated = json!({"messages": [p5, p6, p6_notice]});
    filter_delegated_e2ee_messages(&mut delegated);
    assert_eq!(delegated["messages"], json!([]));
    let public = serde_json::to_string(&(direct, group, delegated)).unwrap();
    assert!(!public.contains("SECRET-P5-CIPHERTEXT"));
    assert!(!public.contains("SECRET-P6-CIPHERTEXT"));
    assert!(!public.contains("SECRET-P6-WELCOME"));
}

#[test]
fn direct_inbox_prepass_hides_group_notices_before_message_projection() {
    let client = Fixture::new().client();
    let mut raw = json!({
        "messages": [
            {
                "method": anp::group_e2ee::METHOD_GROUP_NOTICE_V2,
                "params": {
                    "meta": {
                        "profile": anp::group_e2ee::GROUP_E2EE_PROFILE_V2,
                        "security_profile": anp::group_e2ee::GROUP_E2EE_TRANSPORT_PROFILE_V2
                    },
                    "body": {
                        "notice_type": "welcome-delivery",
                        "welcome_b64u": "BLOCKING-INBOX-CONTROL-SECRET"
                    }
                }
            },
            {
                "method": anp::group_e2ee::METHOD_GROUP_NOTICE_V2,
                "params": {
                    "meta": {"profile": "anp.group.e2ee.unknown"},
                    "body": {"private_extension": "MALFORMED-CONTROL-SECRET"}
                }
            },
            {
                "id": "ordinary-direct-message",
                "content_type": "text/plain",
                "content": "visible"
            }
        ]
    });

    consume_group_e2ee_control_messages(&client, &mut raw);

    assert_eq!(raw["messages"].as_array().unwrap().len(), 1);
    assert_eq!(raw["messages"][0]["id"], "ordinary-direct-message");
    let public = serde_json::to_string(&raw).unwrap();
    assert!(!public.contains("BLOCKING-INBOX-CONTROL-SECRET"));
    assert!(!public.contains("MALFORMED-CONTROL-SECRET"));
}

#[tokio::test]
async fn async_direct_inbox_prepass_hides_group_notices_before_message_projection() {
    let client = Fixture::new().client();
    let mut raw = json!({
        "messages": [
            {
                "method": anp::group_e2ee::METHOD_GROUP_NOTICE_V2,
                "params": {
                    "meta": {
                        "profile": anp::group_e2ee::GROUP_E2EE_PROFILE_V2,
                        "security_profile": anp::group_e2ee::GROUP_E2EE_TRANSPORT_PROFILE_V2
                    },
                    "body": {
                        "notice_type": "commit-delivery",
                        "commit_b64u": "ASYNC-INBOX-CONTROL-SECRET"
                    }
                }
            },
            {
                "id": "ordinary-direct-message",
                "content_type": "text/plain",
                "content": "visible"
            }
        ]
    });

    consume_group_e2ee_control_messages_async(&client, &mut raw).await;

    assert_eq!(raw["messages"].as_array().unwrap().len(), 1);
    assert_eq!(raw["messages"][0]["id"], "ordinary-direct-message");
    assert!(!serde_json::to_string(&raw)
        .unwrap()
        .contains("ASYNC-INBOX-CONTROL-SECRET"));
}

#[test]
#[cfg(feature = "group-e2ee")]
fn v2_profile_recognition_accepts_json_rpc_notification_shape() {
    let p5 = json!({
        "method": "direct.incoming",
        "params": {"meta": {"profile": anp::direct_e2ee::DIRECT_E2EE_PROFILE_V2}}
    });
    let p6 = json!({
        "method": "group.incoming",
        "params": {"meta": {"profile": anp::group_e2ee::GROUP_E2EE_PROFILE_V2}}
    });
    let p6_notice = json!({
        "method": anp::group_e2ee::METHOD_GROUP_NOTICE_V2,
        "params": {"meta": {
            "profile": anp::group_e2ee::GROUP_E2EE_PROFILE_V2,
            "security_profile": anp::group_e2ee::GROUP_E2EE_TRANSPORT_PROFILE_V2
        }}
    });

    assert!(is_p5_v2_projection_candidate(&p5));
    assert!(is_p6_v2_projection_candidate(&p6));
    assert!(is_p6_v2_projection_candidate(&p6_notice));
    assert!(crate::internal::group_e2ee::v2_notice::is_v2_notice_candidate(&p6_notice));
}

#[test]
#[cfg(feature = "group-e2ee")]
fn p6_v2_incoming_parser_accepts_json_rpc_notification_envelope() {
    let mut wire = p6_v2_incoming_wire();
    wire.as_object_mut()
        .unwrap()
        .insert("jsonrpc".to_owned(), json!("2.0"));

    let (meta, body, _) = parse_p6_v2_incoming_notification(&wire).unwrap();

    assert_eq!(meta.message_id, "message-1");
    assert_eq!(body.group_did, "did:example:group");
}

#[test]
#[cfg(feature = "group-e2ee")]
fn p6_v2_incoming_parser_rejects_invalid_json_rpc_version() {
    for invalid in [json!("1.0"), json!(2.0), Value::Null] {
        let mut wire = p6_v2_incoming_wire();
        wire.as_object_mut()
            .unwrap()
            .insert("jsonrpc".to_owned(), invalid);

        assert_eq!(
            parse_p6_v2_incoming_notification(&wire),
            Err(crate::ImError::PermissionDenied)
        );
    }
}

#[test]
#[cfg(feature = "group-e2ee")]
fn p6_v2_incoming_parser_rejects_unknown_top_level_field() {
    let mut wire = p6_v2_incoming_wire();
    let object = wire.as_object_mut().unwrap();
    object.insert("jsonrpc".to_owned(), json!("2.0"));
    object.insert("unexpected".to_owned(), json!(true));

    assert_eq!(
        parse_p6_v2_incoming_notification(&wire),
        Err(crate::ImError::PermissionDenied)
    );
}

#[test]
#[cfg(feature = "group-e2ee")]
fn p6_v2_incoming_parser_accepts_flat_fallback() {
    let wire = p6_v2_incoming_wire();
    let flat = json!({
        "meta": wire.pointer("/params/meta").unwrap(),
        "body": wire.pointer("/params/body").unwrap(),
        "auth": wire.pointer("/params/auth").unwrap(),
    });

    let (meta, body, _) = parse_p6_v2_incoming_notification(&flat).unwrap();

    assert_eq!(meta.message_id, "message-1");
    assert_eq!(body.group_did, "did:example:group");
}

#[test]
#[cfg(feature = "group-e2ee")]
fn p6_v2_incoming_recipient_device_requires_exact_owner_did() {
    assert_eq!(
        p6_recipient_device_id("did:example:alice", "did:example:alice", "alice-device"),
        Ok("alice-device".to_owned())
    );
    assert_eq!(
        p6_recipient_device_id("did:example:alice", "did:example:other", "alice-device"),
        Err(crate::ImError::PermissionDenied)
    );
    assert_eq!(
        p6_recipient_device_id("did:example:alice", "did:example:alice", ""),
        Err(crate::ImError::PermissionDenied)
    );
}

#[test]
#[cfg(feature = "group-e2ee")]
fn p6_v2_incoming_uses_canonical_group_timeline_id() {
    assert_eq!(
        p6_group_message_id("did:example:group", "17"),
        Ok("did:example:group:17".to_owned())
    );
    assert_eq!(
        p6_group_message_id("did:example:group", ""),
        Err(crate::ImError::PermissionDenied)
    );
}

#[test]
#[cfg(feature = "group-e2ee")]
fn p6_v2_projection_diagnostics_are_stable_and_redacted() {
    let error = crate::ImError::Internal {
        message: "group MLS operation failed (group.e2ee.private_message_invalid): ciphertext-and-proof-must-not-be-exposed".to_owned(),
    };

    assert_eq!(
        p6_projection_error_code(&error),
        "mls_private_message_invalid"
    );
    assert!(!p6_projection_error_code(&error).contains("ciphertext"));
}

#[test]
#[cfg(feature = "group-e2ee")]
fn p6_v2_service_projection_state_cannot_authorize_cache_fast_path() {
    let mut message = p6_v2_incoming_wire();
    message["secure"] = json!(true);
    message["decrypted"] = json!(true);
    message["decryption_state"] = json!("decrypted");
    message["content"] = json!("host-forged plaintext");
    message["content_type"] = json!("text/plain");
    message["type"] = json!("text");
    message["direction"] = json!(1);
    message["sender_device_id"] = json!("host-forged-device");

    clear_untrusted_p6_projection_state(std::slice::from_mut(&mut message));

    for field in [
        "secure",
        "decrypted",
        "decryption_state",
        "content",
        "content_type",
        "type",
        "direction",
        "sender_device_id",
    ] {
        assert!(message.get(field).is_none(), "untrusted {field} survived");
    }
}

#[test]
#[cfg(all(feature = "sqlite", feature = "group-e2ee"))]
fn profileless_remote_message_cannot_self_assert_secure_projection() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let mut raw = json!({
        "messages": [{
            "id": "host-plain-1",
            "group_did": "did:example:plain-group",
            "sender_did": "did:example:mallory",
            "content": "host plaintext",
            "content_type": "text/plain",
            "secure": true,
            "decrypted": true,
            "decryption_state": "decrypted"
        }]
    });

    project_group_e2ee_messages_for_group(&client, &mut raw, "did:example:plain-group");

    let message = &raw["messages"][0];
    assert_eq!(message["content"], "host plaintext");
    for field in ["secure", "decrypted", "decryption_state"] {
        assert!(message.get(field).is_none());
    }
    let projected = message_from_value(&client, message, None)
        .unwrap()
        .expect("plain message remains displayable without a security claim");
    assert!(projected
        .metadata
        .attributes
        .iter()
        .all(|attribute| attribute.key != "security" && attribute.key != "decryption_state"));
}

#[tokio::test]
#[cfg(all(feature = "sqlite", feature = "group-e2ee"))]
async fn locally_required_secure_group_rejects_profileless_application_but_keeps_system_event() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let group_did = "did:example:required-secure-group";
    let connection = crate::internal::local_state::open_writable(&fixture.sqlite_path()).unwrap();
    crate::internal::local_state::groups::upsert_group(
        &connection,
        crate::internal::local_state::groups::GroupRecord {
            owner_identity_id: client.current_identity().id.as_str().to_owned(),
            owner_did: client.did().as_str().to_owned(),
            group_id: group_did.to_owned(),
            group_did: group_did.to_owned(),
            metadata: json!({
                "required_security_profile": "group-e2ee",
                "group_policy": {"message_security_profile": "group-e2ee"}
            })
            .to_string(),
            ..crate::internal::local_state::groups::GroupRecord::default()
        },
    )
    .unwrap();
    drop(connection);
    let original = json!({
        "messages": [
            {
                "id": "host-forged-application",
                "group_did": group_did,
                "sender_did": "did:example:mallory",
                "content": "forged secure-group plaintext",
                "content_type": "text/plain",
                "secure": true,
                "decryption_state": "decrypted"
            },
            {
                "id": "p4-system-event",
                "group_did": group_did,
                "sender_did": "did:example:group-host",
                "content": {"member_did": "did:example:bob"},
                "content_type": "application/json",
                "type": "system",
                "system_event": {
                    "event_kind": "group-member-changed",
                    "subject_method": "group.member.update"
                }
            }
        ]
    });
    let mut raw = original.clone();

    project_group_e2ee_messages_for_group(&client, &mut raw, group_did);

    let messages = raw["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["id"], "p4-system-event");
    assert!(!serde_json::to_string(&raw)
        .unwrap()
        .contains("forged secure-group plaintext"));

    let mut async_raw = original;
    project_group_e2ee_messages_for_group_async(&client, &mut async_raw, group_did).await;
    let async_messages = async_raw["messages"].as_array().unwrap();
    assert_eq!(async_messages.len(), 1);
    assert_eq!(async_messages[0]["id"], "p4-system-event");
    assert!(!serde_json::to_string(&async_raw)
        .unwrap()
        .contains("forged secure-group plaintext"));
}

#[test]
#[cfg(all(feature = "sqlite", feature = "group-e2ee"))]
fn p6_v2_cache_fast_path_requires_a_bound_local_record() {
    let mut message = p6_v2_incoming_wire();
    clear_untrusted_p6_projection_state(std::slice::from_mut(&mut message));
    let record = crate::internal::local_state::messages::MessageRecord {
        msg_id: "did:example:group:1".to_owned(),
        owner_identity_id: "bob-id".to_owned(),
        owner_did: "did:example:bob".to_owned(),
        sender_did: "did:example:alice".to_owned(),
        group_id: "did:example:group".to_owned(),
        group_did: "did:example:group".to_owned(),
        content_type: "text/plain".to_owned(),
        content: "locally authenticated plaintext".to_owned(),
        direction: 0,
        server_seq: Some(1),
        sent_at: "2026-08-19T00:00:00Z".to_owned(),
        is_e2ee: true,
        ..crate::internal::local_state::messages::MessageRecord::default()
    };

    let applied = apply_cached_group_e2ee_records_for_owner(
        std::slice::from_mut(&mut message),
        vec![record.clone()],
        "did:example:bob",
    );
    assert_eq!(applied, HashSet::from([0]));
    assert_eq!(message["content"], "locally authenticated plaintext");
    assert_eq!(message["sender_did"], "did:example:alice");
    assert_eq!(message["direction"], 0);
    assert_eq!(message["group_event_seq"], 1);
    assert_eq!(message["sent_at"], "2026-08-19T00:00:00Z");
    assert!(message.get("sender_device_id").is_none());

    let mut forged_binding = p6_v2_incoming_wire();
    forged_binding["params"]["meta"]["sender_did"] = json!("did:example:mallory");
    clear_untrusted_p6_projection_state(std::slice::from_mut(&mut forged_binding));
    assert!(apply_cached_group_e2ee_records_for_owner(
        std::slice::from_mut(&mut forged_binding),
        vec![record],
        "did:example:bob",
    )
    .is_empty());
    assert!(forged_binding.get("content").is_none());
}

#[test]
#[cfg(feature = "group-e2ee")]
fn p6_v2_cached_projection_removes_all_live_wire_fields() {
    let mut message = p6_v2_incoming_wire();
    message["content"] = json!("locally authenticated plaintext");

    strip_p6_v2_wire_fields(&mut message);

    assert_eq!(message["content"], "locally authenticated plaintext");
    for field in [
        "jsonrpc",
        "method",
        "params",
        "meta",
        "body",
        "auth",
        "group_cipher_object",
    ] {
        assert!(message.get(field).is_none());
    }
}

#[test]
#[cfg(feature = "group-e2ee")]
fn p6_v2_plaintext_cache_retries_only_sqlite_lock_errors() {
    assert!(is_transient_sqlite_lock(
        &crate::ImError::LocalStateUnavailable {
            detail: "database is locked".to_owned(),
        }
    ));
    assert!(is_transient_sqlite_lock(
        &crate::ImError::LocalStateUnavailable {
            detail: "database table is locked".to_owned(),
        }
    ));
    assert!(!is_transient_sqlite_lock(
        &crate::ImError::LocalStateUnavailable {
            detail: "disk I/O error".to_owned(),
        }
    ));
    assert!(!is_transient_sqlite_lock(&crate::ImError::PermissionDenied));
}

#[cfg(feature = "group-e2ee")]
fn p6_v2_incoming_wire() -> Value {
    json!({
        "method": anp::group_e2ee::METHOD_GROUP_INCOMING_V2,
        "params": {
            "meta": {
                "anp_version": "2.0",
                "profile": anp::group_e2ee::GROUP_E2EE_PROFILE_V2,
                "security_profile": anp::group_e2ee::GROUP_E2EE_SECURITY_PROFILE_V2,
                "sender_did": "did:example:alice",
                "sender_device_id": "alice-device",
                "target": {"kind": "agent", "did": "did:example:bob"},
                "recipient_device_id": "bob-device",
                "operation_id": "operation-1",
                "message_id": "message-1",
                "content_type": anp::group_e2ee::GROUP_CIPHER_CONTENT_TYPE_V2
            },
            "body": {
                "group_did": "did:example:group",
                "group_state_version": "1",
                "group_event_seq": "1",
                "accepted_at": "2026-07-22T00:00:00Z",
                "group_receipt": {},
                "group_cipher_object": {
                    "crypto_group_id_b64u": "AA",
                    "epoch": "1",
                    "private_message_b64u": "AA",
                    "group_state_ref": {
                        "group_did": "did:example:group",
                        "group_state_version": "1"
                    }
                }
            },
            "auth": {
                "scheme": anp::group_e2ee::RFC9421_ORIGIN_PROOF_SCHEME_V2,
                "origin_proof": {
                    "contentDigest": "digest",
                    "signatureInput": "signature-input",
                    "signature": "signature"
                },
                "origin_context": {
                    "extra_meta": {"anp_version": "2.0"}
                }
            }
        }
    })
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
    assert_eq!(cached.len(), 1, "expected one cached decrypted projection");
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

#[tokio::test]
async fn direct_inbox_projects_verified_handle_before_persisting_message() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let runtime = MessageReadRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response: json!({
                "messages": [{
                    "id": "msg-authoritative-peer",
                    "sender_did": "did:example:bob-new",
                    "receiver_did": "did:example:alice",
                    "content": "verified peer message",
                    "content_type": "text/plain",
                    "sent_at": "2026-07-21T00:00:00Z",
                    "server_seq": 21
                }],
                "has_more": false
            }),
        },
        StaticHandleDirectoryTransport,
    );

    let result = runtime
        .inbox_async(InboxRead {
            query: crate::messages::InboxQuery {
                scope: crate::messages::InboxScope::DirectOnly,
                limit: crate::ids::PageLimit(20),
                cursor: None,
                unread_only: false,
                inbox_history_options: None,
            },
        })
        .await
        .unwrap();

    assert_eq!(result.page.items.len(), 1);
    let scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
        "user-bob",
        "bob.anpclaw.com",
    )
    .unwrap();
    let records = client
        .core_inner()
        .local_state_db()
        .await
        .unwrap()
        .list_direct_messages(
            "alice-id",
            vec![
                crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(
                    &scope,
                ),
            ],
            20,
        )
        .await
        .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].msg_id, "msg-authoritative-peer");
    let connection = crate::internal::local_state::open_writable(&fixture.sqlite_path()).unwrap();
    assert_eq!(
        crate::internal::local_state::inbound_resolution_backlog::pending_count(
            &connection,
            "alice-id"
        )
        .unwrap(),
        0
    );
    assert!(crate::internal::local_state::peer_personas::resolve_by_did(
        &connection,
        "alice-id",
        "did:example:bob-new"
    )
    .unwrap()
    .is_some());
}

#[tokio::test]
async fn direct_page_resolves_expected_peer_scope_once_for_all_messages() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut directory = CountingHandleDirectoryTransport {
        calls: Rc::clone(&calls),
    };
    let mut raw = json!({
        "messages": [
            {
                "id": "msg-page-1",
                "sender_did": "did:example:bob-new",
                "receiver_did": "did:example:alice",
                "content": "first",
                "content_type": "text/plain"
            },
            {
                "id": "msg-page-2",
                "sender_did": "did:example:alice",
                "receiver_did": "did:example:bob-new",
                "content": "second",
                "content_type": "text/plain"
            }
        ]
    });

    annotate_direct_peer_scopes_async(
        &client,
        &mut raw,
        &mut directory,
        None,
        Some("did:example:bob-new"),
        None,
    )
    .await;

    assert_eq!(calls.borrow().as_slice(), ["lookup"]);
    for message in raw["messages"].as_array().unwrap() {
        assert_eq!(message["peer_user_id"], "user-bob");
        assert_eq!(message["peer_full_handle"], "bob.anpclaw.com");
        assert_eq!(message["peer_current_did"], "did:example:bob-new");
    }
}

#[test]
fn plain_direct_history_keeps_peer_wire_identity_after_canonical_thread_projection() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let raw = json!({
        "messages": [{
            "id": "msg-plain-own-sync-history",
            "sender_did": "did:example:alice",
            "receiver_did": "did:example:bob-new",
            "content": "plain sender history",
            "content_type": "text/plain",
            "server_seq": 37,
            "peer_user_id": "user-bob",
            "peer_full_handle": "bob.anpclaw.com",
            "peer_current_did": "did:example:bob-new",
            "resolved_target_did": "did:example:bob-new"
        }],
        "has_more": false
    });
    let page = page_from_raw(&client, &raw, crate::ids::PageLimit(20)).unwrap();
    assert!(matches!(
        &page.items[0].thread,
        crate::messages::ThreadRef::Thread(_)
    ));

    let records = remote_projection_records(
        &client,
        &page.items,
        &DirectP5ProjectionProvenance::default(),
    )
    .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].wire_thread_kind, "direct");
    assert_eq!(records[0].wire_thread_ref, "did:example:bob-new");

    let connection = crate::internal::local_state::open_writable(&fixture.sqlite_path()).unwrap();
    let mut local_projection = records[0].clone();
    local_projection.content = "local optimistic send".to_owned();
    local_projection.server_seq = None;
    crate::internal::local_state::messages::upsert_message(&connection, &local_projection).unwrap();
    crate::internal::local_state::messages::upsert_message(&connection, &records[0]).unwrap();
}

#[tokio::test]
async fn p5_backlog_retries_by_authenticated_wire_and_converges_after_handle_resolution() {
    let fixture = VNextCacheFixture::new();
    let client = fixture.client(true);
    let owner_identity_id = client.current_identity().id.as_str().to_owned();
    let wire = ordinary_p5_cache_message(
        "wire-p5-backlog",
        "did:example:bob-new",
        "device-bob",
        &fixture.did,
        &fixture.device_id,
    );
    let mut first_projection = wire.clone();
    apply_p5_v2_product_outcome(
        &mut first_projection,
        crate::internal::secure_direct::v2_product::V2InboundProductOutcome::Business(
            crate::internal::secure_direct::v2_product::V2InboundBusinessProjection {
                logical_message_id: "msg-logical-backlog".to_owned(),
                conversation_id: None,
                sender_did: "did:example:bob-new".to_owned(),
                sender_device_id: "device-bob".to_owned(),
                recipient_did: fixture.did.clone(),
                wire_message_id: "wire-p5-backlog".to_owned(),
                body: crate::internal::secure_direct::v2_product::V2InboundBusinessBody::Text {
                    text: "backlog plaintext".to_owned(),
                    markdown: false,
                },
                session_reply_pending: false,
            },
        ),
    );
    let mut first_raw = json!({"messages": [first_projection], "has_more": false});
    annotate_direct_peer_scopes_async(
        &client,
        &mut first_raw,
        &mut NoopDirectoryTransport,
        None,
        None,
        None,
    )
    .await;
    let first_page = page_from_raw(&client, &first_raw, crate::ids::PageLimit(20)).unwrap();
    assert_eq!(first_page.items.len(), 1);
    let mut p5_provenance = DirectP5ProjectionProvenance::default();
    p5_provenance.record(
        "msg-logical-backlog",
        p5_cache_binding_from_message(&first_raw["messages"][0])
            .unwrap()
            .unwrap(),
    );
    persist_projection_best_effort_async(&client, &first_page.items, &p5_provenance).await;

    let sqlite_path = VNextCacheFixture::paths(&fixture.root)
        .local_state
        .sqlite_path;
    let connection = crate::internal::local_state::open_writable(&sqlite_path).unwrap();
    assert_eq!(
        crate::internal::local_state::inbound_resolution_backlog::pending_count(
            &connection,
            &owner_identity_id
        )
        .unwrap(),
        1
    );
    let cached =
        crate::internal::local_state::messages::list_decrypted_secure_messages_for_owner_identity(
            &connection,
            &owner_identity_id,
            &["wire-p5-backlog".to_owned()],
        )
        .unwrap();
    assert_eq!(cached.len(), 1);
    assert!(p5_cache_binding_from_record(&cached[0]).is_some());
    drop(connection);

    for _ in 0..2 {
        let runtime = MessageReadRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::new(RefCell::new(Vec::new())),
                response: json!({"messages": [wire.clone()], "has_more": false}),
            },
            StaticHandleDirectoryTransport,
        );
        let result = runtime
            .inbox_async(InboxRead {
                query: crate::messages::InboxQuery {
                    scope: crate::messages::InboxScope::DirectOnly,
                    limit: crate::ids::PageLimit(20),
                    cursor: None,
                    unread_only: false,
                    inbox_history_options: None,
                },
            })
            .await
            .unwrap();
        assert_eq!(result.page.items.len(), 1);
        assert_eq!(result.page.items[0].id.as_str(), "msg-logical-backlog");
    }

    let connection = crate::internal::local_state::open_writable(&sqlite_path).unwrap();
    assert_eq!(
        crate::internal::local_state::inbound_resolution_backlog::pending_count(
            &connection,
            &owner_identity_id
        )
        .unwrap(),
        0
    );
    let scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
        "user-bob",
        "bob.anpclaw.com",
    )
    .unwrap();
    let records = crate::internal::local_state::messages::list_direct_messages_for_owner_identity(
        &connection,
        &owner_identity_id,
        &[
            crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(
                &scope,
            ),
        ],
        20,
    )
    .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].msg_id, "msg-logical-backlog");
}

#[tokio::test]
async fn cached_p5_projection_restores_logical_id_without_redecrypting_replay() {
    let fixture = VNextCacheFixture::new();
    let client = fixture.client(true);
    let direct_wire = ordinary_p5_cache_message(
        "wire-p5-device-delivery",
        "did:example:bob-new",
        "device-bob",
        &fixture.did,
        &fixture.device_id,
    );
    let own_sync_wire = json_rpc_p5_cache_message(ordinary_p5_cache_message(
        "wire-p5-own-sync-delivery",
        &fixture.did,
        "device-alice-sender",
        &fixture.did,
        &fixture.device_id,
    ));
    let owner_identity_id = client.current_identity().id.as_str().to_owned();
    client
        .core_inner()
        .local_state_db()
        .await
        .unwrap()
        .store_messages(vec![
            crate::internal::local_state::messages::MessageRecord {
                msg_id: "msg-logical-p5".to_owned(),
                owner_identity_id: owner_identity_id.clone(),
                owner_did: fixture.did.clone(),
                conversation_id: "dm:did:example:bob-new".to_owned(),
                thread_id: "dm:did:example:bob-new".to_owned(),
                wire_thread_kind: "direct".to_owned(),
                wire_thread_ref: "did:example:bob-new".to_owned(),
                wire_identity_resolution_state: "resolved".to_owned(),
                direction: 0,
                sender_did: "did:example:bob-new".to_owned(),
                receiver_did: fixture.did.clone(),
                content_type: "text/plain".to_owned(),
                content: "cached p5 plaintext".to_owned(),
                sent_at: "2026-07-21T00:00:00Z".to_owned(),
                stored_at: "2026-07-21T00:00:00Z".to_owned(),
                is_e2ee: true,
                metadata: p5_cache_record_metadata(&direct_wire),
                ..crate::internal::local_state::messages::MessageRecord::default()
            },
            crate::internal::local_state::messages::MessageRecord {
                msg_id: "msg-logical-own-sync".to_owned(),
                owner_identity_id,
                owner_did: fixture.did.clone(),
                conversation_id: "dm:did:example:bob-new".to_owned(),
                thread_id: "dm:did:example:bob-new".to_owned(),
                wire_thread_kind: "direct".to_owned(),
                wire_thread_ref: "did:example:bob-new".to_owned(),
                wire_identity_resolution_state: "resolved".to_owned(),
                direction: 1,
                sender_did: fixture.did.clone(),
                receiver_did: "did:example:bob-new".to_owned(),
                content_type: "text/plain".to_owned(),
                content: "cached own-sync plaintext".to_owned(),
                sent_at: "2026-07-21T00:00:01Z".to_owned(),
                stored_at: "2026-07-21T00:00:01Z".to_owned(),
                is_e2ee: true,
                metadata: p5_cache_record_metadata(&own_sync_wire),
                ..crate::internal::local_state::messages::MessageRecord::default()
            },
        ])
        .await
        .unwrap();
    let mut raw = json!({"messages": [direct_wire, own_sync_wire], "has_more": false});

    let p5_provenance =
        project_secure_direct_messages_async(&client, &mut raw, &mut NoopDirectoryTransport).await;

    let messages = raw["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["id"], "msg-logical-p5");
    assert_eq!(messages[0]["raw_message_id"], "wire-p5-device-delivery");
    assert_eq!(messages[0]["content"], "cached p5 plaintext");
    assert_eq!(messages[0]["decryption_state"], "decrypted");
    assert_eq!(messages[0]["direction"], 0);
    assert!(messages[0].get("_awiki_p5_cache_authenticated").is_none());
    assert!(metadata_attributes_from_object(
        messages[0].as_object().unwrap(),
        "msg-logical-p5",
        Some("text/plain")
    )
    .iter()
    .any(|attribute| {
        attribute.key == "raw_message_id" && attribute.value == "wire-p5-device-delivery"
    }));
    assert_eq!(messages[1]["id"], "msg-logical-own-sync");
    assert_eq!(messages[1]["raw_message_id"], "wire-p5-own-sync-delivery");
    assert_eq!(messages[1]["sender_did"], fixture.did);
    assert_eq!(messages[1]["receiver_did"], "did:example:bob-new");
    assert_eq!(messages[1]["content"], "cached own-sync plaintext");
    assert_eq!(messages[1]["decryption_state"], "decrypted");
    assert_eq!(messages[1]["direction"], 1);
    let page = page_from_raw(&client, &raw, crate::ids::PageLimit(20)).unwrap();
    assert!(page
        .items
        .iter()
        .all(
            |message| P5_CACHE_METADATA_KEYS.into_iter().all(|key| !message
                .metadata
                .attributes
                .iter()
                .any(|attribute| attribute.key == key))
        ));
    assert!(messages.iter().all(|message| P5_CACHE_METADATA_KEYS
        .into_iter()
        .all(|key| message.get(key).is_none())));
    persist_projection_best_effort_async(&client, &page.items, &p5_provenance).await;
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )
    .unwrap();
    let cached =
        crate::internal::local_state::messages::list_decrypted_secure_messages_for_owner_identity(
            &connection,
            client.current_identity().id.as_str(),
            &[
                "wire-p5-device-delivery".to_owned(),
                "wire-p5-own-sync-delivery".to_owned(),
            ],
        )
        .unwrap();
    assert_eq!(cached.len(), 2);
    assert!(cached.iter().all(|record| {
        p5_cache_record_has_direct_route(record) && p5_cache_binding_from_record(record).is_some()
    }));
}

#[tokio::test]
async fn exact_device_secure_hydration_selects_p5_only_and_never_persists_ordinary_rows() {
    let fixture = VNextCacheFixture::new();
    let client = fixture.client(true);
    let first_wire = ordinary_p5_cache_message(
        "wire-p5-secure-hydration-1",
        "did:example:bob-new",
        "device-bob",
        &fixture.did,
        &fixture.device_id,
    );
    let second_wire = ordinary_p5_cache_message(
        "wire-p5-secure-hydration-2",
        "did:example:bob-new",
        "device-bob",
        &fixture.did,
        &fixture.device_id,
    );
    client
        .core_inner()
        .local_state_db()
        .await
        .unwrap()
        .store_messages(vec![
            p5_cached_incoming_record(
                &client,
                &first_wire,
                "msg-logical-secure-hydration-1",
                "authenticated secure plaintext 1",
            ),
            p5_cached_incoming_record(
                &client,
                &second_wire,
                "msg-logical-secure-hydration-2",
                "authenticated secure plaintext 2",
            ),
        ])
        .await
        .unwrap();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let authentication_reloads = Rc::new(RefCell::new(0_usize));
    let mut transport = ScriptedAuthenticatedTransport {
        calls: Rc::clone(&calls),
        authentication_reloads: Rc::clone(&authentication_reloads),
        responses: VecDeque::from([
            json!({
                "messages": [
                    first_wire,
                    {
                        "id": "msg-ordinary-must-not-fallback",
                        "sender_did": "did:example:bob-new",
                        "receiver_did": &fixture.did,
                        "content_type": "text/plain",
                        "content": "ordinary response injection"
                    }
                ],
                "has_more": true,
                "warnings": []
            }),
            json!({"updated_count": 1, "warnings": []}),
            json!({
                "messages": [second_wire],
                "has_more": false,
                "warnings": []
            }),
            json!({"updated_count": 1, "warnings": []}),
        ]),
    };

    let warnings = hydrate_exact_device_secure_inbox_async(
        &client,
        &mut transport,
        &mut StaticHandleDirectoryTransport,
        20,
    )
    .await
    .unwrap();

    assert!(warnings.is_empty());
    assert_eq!(*authentication_reloads.borrow(), 2);
    let calls = calls.borrow();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.method.as_str())
            .collect::<Vec<_>>(),
        [
            "inbox.get",
            "inbox.mark_read",
            "inbox.get",
            "inbox.mark_read"
        ]
    );
    for call in calls.iter().filter(|call| call.method == "inbox.get") {
        assert_eq!(call.params["body"]["user_did"], fixture.did);
        assert_eq!(call.params["body"]["limit"], 20);
        assert_eq!(call.params["body"]["security_profile"], "direct-e2ee");
        assert_eq!(call.params["body"].as_object().unwrap().len(), 3);
    }
    assert_eq!(
        calls[1].params["body"]["message_ids"],
        json!(["wire-p5-secure-hydration-1"])
    );
    assert_eq!(
        calls[3].params["body"]["message_ids"],
        json!(["wire-p5-secure-hydration-2"])
    );
    drop(calls);

    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )
    .unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE owner_identity_id = ?1 AND msg_id = ?2",
                (
                    client.current_identity().id.as_str(),
                    "msg-logical-secure-hydration-1"
                ),
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE owner_identity_id = ?1 AND msg_id = ?2",
                (
                    client.current_identity().id.as_str(),
                    "msg-logical-secure-hydration-2"
                ),
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE owner_identity_id = ?1 AND msg_id = ?2",
                (
                    client.current_identity().id.as_str(),
                    "msg-ordinary-must-not-fallback"
                ),
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn exact_device_secure_hydration_skips_legacy_inbox_until_and_after_p5_lane_negotiation() {
    let fixture = VNextCacheFixture::new();
    let client = fixture.client(true);
    let owner_identity_id = client.current_identity().id.as_str().to_owned();
    let db = client.core_inner().local_state_db().await.unwrap();
    db.upsert_identity_account_binding(
        crate::internal::local_state::sync_v2::IdentityAccountBinding {
            owner_identity_id: owner_identity_id.clone(),
            account_id: "read-cache-user".to_owned(),
            handle_scope: client.handle().map(|handle| handle.as_str().to_owned()),
            current_did: client.did().as_str().to_owned(),
            protocol_device_id: fixture.device_id.clone(),
            identity_generation: "1".to_owned(),
            device_auth_generation: "1".to_owned(),
            created_at: 1,
            updated_at: 1,
        },
    )
    .await
    .unwrap();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let authentication_reloads = Rc::new(RefCell::new(0_usize));
    let mut transport = ScriptedAuthenticatedTransport {
        calls: Rc::clone(&calls),
        authentication_reloads: Rc::clone(&authentication_reloads),
        responses: VecDeque::new(),
    };
    assert!(hydrate_exact_device_secure_inbox_async(
        &client,
        &mut transport,
        &mut StaticHandleDirectoryTransport,
        20,
    )
    .await
    .unwrap()
    .is_empty());
    assert!(calls.borrow().is_empty());

    db.replace_lane_sync_states(
        &owner_identity_id,
        vec![crate::internal::local_state::sync_v2::LaneSyncState {
            owner_identity_id: owner_identity_id.clone(),
            lane: crate::internal::wire::sync_v2::SyncLaneV3::P5Device,
            stream_epoch: "41".to_owned(),
            scan_seq: "0".to_owned(),
            committed_seq: "0".to_owned(),
        }],
    )
    .await
    .unwrap();

    let warnings = hydrate_exact_device_secure_inbox_async(
        &client,
        &mut transport,
        &mut StaticHandleDirectoryTransport,
        20,
    )
    .await
    .unwrap();

    assert!(warnings.is_empty());
    assert!(calls.borrow().is_empty());
    assert_eq!(*authentication_reloads.borrow(), 0);
}

#[tokio::test]
async fn exact_device_secure_hydration_keeps_committed_projection_when_ack_is_incomplete() {
    let fixture = VNextCacheFixture::new();
    let client = fixture.client(true);
    let wire = ordinary_p5_cache_message(
        "wire-p5-secure-ack-failure",
        "did:example:bob-new",
        "device-bob",
        &fixture.did,
        &fixture.device_id,
    );
    client
        .core_inner()
        .local_state_db()
        .await
        .unwrap()
        .store_messages(vec![p5_cached_incoming_record(
            &client,
            &wire,
            "msg-logical-secure-ack-failure",
            "committed before ACK",
        )])
        .await
        .unwrap();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let authentication_reloads = Rc::new(RefCell::new(0_usize));
    let mut transport = ScriptedAuthenticatedTransport {
        calls: Rc::clone(&calls),
        authentication_reloads: Rc::clone(&authentication_reloads),
        responses: VecDeque::from([
            json!({"messages": [wire], "has_more": false, "warnings": []}),
            json!({"updated_count": 0, "warnings": []}),
        ]),
    };

    let error = hydrate_exact_device_secure_inbox_async(
        &client,
        &mut transport,
        &mut StaticHandleDirectoryTransport,
        20,
    )
    .await
    .expect_err("an incomplete ACK must fail the foreground hydration");

    assert!(matches!(
        error,
        crate::ImError::Service { code: Some(code), .. }
            if code == "secure_inbox_ack_incomplete"
    ));
    assert_eq!(*authentication_reloads.borrow(), 1);
    assert_eq!(
        calls
            .borrow()
            .iter()
            .map(|call| call.method.as_str())
            .collect::<Vec<_>>(),
        ["inbox.get", "inbox.mark_read"]
    );
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )
    .unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE owner_identity_id = ?1 AND msg_id = ?2",
                (
                    client.current_identity().id.as_str(),
                    "msg-logical-secure-ack-failure"
                ),
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn verified_scoped_thread_p5_projection_persists_wire_route_and_replays() {
    let fixture = VNextCacheFixture::new();
    let client = fixture.client(true);
    let wire = ordinary_p5_cache_message(
        "wire-p5-scoped-thread",
        "did:example:bob-new",
        "device-bob",
        &fixture.did,
        &fixture.device_id,
    );
    let own_sync_wire = json_rpc_p5_cache_message(ordinary_p5_cache_message(
        "wire-p5-scoped-own-sync",
        &fixture.did,
        "device-alice-sender",
        &fixture.did,
        &fixture.device_id,
    ));
    let mut projected = wire.clone();
    projected["peer_user_id"] = json!("service-forged-user");
    projected["peer_full_handle"] = json!("service-forged.example");
    projected["peer_current_did"] = json!("did:example:mallory");
    projected["resolved_target_did"] = json!("did:example:mallory");
    clear_untrusted_p5_projection_state(std::slice::from_mut(&mut projected));
    assert!(projected.get("peer_user_id").is_none());
    assert!(projected.get("peer_full_handle").is_none());
    assert!(projected.get("peer_current_did").is_none());
    assert!(projected.get("resolved_target_did").is_none());
    apply_p5_v2_product_outcome(
        &mut projected,
        crate::internal::secure_direct::v2_product::V2InboundProductOutcome::Business(
            crate::internal::secure_direct::v2_product::V2InboundBusinessProjection {
                logical_message_id: "msg-logical-scoped-thread".to_owned(),
                conversation_id: None,
                sender_did: "did:example:bob-new".to_owned(),
                sender_device_id: "device-bob".to_owned(),
                recipient_did: fixture.did.clone(),
                wire_message_id: "wire-p5-scoped-thread".to_owned(),
                body: crate::internal::secure_direct::v2_product::V2InboundBusinessBody::Text {
                    text: "scoped thread plaintext".to_owned(),
                    markdown: false,
                },
                session_reply_pending: false,
            },
        ),
    );
    let mut own_sync_projected = own_sync_wire.clone();
    clear_untrusted_p5_projection_state(std::slice::from_mut(&mut own_sync_projected));
    apply_p5_v2_product_outcome(
        &mut own_sync_projected,
        crate::internal::secure_direct::v2_product::V2InboundProductOutcome::OwnSync(
            crate::internal::secure_direct::v2_product::V2InboundOwnSyncProjection {
                logical_message_id: "msg-logical-scoped-own-sync".to_owned(),
                conversation_id: None,
                original_sender_did: fixture.did.clone(),
                original_sender_device_id: "device-alice-sender".to_owned(),
                target_did: "did:example:bob-new".to_owned(),
                wire_message_id: "wire-p5-scoped-own-sync".to_owned(),
                body: crate::internal::secure_direct::v2_product::V2InboundBusinessBody::Text {
                    text: "scoped own-sync plaintext".to_owned(),
                    markdown: false,
                },
                session_reply_pending: false,
            },
        ),
    );
    let mut raw = json!({"messages": [projected, own_sync_projected], "has_more": false});
    let mut provenance = DirectP5ProjectionProvenance::default();
    provenance.record(
        "msg-logical-scoped-thread",
        p5_cache_binding_from_message(&raw["messages"][0])
            .unwrap()
            .unwrap(),
    );
    provenance.record(
        "msg-logical-scoped-own-sync",
        p5_cache_binding_from_message(&raw["messages"][1])
            .unwrap()
            .unwrap(),
    );
    provenance.retain_unambiguous_projected_instances(&client, raw["messages"].as_array().unwrap());
    annotate_direct_peer_scopes_async(
        &client,
        &mut raw,
        &mut StaticHandleDirectoryTransport,
        None,
        None,
        Some(&mut provenance),
    )
    .await;
    let page = page_from_raw(&client, &raw, crate::ids::PageLimit(20)).unwrap();
    assert_eq!(page.items.len(), 2);
    assert!(page
        .items
        .iter()
        .all(|message| matches!(message.thread, crate::messages::ThreadRef::Thread(_))));
    let records = remote_projection_records(&client, &page.items, &provenance).unwrap();
    assert_eq!(records.len(), 2);
    assert!(records.iter().all(|record| {
        p5_cache_record_has_direct_route(record) && p5_cache_binding_from_record(record).is_some()
    }));
    client
        .core_inner()
        .local_state_db()
        .await
        .unwrap()
        .store_messages(records)
        .await
        .unwrap();

    for _ in 0..2 {
        let result = MessageReadRuntime::new(
            &client,
            ReadyAnyReadSessionProvider,
            RecordingTransport {
                calls: Rc::new(RefCell::new(Vec::new())),
                response: json!({
                    "messages": [wire.clone(), own_sync_wire.clone()],
                    "has_more": false
                }),
            },
            StaticHandleDirectoryTransport,
        )
        .inbox_async(InboxRead {
            query: crate::messages::InboxQuery {
                scope: crate::messages::InboxScope::DirectOnly,
                limit: crate::ids::PageLimit(20),
                cursor: None,
                unread_only: false,
                inbox_history_options: None,
            },
        })
        .await
        .unwrap();
        assert_eq!(result.page.items.len(), 2);
        let ids = result
            .page
            .items
            .iter()
            .map(|message| message.id.as_str())
            .collect::<HashSet<_>>();
        assert_eq!(
            ids,
            HashSet::from(["msg-logical-scoped-thread", "msg-logical-scoped-own-sync"])
        );
    }
}

#[tokio::test]
async fn legacy_profile_fallback_cannot_attest_scoped_p5_cache() {
    let fixture = VNextCacheFixture::new();
    let client = fixture.client(true);
    let wire = ordinary_p5_cache_message(
        "wire-p5-legacy-profile",
        "did:example:legacy-peer",
        "device-legacy-peer",
        &fixture.did,
        &fixture.device_id,
    );
    client
        .core_inner()
        .local_state_db()
        .await
        .unwrap()
        .store_messages(vec![p5_cached_incoming_record(
            &client,
            &wire,
            "msg-logical-legacy-profile",
            "legacy profile plaintext",
        )])
        .await
        .unwrap();
    let result = MessageReadRuntime::new(
        &client,
        ReadyAnyReadSessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response: json!({"messages": [wire], "has_more": false}),
        },
        ProfileFallbackDirectoryTransport::legacy(json!({
            "id": "did:example:legacy-peer",
            "profile": {
                "user_id": "legacy-user",
                "full_handle": "legacy.anpclaw.com"
            }
        })),
    )
    .inbox_async(InboxRead {
        query: crate::messages::InboxQuery {
            scope: crate::messages::InboxScope::DirectOnly,
            limit: crate::ids::PageLimit(20),
            cursor: None,
            unread_only: false,
            inbox_history_options: None,
        },
    })
    .await
    .unwrap();
    assert_eq!(result.page.items.len(), 1);
    assert!(matches!(
        result.page.items[0].thread,
        crate::messages::ThreadRef::Direct(_)
    ));
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )
    .unwrap();
    let cached =
        crate::internal::local_state::messages::list_decrypted_secure_messages_for_owner_identity(
            &connection,
            client.current_identity().id.as_str(),
            &["wire-p5-legacy-profile".to_owned()],
        )
        .unwrap();
    assert_eq!(cached.len(), 1);
    assert!(p5_cache_record_has_direct_route(&cached[0]));
    assert!(p5_cache_binding_from_record(&cached[0]).is_some());
    assert!(!cached[0].conversation_id.starts_with("dm:peer-scope:"));
}

#[test]
fn self_reported_scoped_thread_without_verified_route_cannot_mint_p5_cache() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let wire = ordinary_p5_cache_message(
        "wire-p5-unverified-thread",
        "did:example:bob-new",
        "device-bob",
        "did:example:alice",
        "device-alice",
    );
    let mut projected = wire.clone();
    apply_p5_v2_product_outcome(
        &mut projected,
        crate::internal::secure_direct::v2_product::V2InboundProductOutcome::Business(
            crate::internal::secure_direct::v2_product::V2InboundBusinessProjection {
                logical_message_id: "msg-logical-unverified-thread".to_owned(),
                conversation_id: None,
                sender_did: "did:example:bob-new".to_owned(),
                sender_device_id: "device-bob".to_owned(),
                recipient_did: "did:example:alice".to_owned(),
                wire_message_id: "wire-p5-unverified-thread".to_owned(),
                body: crate::internal::secure_direct::v2_product::V2InboundBusinessBody::Text {
                    text: "unverified scoped plaintext".to_owned(),
                    markdown: false,
                },
                session_reply_pending: false,
            },
        ),
    );
    let scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
        "service-forged-user",
        "service-forged.example",
    )
    .unwrap();
    annotate_object_with_peer_scope(
        projected.as_object_mut().unwrap(),
        &scope,
        Some("did:example:bob-new"),
    );
    let raw = json!({"messages": [projected], "has_more": false});
    let mut provenance = DirectP5ProjectionProvenance::default();
    provenance.record(
        "msg-logical-unverified-thread",
        p5_cache_binding_from_message(&wire).unwrap().unwrap(),
    );
    let page = page_from_raw(&client, &raw, crate::ids::PageLimit(20)).unwrap();
    assert!(matches!(
        page.items[0].thread,
        crate::messages::ThreadRef::Thread(_)
    ));
    let records = remote_projection_records(&client, &page.items, &provenance).unwrap();
    assert_eq!(records.len(), 1);
    assert!(p5_cache_binding_from_record(&records[0]).is_none());
    assert!(
        p5_cache_record_has_direct_route(&records[0]),
        "authenticated Direct endpoints may preserve their wire route without attesting a P5 cache"
    );
}

#[test]
fn authenticated_p5_business_receiver_overrides_service_projection() {
    let mut message = ordinary_p5_cache_message(
        "wire-p5-authenticated-receiver",
        "did:example:bob-new",
        "device-bob",
        "did:example:alice",
        "device-alice",
    );
    message["receiver_did"] = json!("did:example:service-forged");

    apply_p5_v2_product_outcome(
        &mut message,
        crate::internal::secure_direct::v2_product::V2InboundProductOutcome::Business(
            crate::internal::secure_direct::v2_product::V2InboundBusinessProjection {
                logical_message_id: "msg-p5-authenticated-receiver".to_owned(),
                conversation_id: None,
                sender_did: "did:example:bob-new".to_owned(),
                sender_device_id: "device-bob".to_owned(),
                recipient_did: "did:example:alice".to_owned(),
                wire_message_id: "wire-p5-authenticated-receiver".to_owned(),
                body: crate::internal::secure_direct::v2_product::V2InboundBusinessBody::Text {
                    text: "authenticated endpoints".to_owned(),
                    markdown: false,
                },
                session_reply_pending: false,
            },
        ),
    );

    assert_eq!(message["sender_did"], "did:example:bob-new");
    assert_eq!(message["receiver_did"], "did:example:alice");
}

#[tokio::test]
async fn fresh_scoped_p5_rejection_then_correct_receive_projects_and_persists() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let (mut wire, outcome) = crate::internal::secure_direct::v2_product::v2_product_tests::fresh_scoped_business_receive_for_projection_test().await;
    let binding = p5_cache_binding_from_message(&wire).unwrap().unwrap();
    apply_p5_v2_product_outcome(&mut wire, outcome);
    assert_eq!(wire["receiver_did"], "did:example:alice");

    let mut raw = json!({"messages": [wire], "has_more": false});
    let mut provenance = DirectP5ProjectionProvenance::default();
    provenance.record("logical-scoped-business", binding);
    assert!(!retain_direct_messages_for_expected_peer(
        &client,
        &mut raw,
        "did:example:mallory",
        &mut provenance,
    ));
    annotate_direct_peer_scopes_async(
        &client,
        &mut raw,
        &mut StaticHandleDirectoryTransport,
        None,
        Some("did:example:mallory"),
        Some(&mut provenance),
    )
    .await;
    let scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
        "user-bob",
        "bob.anpclaw.com",
    )
    .unwrap();
    let page = page_from_raw(&client, &raw, crate::ids::PageLimit(20)).unwrap();
    assert_eq!(page.items.len(), 1);
    assert!(matches!(
        &page.items[0].body,
        crate::messages::MessageBodyView::Text { text, .. } if text == "scoped business"
    ));
    assert_eq!(
        persist_projection_async(&client, &page.items, &provenance)
            .await
            .unwrap()
            .stored_messages,
        1
    );
    let records = client
        .core_inner()
        .local_state_db()
        .await
        .unwrap()
        .list_direct_messages(
            "alice-id",
            vec![
                crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(
                    &scope,
                ),
            ],
            20,
        )
        .await
        .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].msg_id, "logical-scoped-business");
    assert_eq!(records[0].content, "scoped business");
}

#[tokio::test]
async fn history_runtime_fresh_p5_mixed_page_rejects_wrong_peer_and_reopens_exactly_once() {
    use crate::internal::secure_direct::v2_product::v2_product_tests::{
        prepare_runtime_p5_test_wires, RuntimeP5TestBody, RuntimeP5TestClientFixture,
        RuntimeP5TestWire,
    };

    const BOB_DID: &str = "did:example:runtime-history-bob";
    const MALLORY_DID: &str = "did:example:runtime-history-mallory";
    let fixture = RuntimeP5TestClientFixture::new("runtime-history-wire");
    let client = fixture.client();
    let wires = prepare_runtime_p5_test_wires(
        &client,
        vec![
            RuntimeP5TestWire {
                peer_did: MALLORY_DID,
                peer_device_id: "runtime-history-mallory-device",
                seed: 161,
                logical_message_id: "runtime-history-wrong",
                server_seq: 41,
                body: RuntimeP5TestBody::Text("wrong peer encrypted text"),
            },
            RuntimeP5TestWire {
                peer_did: BOB_DID,
                peer_device_id: "runtime-history-bob-device",
                seed: 171,
                logical_message_id: "runtime-history-correct",
                server_seq: 42,
                body: RuntimeP5TestBody::Text("correct peer encrypted text"),
            },
        ],
    )
    .await;
    assert_eq!(wires.len(), 2);
    let serialized_wire = serde_json::to_string(&wires).unwrap();
    assert!(!serialized_wire.contains("wrong peer encrypted text"));
    assert!(!serialized_wire.contains("correct peer encrypted text"));
    let wrong_wire = wires[0].clone();
    let correct_wire = wires[1].clone();
    let correct_wire_id = correct_wire["id"].as_str().unwrap().to_owned();
    let bob_scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
        "runtime-history-bob-user",
        "runtime-history-bob.example",
    )
    .unwrap();
    let request = || HistoryRead {
        thread: crate::messages::ThreadRef::Direct(
            crate::ids::PeerRef::parse(BOB_DID, "").unwrap(),
        ),
        query: crate::messages::HistoryQuery {
            limit: crate::ids::PageLimit(20),
            cursor: None,
            inbox_history_options: None,
        },
        resolved_peer_did: Some(BOB_DID.to_owned()),
        peer_scope: Some(bob_scope.clone()),
    };

    let first = MessageReadRuntime::new(
        &client,
        ReadyAnyReadSessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response: json!({
                "messages": [wrong_wire.clone(), correct_wire.clone()],
                "has_more": false
            }),
        },
        NoopDirectoryTransport,
    )
    .history_async(request())
    .await
    .unwrap();
    assert_eq!(first.page.items.len(), 1);
    assert_eq!(first.page.items[0].id.as_str(), "runtime-history-correct");
    assert!(matches!(
        &first.page.items[0].body,
        crate::messages::MessageBodyView::Text { text, .. }
            if text == "correct peer encrypted text"
    ));

    let reopened = fixture.client();
    let replay = MessageReadRuntime::new(
        &reopened,
        ReadyAnyReadSessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response: json!({
                "messages": [wrong_wire.clone(), correct_wire],
                "has_more": false
            }),
        },
        NoopDirectoryTransport,
    )
    .history_async(request())
    .await
    .unwrap();
    assert_eq!(replay.page.items.len(), 1);
    assert_eq!(replay.page.items[0].id.as_str(), "runtime-history-correct");

    let connection = crate::internal::local_state::open_writable(&fixture.sqlite_path()).unwrap();
    let bob_records =
        crate::internal::local_state::messages::list_decrypted_secure_messages_for_owner_identity(
            &connection,
            reopened.current_identity().id.as_str(),
            &[correct_wire_id],
        )
        .unwrap();
    assert_eq!(bob_records.len(), 1);
    assert_eq!(bob_records[0].msg_id, "runtime-history-correct");
    drop(connection);

    // The same ciphertext must still decrypt under its authenticated peer scope.
    // If the mixed Bob page had committed Mallory's replay state, this returns no item.
    let mallory = MessageReadRuntime::new(
        &reopened,
        ReadyAnyReadSessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response: json!({"messages": [wrong_wire], "has_more": false}),
        },
        NoopDirectoryTransport,
    )
    .history_async(HistoryRead {
        thread: crate::messages::ThreadRef::Direct(
            crate::ids::PeerRef::parse(MALLORY_DID, "").unwrap(),
        ),
        query: crate::messages::HistoryQuery {
            limit: crate::ids::PageLimit(20),
            cursor: None,
            inbox_history_options: None,
        },
        resolved_peer_did: Some(MALLORY_DID.to_owned()),
        peer_scope: Some(
            crate::internal::local_state::owner_scope::DirectPeerScope::new(
                "runtime-history-mallory-user",
                "runtime-history-mallory.example",
            )
            .unwrap(),
        ),
    })
    .await
    .unwrap();
    assert_eq!(mallory.page.items.len(), 1);
    assert_eq!(mallory.page.items[0].id.as_str(), "runtime-history-wrong");
}

#[tokio::test]
async fn sync_runtime_fresh_p5_mixed_page_rejects_wrong_peer_and_reopens_exactly_once() {
    use crate::internal::secure_direct::v2_product::v2_product_tests::{
        prepare_runtime_p5_test_wires, RuntimeP5TestBody, RuntimeP5TestClientFixture,
        RuntimeP5TestWire,
    };

    const BOB_DID: &str = "did:example:runtime-sync-bob";
    const MALLORY_DID: &str = "did:example:runtime-sync-mallory";
    let fixture = RuntimeP5TestClientFixture::new("runtime-sync-wire");
    let client = fixture.client();
    let wires = prepare_runtime_p5_test_wires(
        &client,
        vec![
            RuntimeP5TestWire {
                peer_did: MALLORY_DID,
                peer_device_id: "runtime-sync-mallory-device",
                seed: 181,
                logical_message_id: "runtime-sync-wrong",
                server_seq: 51,
                body: RuntimeP5TestBody::Text("wrong sync encrypted text"),
            },
            RuntimeP5TestWire {
                peer_did: BOB_DID,
                peer_device_id: "runtime-sync-bob-device",
                seed: 191,
                logical_message_id: "runtime-sync-correct",
                server_seq: 52,
                body: RuntimeP5TestBody::Text("correct sync encrypted text"),
            },
        ],
    )
    .await;
    assert!(!serde_json::to_string(&wires)
        .unwrap()
        .contains("correct sync encrypted text"));
    let wrong_wire = wires[0].clone();
    let correct_wire = wires[1].clone();
    let correct_wire_id = correct_wire["id"].as_str().unwrap().to_owned();
    let bob_scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
        "runtime-sync-bob-user",
        "runtime-sync-bob.example",
    )
    .unwrap();
    seed_sync_thread_binding_for_test(&client, &bob_scope, "conversation-ref-runtime-bob");
    let mallory_scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
        "runtime-sync-mallory-user",
        "runtime-sync-mallory.example",
    )
    .unwrap();
    seed_sync_thread_binding_for_test(&client, &mallory_scope, "conversation-ref-runtime-mallory");
    let mut wrong_wire = wrong_wire;
    wrong_wire["thread_kind"] = json!("direct");
    let mut correct_wire = correct_wire;
    correct_wire["thread_kind"] = json!("direct");
    let request = || crate::internal::message_runtime::sync::SyncThreadAfterInput {
        request: crate::messages::SyncThreadAfterRequest {
            thread: crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse(BOB_DID, "").unwrap(),
            ),
            after_server_seq: Some("0".to_owned()),
            limit: Some(20),
        },
        resolved_peer_did: Some(BOB_DID.to_owned()),
        peer_scope: Some(bob_scope.clone()),
    };

    let first = crate::internal::message_runtime::sync::MessageSyncRuntime::new(
        &client,
        ReadyAnyReadSessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response: json!({
                "messages": [wrong_wire.clone(), correct_wire.clone()],
                "next_after_server_seq": "52",
                "has_more": false,
                "warnings": []
            }),
        },
        NoopDirectoryTransport,
    )
    .sync_thread_after_async(request())
    .await
    .unwrap();
    assert!(first.messages.is_empty());

    let reopened = fixture.client();
    let replay = crate::internal::message_runtime::sync::MessageSyncRuntime::new(
        &reopened,
        ReadyAnyReadSessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response: json!({
                "messages": [wrong_wire.clone(), correct_wire],
                "next_after_server_seq": "52",
                "has_more": false,
                "warnings": []
            }),
        },
        NoopDirectoryTransport,
    )
    .sync_thread_after_async(request())
    .await
    .unwrap();
    assert!(replay.messages.is_empty());

    let connection = crate::internal::local_state::open_writable(&fixture.sqlite_path()).unwrap();
    let records =
        crate::internal::local_state::messages::list_decrypted_secure_messages_for_owner_identity(
            &connection,
            reopened.current_identity().id.as_str(),
            &[correct_wire_id],
        )
        .unwrap();
    assert!(records.is_empty());
    drop(connection);

    let mallory = crate::internal::message_runtime::sync::MessageSyncRuntime::new(
        &reopened,
        ReadyAnyReadSessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response: json!({
                "messages": [wrong_wire],
                "next_after_server_seq": "51",
                "has_more": false,
                "warnings": []
            }),
        },
        NoopDirectoryTransport,
    )
    .sync_thread_after_async(
        crate::internal::message_runtime::sync::SyncThreadAfterInput {
            request: crate::messages::SyncThreadAfterRequest {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse(MALLORY_DID, "").unwrap(),
                ),
                after_server_seq: Some("0".to_owned()),
                limit: Some(20),
            },
            resolved_peer_did: Some(MALLORY_DID.to_owned()),
            peer_scope: Some(mallory_scope),
        },
    )
    .await
    .unwrap();
    assert!(mallory.messages.is_empty());
}

#[tokio::test]
async fn scoped_history_and_sync_fail_closed_on_nonadvancing_wrong_peer_page() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let response = json!({
        "messages": [{
            "id": "wrong-peer-page",
            "sender_did": "did:example:mallory",
            "receiver_did": "did:example:alice",
            "content": "wrong peer",
            "content_type": "text/plain",
            "server_seq": 7
        }],
        "has_more": true
    });
    let input = HistoryRead {
        thread: crate::messages::ThreadRef::Direct(
            crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
        ),
        query: crate::messages::HistoryQuery {
            limit: crate::ids::PageLimit(20),
            cursor: None,
            inbox_history_options: None,
        },
        resolved_peer_did: Some("did:example:bob".to_owned()),
        peer_scope: None,
    };
    let history_error = MessageReadRuntime::new(
        &client,
        ReadyAnyReadSessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response: response.clone(),
        },
        NoopDirectoryTransport,
    )
    .history_async(input)
    .await
    .unwrap_err();
    assert!(matches!(
        history_error,
        crate::ImError::IdentityBindingConflict { .. }
    ));

    let bob_scope =
        crate::internal::local_state::owner_scope::DirectPeerScope::new("user-bob", "bob.example")
            .unwrap();
    seed_sync_thread_binding_for_test(&client, &bob_scope, "conversation-ref-bob");
    let sync_error = crate::internal::message_runtime::sync::MessageSyncRuntime::new(
        &client,
        ReadyAnyReadSessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response: json!({
                "messages": [{
                    "id": "wrong-peer-page",
                    "thread_kind": "direct",
                    "sender_did": "did:example:mallory",
                    "receiver_did": "did:example:alice",
                    "content": "wrong peer",
                    "content_type": "text/plain",
                    "server_seq": 7
                }],
                "next_after_server_seq": "7",
                "has_more": true,
                "warnings": []
            }),
        },
        NoopDirectoryTransport,
    )
    .sync_thread_after_async(
        crate::internal::message_runtime::sync::SyncThreadAfterInput {
            request: crate::messages::SyncThreadAfterRequest {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                after_server_seq: Some("0".to_owned()),
                limit: Some(20),
            },
            resolved_peer_did: Some("did:example:bob".to_owned()),
            peer_scope: Some(bob_scope),
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(
        sync_error,
        crate::ImError::IdentityBindingConflict { .. }
    ));

    let empty_history_error = MessageReadRuntime::new(
        &client,
        ReadyAnyReadSessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response: json!({"messages": [], "has_more": true}),
        },
        NoopDirectoryTransport,
    )
    .history_async(HistoryRead {
        thread: crate::messages::ThreadRef::Direct(
            crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
        ),
        query: crate::messages::HistoryQuery {
            limit: crate::ids::PageLimit(20),
            cursor: None,
            inbox_history_options: None,
        },
        resolved_peer_did: Some("did:example:bob".to_owned()),
        peer_scope: None,
    })
    .await
    .unwrap_err();
    assert!(matches!(
        empty_history_error,
        crate::ImError::IdentityBindingConflict { .. }
    ));

    let empty_terminal = MessageReadRuntime::new(
        &client,
        ReadyAnyReadSessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response: json!({"messages": [], "has_more": false}),
        },
        NoopDirectoryTransport,
    )
    .history_async(HistoryRead {
        thread: crate::messages::ThreadRef::Direct(
            crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
        ),
        query: crate::messages::HistoryQuery {
            limit: crate::ids::PageLimit(20),
            cursor: None,
            inbox_history_options: None,
        },
        resolved_peer_did: Some("did:example:bob".to_owned()),
        peer_scope: None,
    })
    .await
    .unwrap();
    assert!(empty_terminal.page.items.is_empty());
}

#[tokio::test]
async fn scoped_history_mixed_page_keeps_and_persists_requested_peer_once() {
    let fixture = VNextCacheFixture::new();
    let client = fixture.client(true);
    let requested_peer_did = "did:example:bob-new";
    let requested_scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
        "user-bob",
        "bob.anpclaw.com",
    )
    .unwrap();
    let requested_wire = json!({
        "id": "logical-mixed-requested",
        "sender_did": requested_peer_did,
        "receiver_did": &fixture.did,
        "content": "requested fresh plaintext",
        "content_type": "text/plain",
        "server_seq": 11
    });
    let mut wrong_wire = ordinary_p5_cache_message(
        "wire-p5-mixed-wrong",
        "did:example:mallory",
        "device-mallory",
        &fixture.did,
        &fixture.device_id,
    );
    wrong_wire["server_seq"] = json!(12);
    client
        .core_inner()
        .local_state_db()
        .await
        .unwrap()
        .store_messages(vec![p5_cached_incoming_record(
            &client,
            &wrong_wire,
            "logical-p5-mixed-wrong",
            "wrong cached plaintext",
        )])
        .await
        .unwrap();

    let first = MessageReadRuntime::new(
        &client,
        ReadyAnyReadSessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response: json!({
                "messages": [requested_wire.clone(), wrong_wire.clone()],
                "has_more": true
            }),
        },
        StaticHandleDirectoryTransport,
    )
    .history_async(HistoryRead {
        thread: crate::messages::ThreadRef::Direct(
            crate::ids::PeerRef::parse(requested_peer_did, "").unwrap(),
        ),
        query: crate::messages::HistoryQuery {
            limit: crate::ids::PageLimit(20),
            cursor: None,
            inbox_history_options: None,
        },
        resolved_peer_did: Some(requested_peer_did.to_owned()),
        peer_scope: None,
    })
    .await
    .unwrap();
    assert_eq!(first.page.items.len(), 1);
    assert_eq!(first.page.items[0].id.as_str(), "logical-mixed-requested");

    let scoped_records = client
        .core_inner()
        .local_state_db()
        .await
        .unwrap()
        .list_direct_messages(
            client.current_identity().id.as_str(),
            vec![
                crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(
                    &requested_scope,
                ),
            ],
            20,
        )
        .await
        .unwrap();
    assert_eq!(scoped_records.len(), 1);
    assert_eq!(scoped_records[0].msg_id, "logical-mixed-requested");

    let reopened = fixture.client(true);
    let replay = MessageReadRuntime::new(
        &reopened,
        ReadyAnyReadSessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response: json!({
                "messages": [requested_wire, wrong_wire],
                "has_more": false
            }),
        },
        StaticHandleDirectoryTransport,
    )
    .history_async(HistoryRead {
        thread: crate::messages::ThreadRef::Direct(
            crate::ids::PeerRef::parse(requested_peer_did, "").unwrap(),
        ),
        query: crate::messages::HistoryQuery {
            limit: crate::ids::PageLimit(20),
            cursor: None,
            inbox_history_options: None,
        },
        resolved_peer_did: Some(requested_peer_did.to_owned()),
        peer_scope: None,
    })
    .await
    .unwrap();
    assert_eq!(replay.page.items.len(), 1);
    assert_eq!(replay.page.items[0].id.as_str(), "logical-mixed-requested");
}

#[tokio::test]
async fn history_and_sync_reject_authenticated_p5_for_unrequested_peer() {
    let fixture = VNextCacheFixture::new();
    let client = fixture.client(true);
    let mut incoming_wire = ordinary_p5_cache_message(
        "wire-p5-wrong-history-peer",
        "did:example:mallory",
        "device-mallory",
        &fixture.did,
        &fixture.device_id,
    );
    let mut own_sync_wire = json_rpc_p5_cache_message(ordinary_p5_cache_message(
        "wire-p5-wrong-history-own-sync",
        &fixture.did,
        "device-alice-sender",
        &fixture.did,
        &fixture.device_id,
    ));
    incoming_wire["thread_kind"] = json!("direct");
    own_sync_wire["thread_kind"] = json!("direct");
    let owner_identity_id = client.current_identity().id.as_str().to_owned();
    client
        .core_inner()
        .local_state_db()
        .await
        .unwrap()
        .store_messages(vec![
            p5_cached_incoming_record(
                &client,
                &incoming_wire,
                "msg-logical-wrong-history-peer",
                "wrong history peer plaintext",
            ),
            crate::internal::local_state::messages::MessageRecord {
                msg_id: "msg-logical-wrong-history-own-sync".to_owned(),
                owner_identity_id,
                owner_did: fixture.did.clone(),
                conversation_id: "dm:did:example:mallory".to_owned(),
                thread_id: "dm:did:example:mallory".to_owned(),
                wire_thread_kind: "direct".to_owned(),
                wire_thread_ref: "did:example:mallory".to_owned(),
                wire_identity_resolution_state: "resolved".to_owned(),
                direction: 1,
                sender_did: fixture.did.clone(),
                receiver_did: "did:example:mallory".to_owned(),
                content_type: "text/plain".to_owned(),
                content: "wrong history own-sync plaintext".to_owned(),
                is_e2ee: true,
                metadata: p5_cache_record_metadata(&own_sync_wire),
                ..crate::internal::local_state::messages::MessageRecord::default()
            },
        ])
        .await
        .unwrap();
    let requested_peer_did = "did:example:bob-new";
    let requested_scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
        "user-bob",
        "bob.anpclaw.com",
    )
    .unwrap();
    seed_sync_thread_binding_for_test(&client, &requested_scope, "conversation-ref-bob-new");
    let response = json!({
        "messages": [incoming_wire.clone(), own_sync_wire.clone()],
        "next_after_server_seq": "1",
        "has_more": false,
        "warnings": []
    });

    let history = MessageReadRuntime::new(
        &client,
        ReadyAnyReadSessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response: response.clone(),
        },
        StaticHandleDirectoryTransport,
    )
    .history_async(HistoryRead {
        thread: crate::messages::ThreadRef::Direct(
            crate::ids::PeerRef::parse(requested_peer_did, "").unwrap(),
        ),
        query: crate::messages::HistoryQuery {
            limit: crate::ids::PageLimit(20),
            cursor: None,
            inbox_history_options: None,
        },
        resolved_peer_did: Some(requested_peer_did.to_owned()),
        peer_scope: Some(requested_scope.clone()),
    })
    .await
    .unwrap();
    assert!(history.page.items.is_empty());

    let sync = crate::internal::message_runtime::sync::MessageSyncRuntime::new(
        &client,
        ReadyAnyReadSessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response,
        },
        StaticHandleDirectoryTransport,
    )
    .sync_thread_after_async(
        crate::internal::message_runtime::sync::SyncThreadAfterInput {
            request: crate::messages::SyncThreadAfterRequest {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse(requested_peer_did, "").unwrap(),
                ),
                after_server_seq: None,
                limit: Some(20),
            },
            resolved_peer_did: Some(requested_peer_did.to_owned()),
            peer_scope: Some(requested_scope.clone()),
        },
    )
    .await
    .unwrap();
    assert!(sync.messages.is_empty());

    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )
    .unwrap();
    let requested_records =
        crate::internal::local_state::messages::list_direct_messages_for_owner_identity(
            &connection,
            client.current_identity().id.as_str(),
            &[
                crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(
                    &requested_scope,
                ),
            ],
            20,
        )
        .unwrap();
    assert!(requested_records.is_empty());
}

#[tokio::test]
async fn cached_p5_projection_sync_restores_authorized_plaintext() {
    let fixture = VNextCacheFixture::new();
    let client = fixture.client(true);
    let wire = ordinary_p5_cache_message(
        "wire-p5-sync-cache-hit",
        "did:example:bob-new",
        "device-bob",
        &fixture.did,
        &fixture.device_id,
    );
    client
        .core_inner()
        .local_state_db()
        .await
        .unwrap()
        .store_messages(vec![p5_cached_incoming_record(
            &client,
            &wire,
            "msg-logical-sync-cache-hit",
            "sync cached plaintext",
        )])
        .await
        .unwrap();
    let mut raw = json!({"messages": [wire], "has_more": false});

    project_secure_direct_messages(&client, &mut raw, &mut NoopDirectoryTransport);

    let messages = raw["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["id"], "msg-logical-sync-cache-hit");
    assert_eq!(messages[0]["content"], "sync cached plaintext");
    assert_eq!(messages[0]["decryption_state"], "decrypted");
    assert!(P5_CACHE_METADATA_KEYS
        .into_iter()
        .all(|key| messages[0].get(key).is_none()));
}

#[tokio::test]
async fn direct_inbox_dedupe_collision_cannot_move_p5_provenance_to_plaintext() {
    let fixture = VNextCacheFixture::new();
    let client = fixture.client(true);
    let message_id = "msg-p5-direct-inbox-collision";
    let wire = ordinary_p5_cache_message(
        message_id,
        "did:example:bob-new",
        "device-bob",
        &fixture.did,
        &fixture.device_id,
    );
    client
        .core_inner()
        .local_state_db()
        .await
        .unwrap()
        .store_messages(vec![p5_cached_incoming_record(
            &client,
            &wire,
            message_id,
            "authenticated cached plaintext",
        )])
        .await
        .unwrap();
    let injected = json!({
        "message_id": message_id,
        "sender_did": "did:example:bob-new",
        "receiver_did": &fixture.did,
        "content": "service injected plaintext",
        "content_type": "text/plain",
        "server_seq": 3
    });

    let mut projected_raw =
        json!({"messages": [injected.clone(), wire.clone()], "has_more": false});
    let provenance =
        project_secure_direct_messages(&client, &mut projected_raw, &mut NoopDirectoryTransport);
    let mut projected_page =
        page_from_raw(&client, &projected_raw, crate::ids::PageLimit(20)).unwrap();
    dedupe_and_truncate_messages(&mut projected_page.items, crate::ids::PageLimit(20));
    let records = remote_projection_records(&client, &projected_page.items, &provenance).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].content, "service injected plaintext");
    assert!(p5_cache_binding_from_record(&records[0]).is_none());

    let result = MessageReadRuntime::new(
        &client,
        ReadyAnyReadSessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response: json!({"messages": [injected, wire.clone()], "has_more": false}),
        },
        NoopDirectoryTransport,
    )
    .inbox_async(InboxRead {
        query: crate::messages::InboxQuery {
            scope: crate::messages::InboxScope::DirectOnly,
            limit: crate::ids::PageLimit(20),
            cursor: None,
            unread_only: false,
            inbox_history_options: None,
        },
    })
    .await
    .unwrap();

    assert_eq!(result.page.items.len(), 1);
    assert_eq!(result.page.items[0].id.as_str(), message_id);
    assert!(matches!(
        &result.page.items[0].body,
        crate::messages::MessageBodyView::Text { text, .. }
            if text == "service injected plaintext"
    ));
    let replay = MessageReadRuntime::new(
        &client,
        ReadyAnyReadSessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response: json!({"messages": [wire], "has_more": false}),
        },
        StaticHandleDirectoryTransport,
    )
    .history_async(HistoryRead {
        thread: crate::messages::ThreadRef::Direct(
            crate::ids::PeerRef::parse("did:example:bob-new", "").unwrap(),
        ),
        query: crate::messages::HistoryQuery {
            limit: crate::ids::PageLimit(20),
            cursor: None,
            inbox_history_options: None,
        },
        resolved_peer_did: Some("did:example:bob-new".to_owned()),
        peer_scope: None,
    })
    .await
    .unwrap();
    assert!(replay.page.items.iter().all(|message| !matches!(
        &message.body,
        crate::messages::MessageBodyView::Text { text, .. }
            if text == "service injected plaintext"
    )));
}

#[tokio::test]
async fn sync_thread_after_duplicate_instance_cannot_persist_p5_provenance() {
    let fixture = VNextCacheFixture::new();
    let client = fixture.client(true);
    let message_id = "msg-p5-thread-after-collision";
    let mut wire = ordinary_p5_cache_message(
        message_id,
        "did:example:bob-new",
        "device-bob",
        &fixture.did,
        &fixture.device_id,
    );
    wire["thread_kind"] = json!("direct");
    let peer_scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
        "user-bob",
        "bob.anpclaw.com",
    )
    .unwrap();
    let conversation_id =
        crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(
            &peer_scope,
        );
    let db = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )
    .unwrap();
    crate::internal::local_state::sync_v2::upsert_identity_account_binding(
        &db,
        &crate::internal::local_state::sync_v2::IdentityAccountBinding {
            owner_identity_id: client.current_identity().id.as_str().to_owned(),
            account_id: "account-read-cache".to_owned(),
            handle_scope: Some("read-cache.awiki.test".to_owned()),
            current_did: fixture.did.clone(),
            protocol_device_id: fixture.device_id.clone(),
            identity_generation: "1".to_owned(),
            device_auth_generation: "1".to_owned(),
            created_at: 1,
            updated_at: 1,
        },
    )
    .unwrap();
    crate::internal::local_state::sync_v2::upsert_sync_thread_binding(
        &db,
        &crate::internal::local_state::sync_v2::SyncThreadBinding {
            owner_identity_id: client.current_identity().id.as_str().to_owned(),
            remote_thread_key: "conversation-ref-bob".to_owned(),
            thread_kind: "direct".to_owned(),
            conversation_id,
            updated_at: 1,
        },
    )
    .unwrap();
    client
        .core_inner()
        .local_state_db()
        .await
        .unwrap()
        .store_messages(vec![p5_cached_incoming_record(
            &client,
            &wire,
            message_id,
            "authenticated cached plaintext",
        )])
        .await
        .unwrap();
    let injected = json!({
        "message_id": message_id,
        "thread_kind": "direct",
        "sender_did": "did:example:bob-new",
        "receiver_did": &fixture.did,
        "content": "service injected thread plaintext",
        "content_type": "text/plain",
        "server_seq": 3
    });

    let mut projected_raw =
        json!({"messages": [wire.clone(), injected.clone()], "has_more": false});
    let provenance = project_secure_direct_messages_async(
        &client,
        &mut projected_raw,
        &mut NoopDirectoryTransport,
    )
    .await;
    let mut projected_page =
        page_from_raw(&client, &projected_raw, crate::ids::PageLimit(20)).unwrap();
    projected_page.items.retain(|message| {
        message
            .metadata
            .server_sequence
            .is_some_and(|server_sequence| server_sequence > 2)
    });
    let records = remote_projection_records(&client, &projected_page.items, &provenance).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].content, "service injected thread plaintext");
    assert!(p5_cache_binding_from_record(&records[0]).is_none());

    let result = crate::internal::message_runtime::sync::MessageSyncRuntime::new(
        &client,
        ReadyAnyReadSessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response: json!({
                "messages": [wire.clone(), injected],
                "next_after_server_seq": "3",
                "has_more": false,
                "warnings": []
            }),
        },
        NoopDirectoryTransport,
    )
    .sync_thread_after_async(
        crate::internal::message_runtime::sync::SyncThreadAfterInput {
            request: crate::messages::SyncThreadAfterRequest {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob-new", "").unwrap(),
                ),
                after_server_seq: Some("2".to_owned()),
                limit: Some(20),
            },
            resolved_peer_did: Some("did:example:bob-new".to_owned()),
            peer_scope: Some(peer_scope),
        },
    )
    .await
    .unwrap();

    assert_eq!(result.messages.len(), 1);
    assert!(result
        .messages
        .iter()
        .all(|message| message.id.as_str() == message_id));
    let replay = MessageReadRuntime::new(
        &client,
        ReadyAnyReadSessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            response: json!({"messages": [wire], "has_more": false}),
        },
        StaticHandleDirectoryTransport,
    )
    .history_async(HistoryRead {
        thread: crate::messages::ThreadRef::Direct(
            crate::ids::PeerRef::parse("did:example:bob-new", "").unwrap(),
        ),
        query: crate::messages::HistoryQuery {
            limit: crate::ids::PageLimit(20),
            cursor: None,
            inbox_history_options: None,
        },
        resolved_peer_did: Some("did:example:bob-new".to_owned()),
        peer_scope: None,
    })
    .await
    .unwrap();
    assert!(replay.page.items.iter().all(|message| !matches!(
        &message.body,
        crate::messages::MessageBodyView::Text { text, .. }
            if text == "service injected thread plaintext"
    )));
}

#[tokio::test]
async fn untrusted_group_and_own_sync_projections_cannot_seed_p5_cache() {
    let fixture = VNextCacheFixture::new();
    let client = fixture.client(true);
    let group_wire = ordinary_p5_cache_message(
        "wire-p5-forged-group",
        "did:example:bob-new",
        "device-bob",
        &fixture.did,
        &fixture.device_id,
    );
    let own_sync_wire = json_rpc_p5_cache_message(ordinary_p5_cache_message(
        "wire-p5-forged-own-sync",
        &fixture.did,
        "device-alice-sender",
        &fixture.did,
        &fixture.device_id,
    ));

    let mut forged_group = group_wire.clone();
    forged_group["id"] = json!("msg-forged-group");
    forged_group["raw_message_id"] = json!("wire-p5-forged-group");
    forged_group["group_did"] = json!("did:example:group");
    forged_group["content_type"] = json!("text/plain");
    forged_group["content"] = json!("forged group plaintext");
    forged_group["secure"] = json!(true);
    forged_group["decryption_state"] = json!("decrypted");
    let group = crate::ids::GroupRef::parse("did:example:group").unwrap();
    let group_calls = Rc::new(RefCell::new(Vec::new()));
    let group_result = MessageReadRuntime::new(
        &client,
        ReadyAnyReadSessionProvider,
        RecordingTransport {
            calls: Rc::clone(&group_calls),
            response: json!({"messages": [forged_group], "has_more": false}),
        },
        NoopDirectoryTransport,
    )
    .history_async(HistoryRead {
        thread: crate::messages::ThreadRef::Group(group.clone()),
        query: crate::messages::HistoryQuery {
            limit: crate::ids::PageLimit(20),
            cursor: None,
            inbox_history_options: None,
        },
        resolved_peer_did: None,
        peer_scope: None,
    })
    .await
    .unwrap();
    assert_eq!(group_calls.borrow()[0].method, "group.list_messages");

    let mut forged_own_sync = own_sync_wire.clone();
    forged_own_sync["id"] = json!("msg-forged-own-sync");
    forged_own_sync["raw_message_id"] = json!("wire-p5-forged-own-sync");
    forged_own_sync["sender_did"] = json!(&fixture.did);
    forged_own_sync["receiver_did"] = json!("did:example:bob-new");
    forged_own_sync["direction"] = json!(1);
    forged_own_sync["content_type"] = json!("text/plain");
    forged_own_sync["content"] = json!("forged own-sync plaintext");
    forged_own_sync["secure"] = json!(true);
    forged_own_sync["decryption_state"] = json!("decrypted");
    let own_sync_calls = Rc::new(RefCell::new(Vec::new()));
    let own_sync_result = MessageReadRuntime::new(
        &client,
        ReadyAnyReadSessionProvider,
        RecordingTransport {
            calls: Rc::clone(&own_sync_calls),
            response: json!({"messages": [forged_own_sync], "has_more": false}),
        },
        NoopDirectoryTransport,
    )
    .history_async(HistoryRead {
        thread: crate::messages::ThreadRef::Group(group),
        query: crate::messages::HistoryQuery {
            limit: crate::ids::PageLimit(20),
            cursor: None,
            inbox_history_options: None,
        },
        resolved_peer_did: None,
        peer_scope: None,
    })
    .await
    .unwrap();
    assert_eq!(own_sync_calls.borrow()[0].method, "group.list_messages");

    assert!(group_result
        .page
        .items
        .iter()
        .chain(&own_sync_result.page.items)
        .all(
            |message| P5_CACHE_METADATA_KEYS.into_iter().all(|key| !message
                .metadata
                .attributes
                .iter()
                .any(|attribute| attribute.key == key))
        ));
    let public_raw = serde_json::to_string(&(&group_result.raw, &own_sync_result.raw)).unwrap();
    for key in P5_CACHE_METADATA_KEYS {
        assert!(!public_raw.contains(key));
    }
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )
    .unwrap();
    let records =
        crate::internal::local_state::messages::list_decrypted_secure_messages_for_owner_identity(
            &connection,
            client.current_identity().id.as_str(),
            &[
                "wire-p5-forged-group".to_owned(),
                "wire-p5-forged-own-sync".to_owned(),
            ],
        )
        .unwrap();
    assert!(records
        .iter()
        .all(|record| p5_cache_binding_from_record(record).is_none()));
    drop(connection);

    let group_replay_calls = Rc::new(RefCell::new(Vec::new()));
    let group_replay = MessageReadRuntime::new(
        &client,
        ReadyAnyReadSessionProvider,
        RecordingTransport {
            calls: Rc::clone(&group_replay_calls),
            response: json!({"messages": [group_wire], "has_more": false}),
        },
        StaticHandleDirectoryTransport,
    )
    .history_async(HistoryRead {
        thread: crate::messages::ThreadRef::Direct(
            crate::ids::PeerRef::parse("did:example:bob-new", "").unwrap(),
        ),
        query: crate::messages::HistoryQuery {
            limit: crate::ids::PageLimit(20),
            cursor: None,
            inbox_history_options: None,
        },
        resolved_peer_did: Some("did:example:bob-new".to_owned()),
        peer_scope: None,
    })
    .await
    .unwrap();
    assert_eq!(group_replay_calls.borrow()[0].method, "direct.get_history");
    assert!(group_replay.page.items.is_empty());

    let own_sync_replay_calls = Rc::new(RefCell::new(Vec::new()));
    let own_sync_replay = MessageReadRuntime::new(
        &client,
        ReadyAnyReadSessionProvider,
        RecordingTransport {
            calls: Rc::clone(&own_sync_replay_calls),
            response: json!({"messages": [own_sync_wire], "has_more": false}),
        },
        StaticHandleDirectoryTransport,
    )
    .history_async(HistoryRead {
        thread: crate::messages::ThreadRef::Direct(
            crate::ids::PeerRef::parse("did:example:bob-new", "").unwrap(),
        ),
        query: crate::messages::HistoryQuery {
            limit: crate::ids::PageLimit(20),
            cursor: None,
            inbox_history_options: None,
        },
        resolved_peer_did: Some("did:example:bob-new".to_owned()),
        peer_scope: None,
    })
    .await
    .unwrap();
    assert_eq!(
        own_sync_replay_calls.borrow()[0].method,
        "direct.get_history"
    );
    assert!(own_sync_replay.page.items.is_empty());
}

#[tokio::test]
async fn cached_p5_projection_sync_revalidates_gate_and_current_endpoint() {
    let fixture = VNextCacheFixture::new();
    let gate_off_client = fixture.client(false);
    let gate_wire = ordinary_p5_cache_message(
        "wire-p5-sync-gate-off",
        "did:example:bob-new",
        "device-bob",
        &fixture.did,
        &fixture.device_id,
    );
    let wrong_device_wire = ordinary_p5_cache_message(
        "wire-p5-sync-wrong-device",
        "did:example:bob-new",
        "device-bob",
        &fixture.did,
        "device-not-current",
    );
    let revoked_wire = ordinary_p5_cache_message(
        "wire-p5-sync-revoked",
        "did:example:bob-new",
        "device-bob",
        &fixture.did,
        &fixture.device_id,
    );
    gate_off_client
        .core_inner()
        .local_state_db()
        .await
        .unwrap()
        .store_messages(vec![
            p5_cached_incoming_record(
                &gate_off_client,
                &gate_wire,
                "msg-logical-sync-gate-off",
                "gate-off cached plaintext",
            ),
            p5_cached_incoming_record(
                &gate_off_client,
                &wrong_device_wire,
                "msg-logical-sync-wrong-device",
                "wrong-device cached plaintext",
            ),
            p5_cached_incoming_record(
                &gate_off_client,
                &revoked_wire,
                "msg-logical-sync-revoked",
                "revoked cached plaintext",
            ),
        ])
        .await
        .unwrap();

    let mut gate_off_raw = json!({"messages": [gate_wire], "has_more": false});
    project_secure_direct_messages(
        &gate_off_client,
        &mut gate_off_raw,
        &mut NoopDirectoryTransport,
    );
    assert!(gate_off_raw["messages"].as_array().unwrap().is_empty());

    let enabled_client = fixture.client(true);
    let mut wrong_device_raw = json!({"messages": [wrong_device_wire], "has_more": false});
    project_secure_direct_messages(
        &enabled_client,
        &mut wrong_device_raw,
        &mut NoopDirectoryTransport,
    );
    assert!(wrong_device_raw["messages"].as_array().unwrap().is_empty());

    fixture.set_authorization_status("revoked");
    let mut revoked_raw = json!({"messages": [revoked_wire], "has_more": false});
    project_secure_direct_messages(
        &enabled_client,
        &mut revoked_raw,
        &mut NoopDirectoryTransport,
    );
    assert!(revoked_raw["messages"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn cached_p5_projection_is_hidden_when_product_gate_is_disabled() {
    let fixture = VNextCacheFixture::new();
    let client = fixture.client(false);
    let wire = ordinary_p5_cache_message(
        "wire-p5-gate-off",
        "did:example:bob-new",
        "device-bob",
        &fixture.did,
        &fixture.device_id,
    );
    client
        .core_inner()
        .local_state_db()
        .await
        .unwrap()
        .store_messages(vec![p5_cached_incoming_record(
            &client,
            &wire,
            "msg-logical-gate-off",
            "gate-off cached plaintext",
        )])
        .await
        .unwrap();
    let mut raw = json!({"messages": [wire], "has_more": false});

    project_secure_direct_messages_async(&client, &mut raw, &mut NoopDirectoryTransport).await;

    assert!(raw["messages"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn cached_p5_projection_revalidates_current_device_and_authorization() {
    let fixture = VNextCacheFixture::new();
    let client = fixture.client(true);
    let wrong_device_wire = ordinary_p5_cache_message(
        "wire-p5-wrong-local-device",
        "did:example:bob-new",
        "device-bob",
        &fixture.did,
        "device-not-current",
    );
    let revoked_wire = ordinary_p5_cache_message(
        "wire-p5-revoked-local-device",
        "did:example:bob-new",
        "device-bob",
        &fixture.did,
        &fixture.device_id,
    );
    let unauthorized_wire = ordinary_p5_cache_message(
        "wire-p5-unauthorized-local-device",
        "did:example:bob-new",
        "device-bob",
        &fixture.did,
        &fixture.device_id,
    );
    client
        .core_inner()
        .local_state_db()
        .await
        .unwrap()
        .store_messages(vec![
            p5_cached_incoming_record(
                &client,
                &wrong_device_wire,
                "msg-logical-wrong-local-device",
                "wrong-device cached plaintext",
            ),
            p5_cached_incoming_record(
                &client,
                &revoked_wire,
                "msg-logical-revoked-local-device",
                "revoked cached plaintext",
            ),
            p5_cached_incoming_record(
                &client,
                &unauthorized_wire,
                "msg-logical-unauthorized-local-device",
                "unauthorized cached plaintext",
            ),
        ])
        .await
        .unwrap();

    let mut wrong_device_raw = json!({"messages": [wrong_device_wire], "has_more": false});
    project_secure_direct_messages_async(
        &client,
        &mut wrong_device_raw,
        &mut NoopDirectoryTransport,
    )
    .await;
    assert!(wrong_device_raw["messages"].as_array().unwrap().is_empty());

    fixture.set_authorization_status("revoked");
    let mut revoked_raw = json!({"messages": [revoked_wire], "has_more": false});
    project_secure_direct_messages_async(&client, &mut revoked_raw, &mut NoopDirectoryTransport)
        .await;
    assert!(revoked_raw["messages"].as_array().unwrap().is_empty());

    fixture.set_authorization_status("active");
    fixture.remove_direct_profile_from_local_document();
    let mut unauthorized_raw = json!({"messages": [unauthorized_wire], "has_more": false});
    project_secure_direct_messages_async(
        &client,
        &mut unauthorized_raw,
        &mut NoopDirectoryTransport,
    )
    .await;
    assert!(unauthorized_raw["messages"].as_array().unwrap().is_empty());
}

#[test]
fn p5_cache_rejects_tampered_cross_profile_and_ambiguous_wire() {
    let wire = ordinary_p5_cache_message(
        "wire-p5-binding",
        "did:example:bob-new",
        "device-bob",
        "did:example:alice",
        "device-alice",
    );
    let record = crate::internal::local_state::messages::MessageRecord {
        msg_id: "msg-logical-binding".to_owned(),
        owner_identity_id: "alice-id".to_owned(),
        owner_did: "did:example:alice".to_owned(),
        conversation_id: "dm:did:example:bob-new".to_owned(),
        thread_id: "dm:did:example:bob-new".to_owned(),
        wire_thread_kind: "direct".to_owned(),
        wire_thread_ref: "did:example:bob-new".to_owned(),
        wire_identity_resolution_state: "resolved".to_owned(),
        direction: 0,
        sender_did: "did:example:bob-new".to_owned(),
        receiver_did: "did:example:alice".to_owned(),
        content_type: "text/plain".to_owned(),
        content: "authenticated cached plaintext".to_owned(),
        is_e2ee: true,
        metadata: p5_cache_record_metadata(&wire),
        ..crate::internal::local_state::messages::MessageRecord::default()
    };
    let rejects = |mut candidate: Value, records: Vec<_>| {
        let applied = apply_cached_secure_direct_records(
            std::slice::from_mut(&mut candidate),
            records,
            Some(&HashSet::from([0])),
        );
        assert!(applied.is_empty());
        assert_ne!(candidate["id"], "msg-logical-binding");
        assert_ne!(candidate["content"], "authenticated cached plaintext");
    };

    let mut malformed = wire.clone();
    malformed.as_object_mut().unwrap().remove("body");
    rejects(malformed, vec![record.clone()]);

    let mut cross_profile = wire.clone();
    cross_profile["meta"]["profile"] = json!("anp.direct.e2ee.v1");
    rejects(cross_profile, vec![record.clone()]);

    let mut sender_device_tamper = wire.clone();
    sender_device_tamper["meta"]["sender_device_id"] = json!("device-mallory");
    rejects(sender_device_tamper, vec![record.clone()]);

    let mut recipient_device_tamper = wire.clone();
    recipient_device_tamper["meta"]["recipient_device_id"] = json!("device-other");
    rejects(recipient_device_tamper, vec![record.clone()]);

    let mut ciphertext_tamper = wire.clone();
    ciphertext_tamper["body"]["ciphertext_b64u"] = json!("QU5PVEhFUi1WQUxJRC1DSVBIRVJURVhU");
    rejects(ciphertext_tamper, vec![record.clone()]);

    let mut session_tamper = wire.clone();
    session_tamper["body"]["session_id"] = json!("AQEBAQEBAQEBAQEBAQEBAQ");
    rejects(session_tamper, vec![record.clone()]);

    let mut ratchet_header_tamper = wire.clone();
    ratchet_header_tamper["body"]["ratchet_header"]["n"] = json!("1");
    rejects(ratchet_header_tamper, vec![record.clone()]);

    let mut suite_tamper = wire.clone();
    suite_tamper["body"]
        .as_object_mut()
        .unwrap()
        .remove("suite");
    rejects(suite_tamper, vec![record.clone()]);

    let mut message_id_tamper = wire.clone();
    message_id_tamper["meta"]["message_id"] = json!("wire-p5-other");
    message_id_tamper["meta"]["operation_id"] = json!("wire-p5-other");
    rejects(message_id_tamper, vec![record.clone()]);

    let mut legacy_record = record.clone();
    let mut legacy_metadata: Value = serde_json::from_str(&legacy_record.metadata).unwrap();
    legacy_metadata
        .as_object_mut()
        .unwrap()
        .remove(P5_CACHE_BINDING_DIGEST_KEY);
    legacy_record.metadata = legacy_metadata.to_string();
    rejects(wire.clone(), vec![legacy_record]);

    let mut duplicate = record.clone();
    duplicate.msg_id = "msg-logical-duplicate".to_owned();
    rejects(wire.clone(), vec![record.clone(), duplicate]);

    let mut outer_tamper = wire;
    outer_tamper["id"] = json!("forged-outer-id");
    outer_tamper["sender_did"] = json!("did:example:mallory");
    outer_tamper["receiver_did"] = json!("did:example:mallory");
    let applied = apply_cached_secure_direct_records(
        std::slice::from_mut(&mut outer_tamper),
        vec![record],
        Some(&HashSet::from([0])),
    );
    assert_eq!(applied.len(), 1);
    assert_eq!(outer_tamper["id"], "msg-logical-binding");
    assert_eq!(
        outer_tamper["raw_message_id"], "wire-p5-binding",
        "only authenticated inner meta.message_id is the cache index"
    );

    let init_wire = ordinary_p5_cache_init_message(
        "wire-p5-init-binding",
        "did:example:bob-new",
        "device-bob",
        "did:example:alice",
        "device-alice",
    );
    let init_record = crate::internal::local_state::messages::MessageRecord {
        msg_id: "msg-logical-init-binding".to_owned(),
        owner_identity_id: "alice-id".to_owned(),
        owner_did: "did:example:alice".to_owned(),
        conversation_id: "dm:did:example:bob-new".to_owned(),
        thread_id: "dm:did:example:bob-new".to_owned(),
        wire_thread_kind: "direct".to_owned(),
        wire_thread_ref: "did:example:bob-new".to_owned(),
        wire_identity_resolution_state: "resolved".to_owned(),
        direction: 0,
        sender_did: "did:example:bob-new".to_owned(),
        receiver_did: "did:example:alice".to_owned(),
        content_type: "text/plain".to_owned(),
        content: "authenticated init plaintext".to_owned(),
        is_e2ee: true,
        metadata: p5_cache_record_metadata(&init_wire),
        ..crate::internal::local_state::messages::MessageRecord::default()
    };
    let mut ephemeral_tamper = init_wire;
    ephemeral_tamper["body"]["sender_ephemeral_pub_b64u"] =
        json!("AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE");
    rejects(ephemeral_tamper, vec![init_record]);
}

#[test]
fn p5_cache_binding_is_canonical_and_excludes_advisory_metadata() {
    let wire = ordinary_p5_cache_message(
        "wire-p5-canonical",
        "did:example:bob-new",
        "device-bob",
        "did:example:alice",
        "device-alice",
    );
    let baseline = p5_cache_binding_from_message(&wire).unwrap().unwrap();

    let mut reordered = wire.clone();
    reordered["body"] = json!({
        "ciphertext_b64u": "U0VTU0lPTi1DT05UUk9MLUNJUEhFUlRFWFQ",
        "ratchet_header": {
            "n": "0",
            "pn": "0",
            "dh_pub_b64u": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
        },
        "suite": anp::direct_e2ee::MTI_DIRECT_E2EE_SUITE_V2,
        "session_id": "AAAAAAAAAAAAAAAAAAAAAA"
    });
    assert_eq!(
        p5_cache_binding_from_message(&reordered).unwrap().unwrap(),
        baseline,
        "JSON member order must not affect the binding digest"
    );

    let mut advisory_change = wire;
    advisory_change["meta"]["anp_version"] = json!("9.9");
    advisory_change["meta"]["created_at"] = json!("2026-07-21T01:02:03Z");
    assert_eq!(
        p5_cache_binding_from_message(&advisory_change)
            .unwrap()
            .unwrap(),
        baseline,
        "non-AAD advisory metadata must not affect cache identity"
    );
}

#[test]
fn p5_cache_metadata_requires_authenticated_outcome_and_persists_same_wire_id() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let message_id = "msg-p5-same-wire-and-logical";
    let wire = ordinary_p5_cache_message(
        message_id,
        "did:example:bob-new",
        "device-bob",
        "did:example:alice",
        "device-alice",
    );
    let mut untrusted = wire.clone();
    untrusted["decryption_state"] = json!("decrypted");
    untrusted["secure"] = json!(true);
    untrusted["raw_message_id"] = json!(message_id);
    clear_untrusted_p5_projection_state(std::slice::from_mut(&mut untrusted));
    let untrusted_attributes = metadata_attributes_from_object(
        untrusted.as_object().unwrap(),
        message_id,
        Some("text/plain"),
    );
    assert!(P5_CACHE_METADATA_KEYS.into_iter().all(|key| {
        !untrusted_attributes
            .iter()
            .any(|attribute| attribute.key == key)
    }));

    let mut authenticated = wire;
    apply_p5_v2_product_outcome(
        &mut authenticated,
        crate::internal::secure_direct::v2_product::V2InboundProductOutcome::Business(
            crate::internal::secure_direct::v2_product::V2InboundBusinessProjection {
                logical_message_id: message_id.to_owned(),
                conversation_id: None,
                sender_did: "did:example:bob-new".to_owned(),
                sender_device_id: "device-bob".to_owned(),
                recipient_did: "did:example:alice".to_owned(),
                wire_message_id: message_id.to_owned(),
                body: crate::internal::secure_direct::v2_product::V2InboundBusinessBody::Text {
                    text: "same id plaintext".to_owned(),
                    markdown: false,
                },
                session_reply_pending: false,
            },
        ),
    );
    let raw = json!({"messages": [authenticated], "has_more": false});
    assert!(raw["messages"][0]
        .get("_awiki_p5_cache_authenticated")
        .is_none());
    let page = page_from_raw(&client, &raw, crate::ids::PageLimit(20)).unwrap();
    assert!(P5_CACHE_METADATA_KEYS.into_iter().all(|key| !page.items[0]
        .metadata
        .attributes
        .iter()
        .any(|attribute| attribute.key == key)));
    assert!(P5_CACHE_METADATA_KEYS
        .into_iter()
        .all(|key| raw["messages"][0].get(key).is_none()));
    let mut p5_provenance = DirectP5ProjectionProvenance::default();
    p5_provenance.record(
        message_id,
        p5_cache_binding_from_message(&raw["messages"][0])
            .unwrap()
            .unwrap(),
    );
    let records = remote_projection_records(&client, &page.items, &p5_provenance).unwrap();
    assert_eq!(records.len(), 1);
    let metadata: Value = serde_json::from_str(&records[0].metadata).unwrap();
    assert_eq!(metadata["raw_message_id"], message_id);
    for key in P5_CACHE_METADATA_KEYS {
        assert!(
            metadata.get(key).is_some(),
            "missing internal cache key {key}"
        );
    }
    for secret_key in [
        "plaintext",
        "content",
        "ciphertext",
        "ciphertext_b64u",
        "aad",
    ] {
        assert!(metadata.get(secret_key).is_none());
    }
    assert!(!records[0]
        .metadata
        .contains("U0VTU0lPTi1DT05UUk9MLUNJUEhFUlRFWFQ"));
    assert!(!records[0].metadata.contains("same id plaintext"));
}

#[tokio::test]
async fn verified_handle_projection_rejects_missing_and_conflicting_authority() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let mut first = FixedLookupDirectoryTransport(json!({
        "handle": "bob",
        "full_handle": "bob.anpclaw.com",
        "did": "did:example:bob-new",
        "domain": "anpclaw.com",
        "status": "active",
        "user_id": "user-bob"
    }));
    assert!(matches!(
        lookup_direct_peer_scope_async(&client, &mut first, "did:example:bob-new").await,
        VerifiedHandleScopeLookup::Verified(_)
    ));

    let mut missing = FixedLookupDirectoryTransport(json!({
        "handle": "bob",
        "full_handle": "bob.anpclaw.com",
        "did": "did:example:bob-missing-authority",
        "domain": "anpclaw.com",
        "status": "active"
    }));
    assert!(matches!(
        lookup_direct_peer_scope_async(&client, &mut missing, "did:example:bob-missing-authority")
            .await,
        VerifiedHandleScopeLookup::Rejected
    ));

    let mismatched_response = json!({
        "handle": "mallory",
        "full_handle": "mallory.anpclaw.com",
        "did": "did:example:mallory-response",
        "domain": "anpclaw.com",
        "status": "active",
        "user_id": "user-mallory-response"
    });
    let mut mismatched_async = FixedLookupDirectoryTransport(mismatched_response.clone());
    assert!(matches!(
        lookup_direct_peer_scope_async(&client, &mut mismatched_async, "did:example:bob-requested")
            .await,
        VerifiedHandleScopeLookup::Rejected
    ));
    let mut mismatched_sync = FixedLookupDirectoryTransport(mismatched_response);
    assert!(matches!(
        lookup_direct_peer_scope(&client, &mut mismatched_sync, "did:example:bob-requested"),
        VerifiedHandleScopeLookup::Rejected
    ));

    let connection = crate::internal::local_state::open_writable(&fixture.sqlite_path()).unwrap();
    assert!(crate::internal::local_state::peer_personas::resolve_by_did(
        &connection,
        "alice-id",
        "did:example:bob-requested"
    )
    .unwrap()
    .is_none());
    assert!(crate::internal::local_state::peer_personas::resolve_by_did(
        &connection,
        "alice-id",
        "did:example:mallory-response"
    )
    .unwrap()
    .is_none());
    let before = crate::internal::local_state::peer_personas::resolve_by_did(
        &connection,
        "alice-id",
        "did:example:bob-new",
    )
    .unwrap()
    .unwrap();
    drop(connection);
    let mut conflicting = FixedLookupDirectoryTransport(json!({
        "handle": "mallory",
        "full_handle": "mallory.anpclaw.com",
        "did": "did:example:bob-new",
        "domain": "anpclaw.com",
        "status": "active",
        "user_id": "user-mallory"
    }));
    assert!(matches!(
        lookup_direct_peer_scope_async(&client, &mut conflicting, "did:example:bob-new").await,
        VerifiedHandleScopeLookup::Rejected
    ));
    let connection = crate::internal::local_state::open_writable(&fixture.sqlite_path()).unwrap();
    let after = crate::internal::local_state::peer_personas::resolve_by_did(
        &connection,
        "alice-id",
        "did:example:bob-new",
    )
    .unwrap()
    .unwrap();
    assert_eq!(after, before);

    assert!(matches!(
        lookup_direct_peer_scope_async(
            &client,
            &mut NoopDirectoryTransport,
            "did:example:no-authority"
        )
        .await,
        VerifiedHandleScopeLookup::Unavailable
    ));

    let mut fallback = ProfileFallbackDirectoryTransport::legacy(json!({
        "user_id": "legacy-user",
        "full_handle": "legacy.anpclaw.com"
    }));
    let fallback_scope =
        resolve_direct_peer_scope_async(&client, &mut fallback, "did:example:legacy-profile")
            .await
            .unwrap();
    assert_eq!(fallback_scope.user_id, "legacy-user");
    assert_eq!(fallback_scope.full_handle, "legacy.anpclaw.com");

    let rejected_calls = Rc::new(RefCell::new(Vec::new()));
    let mut rejected_fallback = ProfileFallbackDirectoryTransport {
        lookup_result: Ok(json!({
            "did": "did:example:malformed-authority",
            "full_handle": "malformed.anpclaw.com"
        })),
        profile_response: json!({
            "user_id": "must-not-be-used",
            "full_handle": "must-not-be-used.anpclaw.com"
        }),
        calls: Rc::clone(&rejected_calls),
    };
    assert!(
        resolve_direct_peer_scope_async(
            &client,
            &mut rejected_fallback,
            "did:example:malformed-authority"
        )
        .await
        .is_none(),
        "a malformed authority response must not downgrade to Profile fallback"
    );
    assert_eq!(rejected_calls.borrow().as_slice(), ["lookup"]);
}

#[tokio::test]
async fn profile_fallback_is_limited_and_rejects_every_conflicting_did_claim() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let requested = "did:example:legacy-peer";

    let mut method_absent = ProfileFallbackDirectoryTransport::with_lookup_error(
        crate::ImError::Service {
            status_code: None,
            code: Some("-32601".to_owned()),
            message: "method absent".to_owned(),
            data: None,
        },
        json!({
            "profile": {
                "subject_did": requested,
                "user_id": "legacy-user",
                "full_handle": "legacy.anpclaw.com"
            }
        }),
    );
    assert!(
        resolve_direct_peer_scope_async(&client, &mut method_absent, requested)
            .await
            .is_some()
    );
    assert_eq!(method_absent.calls.borrow().len(), 2);
    let mut method_absent_sync = ProfileFallbackDirectoryTransport::with_lookup_error(
        crate::ImError::Service {
            status_code: None,
            code: Some("method_not_found".to_owned()),
            message: "method absent".to_owned(),
            data: None,
        },
        json!({
            "profile": {
                "id": requested,
                "user_id": "legacy-user",
                "full_handle": "legacy.anpclaw.com"
            }
        }),
    );
    assert!(resolve_direct_peer_scope(&client, &mut method_absent_sync, requested).is_some());

    for error in [
        crate::ImError::PermissionDenied,
        crate::ImError::Service {
            status_code: Some(503),
            code: Some("directory_unavailable".to_owned()),
            message: "temporary failure".to_owned(),
            data: None,
        },
        crate::ImError::Service {
            status_code: Some(403),
            code: Some("not_found".to_owned()),
            message: "forbidden".to_owned(),
            data: None,
        },
        crate::ImError::Service {
            status_code: Some(503),
            code: Some("-32601".to_owned()),
            message: "upstream unavailable".to_owned(),
            data: None,
        },
    ] {
        let mut rejected = ProfileFallbackDirectoryTransport::with_lookup_error(
            error.clone(),
            json!({
                "user_id": "must-not-be-used",
                "full_handle": "must-not-be-used.anpclaw.com"
            }),
        );
        assert!(
            resolve_direct_peer_scope_async(&client, &mut rejected, requested)
                .await
                .is_none()
        );
        assert_eq!(rejected.calls.borrow().as_slice(), ["lookup"]);
        let mut rejected_sync = ProfileFallbackDirectoryTransport::with_lookup_error(
            error,
            json!({
                "user_id": "must-not-be-used",
                "full_handle": "must-not-be-used.anpclaw.com"
            }),
        );
        assert!(resolve_direct_peer_scope(&client, &mut rejected_sync, requested).is_none());
        assert_eq!(rejected_sync.calls.borrow().as_slice(), ["lookup"]);
    }

    let conflicting_profile = json!({
        "did": requested,
        "profile": {
            "id": requested,
            "subject_did": "did:example:other-peer",
            "user_id": "legacy-user",
            "full_handle": "legacy.anpclaw.com"
        },
        "result": {
            "subjectDid": requested,
            "profile": {"subject": {"id": requested}}
        }
    });
    let mut async_conflict = ProfileFallbackDirectoryTransport::legacy(conflicting_profile.clone());
    assert!(
        resolve_direct_peer_scope_async(&client, &mut async_conflict, requested)
            .await
            .is_none(),
        "the resolver must inspect every DID claim, not only the first one"
    );
    let mut sync_conflict = ProfileFallbackDirectoryTransport::legacy(conflicting_profile);
    assert!(resolve_direct_peer_scope(&client, &mut sync_conflict, requested).is_none());

    let profile_subject_mismatch = json!({
        "profile": {
            "subject_did": "did:example:other-peer",
            "user_id": "legacy-user",
            "full_handle": "legacy.anpclaw.com"
        }
    });
    let mut async_mismatch =
        ProfileFallbackDirectoryTransport::legacy(profile_subject_mismatch.clone());
    assert!(
        resolve_direct_peer_scope_async(&client, &mut async_mismatch, requested)
            .await
            .is_none()
    );
    let mut sync_mismatch = ProfileFallbackDirectoryTransport::legacy(profile_subject_mismatch);
    assert!(resolve_direct_peer_scope(&client, &mut sync_mismatch, requested).is_none());

    let all_aliases_match = json!({
        "id": requested,
        "profile": {
            "id": requested,
            "subject": {"id": requested},
            "user_id": "legacy-user",
            "full_handle": "legacy.anpclaw.com"
        },
        "result": {
            "id": requested,
            "profile": {
                "id": requested,
                "subject": {"id": requested}
            }
        }
    });
    let mut aliases_async = ProfileFallbackDirectoryTransport::legacy(all_aliases_match.clone());
    assert!(
        resolve_direct_peer_scope_async(&client, &mut aliases_async, requested)
            .await
            .is_some()
    );
    let mut aliases_sync = ProfileFallbackDirectoryTransport::legacy(all_aliases_match);
    assert!(resolve_direct_peer_scope(&client, &mut aliases_sync, requested).is_some());

    let mut nested_id_conflict = ProfileFallbackDirectoryTransport::legacy(json!({
        "id": requested,
        "profile": {
            "user_id": "legacy-user",
            "full_handle": "legacy.anpclaw.com"
        },
        "result": {
            "profile": {"subject": {"id": "did:example:other-peer"}}
        }
    }));
    assert!(
        resolve_direct_peer_scope_async(&client, &mut nested_id_conflict, requested)
            .await
            .is_none()
    );
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
struct DelegatedProofOnlySessionProvider;

impl SessionProvider for DelegatedProofOnlySessionProvider {
    fn ensure_session(
        &self,
        _scope: crate::auth::AuthScope,
    ) -> crate::ImResult<crate::auth::SessionBundle> {
        panic!("delegated inbox reads must use their DID proof without an access-token session")
    }

    fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
        panic!("delegated inbox reads must not refresh an access-token session")
    }

    fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
        panic!("delegated inbox reads must not inspect an access-token session")
    }
}

impl crate::internal::auth::session::AsyncSessionProvider for DelegatedProofOnlySessionProvider {
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

struct ScriptedAuthenticatedTransport {
    calls: Rc<RefCell<Vec<RecordedCall>>>,
    authentication_reloads: Rc<RefCell<usize>>,
    responses: VecDeque<Value>,
}

struct AllInboxRecordingTransport {
    calls: Rc<RefCell<Vec<RecordedCall>>>,
    direct_response: Value,
    group_list_response: Value,
    group_messages_response: Value,
}

impl AuthenticatedRpcTransport for AllInboxRecordingTransport {
    fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value> {
        self.calls.borrow_mut().push(RecordedCall {
            endpoint: endpoint.to_owned(),
            method: method.to_owned(),
            params,
        });
        match method {
            "inbox.get" => Ok(self.direct_response.clone()),
            "group.list" => Ok(self.group_list_response.clone()),
            "group.list_messages" => Ok(self.group_messages_response.clone()),
            _ => Err(crate::ImError::unsupported("all-inbox-test-rpc")),
        }
    }
}

impl AsyncAuthenticatedRpcTransport for AllInboxRecordingTransport {
    async fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value> {
        AuthenticatedRpcTransport::authenticated_rpc(self, endpoint, method, params)
    }
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

impl AsyncAuthenticatedRpcTransport for ScriptedAuthenticatedTransport {
    async fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value> {
        self.calls.borrow_mut().push(RecordedCall {
            endpoint: endpoint.to_owned(),
            method: method.to_owned(),
            params,
        });
        self.responses
            .pop_front()
            .ok_or_else(|| crate::ImError::Internal {
                message: "scripted authenticated response exhausted".to_owned(),
            })
    }

    fn reload_authentication_state(&mut self) -> crate::ImResult<()> {
        *self.authentication_reloads.borrow_mut() += 1;
        Ok(())
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

struct CountingHandleDirectoryTransport {
    calls: Rc<RefCell<Vec<String>>>,
}

impl RpcTransport for CountingHandleDirectoryTransport {
    fn rpc(&mut self, _endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        self.calls.borrow_mut().push(method.to_owned());
        let did = params
            .get("did")
            .and_then(Value::as_str)
            .unwrap_or("did:example:bob-new");
        Ok(json!({
            "handle": "bob",
            "full_handle": "bob.anpclaw.com",
            "did": did,
            "domain": "anpclaw.com",
            "status": "active",
            "user_id": "user-bob"
        }))
    }
}

impl AsyncRpcTransport for CountingHandleDirectoryTransport {
    async fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        RpcTransport::rpc(self, endpoint, method, params)
    }
}

struct FixedLookupDirectoryTransport(Value);

impl RpcTransport for FixedLookupDirectoryTransport {
    fn rpc(&mut self, _endpoint: &str, method: &str, _params: Value) -> crate::ImResult<Value> {
        if method == "lookup" {
            Ok(self.0.clone())
        } else {
            Err(crate::ImError::PeerNotFound {
                peer: "fixed-lookup-directory".to_owned(),
            })
        }
    }
}

impl AsyncRpcTransport for FixedLookupDirectoryTransport {
    async fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        RpcTransport::rpc(self, endpoint, method, params)
    }
}

struct ProfileFallbackDirectoryTransport {
    lookup_result: crate::ImResult<Value>,
    profile_response: Value,
    calls: Rc<RefCell<Vec<String>>>,
}

impl ProfileFallbackDirectoryTransport {
    fn legacy(profile_response: Value) -> Self {
        Self::with_lookup_error(
            crate::ImError::PeerNotFound {
                peer: "profile-fallback".to_owned(),
            },
            profile_response,
        )
    }

    fn with_lookup_error(error: crate::ImError, profile_response: Value) -> Self {
        Self {
            lookup_result: Err(error),
            profile_response,
            calls: Rc::new(RefCell::new(Vec::new())),
        }
    }
}

impl RpcTransport for ProfileFallbackDirectoryTransport {
    fn rpc(&mut self, _endpoint: &str, method: &str, _params: Value) -> crate::ImResult<Value> {
        self.calls.borrow_mut().push(method.to_owned());
        if method == "lookup" {
            return self.lookup_result.clone();
        }
        Ok(self.profile_response.clone())
    }
}

impl AsyncRpcTransport for ProfileFallbackDirectoryTransport {
    async fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        RpcTransport::rpc(self, endpoint, method, params)
    }
}

fn seed_sync_thread_binding_for_test(
    client: &crate::core::ImClient,
    peer_scope: &crate::internal::local_state::owner_scope::DirectPeerScope,
    remote_thread_key: &str,
) {
    let db = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )
    .unwrap();
    crate::internal::local_state::sync_v2::upsert_identity_account_binding(
        &db,
        &crate::internal::local_state::sync_v2::IdentityAccountBinding {
            owner_identity_id: client.current_identity().id.as_str().to_owned(),
            account_id: format!("test-account-{}", client.current_identity().id.as_str()),
            handle_scope: client.handle().map(|handle| handle.as_str().to_owned()),
            current_did: client.did().as_str().to_owned(),
            protocol_device_id: client
                .current_identity()
                .device_id
                .clone()
                .unwrap_or_else(|| "test-device".to_owned()),
            identity_generation: "1".to_owned(),
            device_auth_generation: "1".to_owned(),
            created_at: 1,
            updated_at: 1,
        },
    )
    .unwrap();
    crate::internal::local_state::sync_v2::upsert_sync_thread_binding(
        &db,
        &crate::internal::local_state::sync_v2::SyncThreadBinding {
            owner_identity_id: client.current_identity().id.as_str().to_owned(),
            remote_thread_key: remote_thread_key.to_owned(),
            thread_kind: "direct".to_owned(),
            conversation_id:
                crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(
                    peer_scope,
                ),
            updated_at: 1,
        },
    )
    .unwrap();
}

struct Fixture {
    root: PathBuf,
}

struct VNextCacheFixture {
    root: PathBuf,
    did: String,
    device_id: String,
    identity_dir_name: String,
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

impl VNextCacheFixture {
    const VAULT_SEED: [u8; 32] = [41_u8; 32];
    const WORKSPACE_ID: &'static str = "read-p5-cache-workspace";
    const VAULT_DEVICE_ID: &'static str = "read-p5-cache-vault-device";

    fn new() -> Self {
        use crate::internal::identity_device_state::{
            DeviceAuthorizationProjection, DeviceAuthorizationRole, DeviceAuthorizationStatus,
            IdentityDeviceMode, IdentityDeviceState, IdentityInternalCheckpoint,
            IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
        };
        use crate::internal::identity_store::{
            IdentityStore, SaveIdentityInput, SaveIdentityKeyMode, SaveIdentitySecretStorage,
        };

        let root = unique_temp_root();
        let paths = Self::paths(&root);
        fs::create_dir_all(&paths.identities.identity_root_dir).unwrap();
        let generated =
            crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
                "awiki.test",
                "read-cache",
                None,
                None,
            )
            .unwrap();
        let device_state = IdentityDeviceState {
            schema_version: IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
            mode: IdentityDeviceMode::VNext,
            authorization: Some(DeviceAuthorizationProjection {
                protocol_device_id: generated.protocol_device_id.clone(),
                signing_key_id: generated.device_signing_key_id.clone(),
                e2ee_key_id: generated.device_e2ee_key_id.clone(),
                status: DeviceAuthorizationStatus::Active,
                role: DeviceAuthorizationRole::Member,
                management_ready: false,
                auth_generation: 1,
            }),
            checkpoint: Some(IdentityInternalCheckpoint {
                document_version: 1,
                document_hash: "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned(),
                registry_version: 1,
            }),
        };
        let vault = Arc::new(FileSecretVault::new(
            DeviceVaultRootKey::from_bytes(Self::VAULT_SEED),
            FileSecretVaultStore::new(root.join("vault")),
        ));
        let store = IdentityStore::new(&paths.identities);
        store
            .save_identity_with_secret_storage(
                SaveIdentityInput {
                    local_alias: "alice".to_owned(),
                    did: generated.did.clone(),
                    unique_id: generated.unique_id.clone(),
                    user_id: "read-cache-user".to_owned(),
                    display_name: "Read cache".to_owned(),
                    handle: "read-cache".to_owned(),
                    full_handle: "read-cache.awiki.test".to_owned(),
                    binding_generation: None,
                    jwt_token: "test-device-token".to_owned(),
                    did_document: Some(generated.did_document.clone()),
                    key_mode: SaveIdentityKeyMode::VNext {
                        root_key_id: generated.root_key_id.clone(),
                        device_signing_key_id: generated.device_signing_key_id.clone(),
                        device_e2ee_key_id: generated.device_e2ee_key_id.clone(),
                    },
                    device_state: Some(device_state),
                    key1_private_pem: generated.root_private_pem.clone(),
                    key1_public_pem: generated.root_public_pem.clone(),
                    e2ee_signing_private_pem: generated.device_signing_private_pem.clone(),
                    e2ee_agreement_private_pem: generated.device_e2ee_private_pem.clone(),
                    daemon_subkey_package: Some(generated.daemon_subkey_package.clone()),
                    make_default: true,
                },
                SaveIdentitySecretStorage::Vault {
                    workspace_id: Self::WORKSPACE_ID.to_owned(),
                    device_id: Self::VAULT_DEVICE_ID.to_owned(),
                    vault,
                },
            )
            .unwrap();
        let identity_dir_name = store.load_index().unwrap().credentials["alice"]
            .dir_name
            .clone();
        Self {
            root,
            did: generated.did.as_str().to_owned(),
            device_id: generated.protocol_device_id.as_str().to_owned(),
            identity_dir_name,
        }
    }

    fn paths(root: &std::path::Path) -> crate::ImCorePaths {
        crate::ImCorePaths {
            identities: crate::paths::IdentityRegistryPaths {
                identity_root_dir: root.join("identities"),
                registry_path: root.join("identities").join("registry.json"),
                default_identity_path: Some(root.join("identities").join("default")),
            },
            local_state: crate::paths::LocalStatePaths {
                sqlite_path: root.join("local").join("im.sqlite"),
            },
            runtime: crate::paths::RuntimePaths {
                cache_dir: root.join("cache"),
                temp_dir: root.join("tmp"),
            },
        }
    }

    fn client(&self, enabled: bool) -> crate::core::ImClient {
        crate::core::ImCore::new_with_options(
            crate::ImCoreConfig {
                service_base_url: crate::ServiceEndpoint::parse("https://example.test").unwrap(),
                did_domain: "awiki.test".to_owned(),
                client_version_info: None,
                user_service_endpoint: None,
                message_service_endpoint: None,
                mail_service_endpoint: None,
                anp_service_endpoint: None,
                anp_service_did: None,
                ca_bundle: None,
                transport_policy: crate::MessageTransportPolicy::HttpOnly,
            },
            Self::paths(&self.root),
            crate::ImCoreOpenOptions::default()
                .with_identity_secret_vault(
                    crate::IdentitySecretStoragePolicy::VaultRequired,
                    crate::ImCoreSecretVaultOptions::new(
                        DeviceVaultRootKey::from_bytes(Self::VAULT_SEED),
                        self.root.join("vault"),
                        Self::WORKSPACE_ID,
                        Self::VAULT_DEVICE_ID,
                    ),
                )
                .with_multi_device_direct_e2ee_enabled(enabled),
        )
        .unwrap()
        .client(crate::identity::IdentitySelector::LocalAlias(
            "alice".to_owned(),
        ))
        .unwrap()
    }

    fn set_authorization_status(&self, status: &str) {
        let path = self.root.join("identities").join("registry.json");
        let mut registry: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        registry["credentials"]["alice"]["device_state"]["authorization"]["status"] = json!(status);
        fs::write(path, serde_json::to_vec_pretty(&registry).unwrap()).unwrap();
    }

    fn remove_direct_profile_from_local_document(&self) {
        let path = self
            .root
            .join("identities")
            .join(&self.identity_dir_name)
            .join("did_document.json");
        let mut document: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let devices = document["deviceManifest"]["devices"]
            .as_array_mut()
            .unwrap();
        let device = devices
            .iter_mut()
            .find(|device| device["device_id"] == self.device_id)
            .unwrap();
        device["profiles"]
            .as_array_mut()
            .unwrap()
            .retain(|profile| {
                profile.as_str() != Some(anp::authentication::PROFILE_DIRECT_E2EE_V2)
            });
        fs::write(path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
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
        crate::core::ImCore::new_with_options(
            crate::ImCoreConfig {
                service_base_url: crate::ServiceEndpoint::parse("https://example.test").unwrap(),
                did_domain: "awiki.test".to_string(),
                client_version_info: None,
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
            crate::ImCoreOpenOptions::default(),
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

fn p5_wire_message(init: bool) -> Value {
    let operation_id = "p5-wire-placeholder";
    let content_type = if init {
        anp::direct_e2ee::CONTENT_TYPE_DIRECT_INIT_V2
    } else {
        anp::direct_e2ee::CONTENT_TYPE_DIRECT_CIPHER_V2
    };
    let body = if init {
        json!({
            "session_id": "AAAAAAAAAAAAAAAAAAAAAA",
            "suite": anp::direct_e2ee::MTI_DIRECT_E2EE_SUITE_V2,
            "sender_static_key_agreement_id": "did:example:alice#ka-admin",
            "recipient_bundle_id": "bundle-member",
            "recipient_signed_prekey_id": "signed-member",
            "sender_ephemeral_pub_b64u": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "ciphertext_b64u": "U0VTU0lPTi1DT05UUk9MLUNJUEhFUlRFWFQ"
        })
    } else {
        json!({
            "session_id": "AAAAAAAAAAAAAAAAAAAAAA",
            "suite": anp::direct_e2ee::MTI_DIRECT_E2EE_SUITE_V2,
            "ratchet_header": {
                "dh_pub_b64u": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                "pn": "0",
                "n": "0"
            },
            "ciphertext_b64u": "U0VTU0lPTi1DT05UUk9MLUNJUEhFUlRFWFQ"
        })
    };
    json!({
        "id": operation_id,
        "sender_did": "did:example:alice",
        "receiver_did": "did:example:alice",
        "content_type": content_type,
        "server_seq": if init { 1 } else { 2 },
        "meta": {
            "profile": anp::direct_e2ee::DIRECT_E2EE_PROFILE_V2,
            "security_profile": "direct-e2ee",
            "sender_did": "did:example:alice",
            "sender_device_id": "device-admin",
            "target": {"kind": "agent", "did": "did:example:alice"},
            "recipient_device_id": "device-member",
            "operation_id": operation_id,
            "message_id": operation_id,
            "content_type": content_type
        },
        "body": body,
        "content": body
    })
}

fn ordinary_p5_cache_message(
    message_id: &str,
    sender_did: &str,
    sender_device_id: &str,
    recipient_did: &str,
    recipient_device_id: &str,
) -> Value {
    ordinary_p5_cache_message_with_kind(
        false,
        message_id,
        sender_did,
        sender_device_id,
        recipient_did,
        recipient_device_id,
    )
}

fn ordinary_p5_cache_init_message(
    message_id: &str,
    sender_did: &str,
    sender_device_id: &str,
    recipient_did: &str,
    recipient_device_id: &str,
) -> Value {
    ordinary_p5_cache_message_with_kind(
        true,
        message_id,
        sender_did,
        sender_device_id,
        recipient_did,
        recipient_device_id,
    )
}

fn ordinary_p5_cache_message_with_kind(
    init: bool,
    message_id: &str,
    sender_did: &str,
    sender_device_id: &str,
    recipient_did: &str,
    recipient_device_id: &str,
) -> Value {
    let mut message = p5_wire_message(init);
    let object = message.as_object_mut().unwrap();
    object.insert("id".to_owned(), json!(message_id));
    object.insert("sender_did".to_owned(), json!(sender_did));
    object.insert("receiver_did".to_owned(), json!(recipient_did));
    let meta = object
        .get_mut("meta")
        .and_then(Value::as_object_mut)
        .unwrap();
    meta.insert("sender_did".to_owned(), json!(sender_did));
    meta.insert("sender_device_id".to_owned(), json!(sender_device_id));
    meta.insert(
        "target".to_owned(),
        json!({"kind": "agent", "did": recipient_did}),
    );
    meta.insert("recipient_device_id".to_owned(), json!(recipient_device_id));
    meta.insert("operation_id".to_owned(), json!(message_id));
    meta.insert("message_id".to_owned(), json!(message_id));
    message
}

fn json_rpc_p5_cache_message(mut message: Value) -> Value {
    let object = message.as_object_mut().unwrap();
    let meta = object.remove("meta").unwrap();
    let body = object.remove("body").unwrap();
    object.insert("method".to_owned(), json!("direct.incoming"));
    object.insert("params".to_owned(), json!({"meta": meta, "body": body}));
    message
}

fn p5_cache_record_metadata(message: &Value) -> String {
    let binding = p5_cache_binding_from_message(message).unwrap().unwrap();
    json!({
        "decryption_state": "decrypted",
        "raw_message_id": binding.message_id,
        (P5_CACHE_PROFILE_KEY): binding.profile,
        (P5_CACHE_SENDER_DID_KEY): binding.sender_did,
        (P5_CACHE_SENDER_DEVICE_ID_KEY): binding.sender_device_id,
        (P5_CACHE_RECIPIENT_DID_KEY): binding.recipient_did,
        (P5_CACHE_RECIPIENT_DEVICE_ID_KEY): binding.recipient_device_id,
        (P5_CACHE_BINDING_DIGEST_KEY): binding.digest,
    })
    .to_string()
}

fn p5_cached_incoming_record(
    client: &crate::core::ImClient,
    wire: &Value,
    logical_message_id: &str,
    plaintext: &str,
) -> crate::internal::local_state::messages::MessageRecord {
    let binding = p5_cache_binding_from_message(wire).unwrap().unwrap();
    crate::internal::local_state::messages::MessageRecord {
        msg_id: logical_message_id.to_owned(),
        owner_identity_id: client.current_identity().id.as_str().to_owned(),
        owner_did: client.did().as_str().to_owned(),
        conversation_id: format!("dm:{}", binding.sender_did),
        thread_id: format!("dm:{}", binding.sender_did),
        wire_thread_kind: "direct".to_owned(),
        wire_thread_ref: binding.sender_did.clone(),
        wire_identity_resolution_state: "resolved".to_owned(),
        direction: 0,
        sender_did: binding.sender_did,
        receiver_did: binding.recipient_did,
        content_type: "text/plain".to_owned(),
        content: plaintext.to_owned(),
        is_e2ee: true,
        metadata: p5_cache_record_metadata(wire),
        ..crate::internal::local_state::messages::MessageRecord::default()
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
