use serde_json::Value;

use crate::internal::auth::session::SessionProvider;
use crate::internal::transport::AuthenticatedRpcTransport;

pub(crate) const MESSAGE_RPC_ENDPOINT: &str = "/im/rpc";

pub(crate) struct MessageReadRuntime<'a, P, T> {
    client: &'a crate::core::ImClient,
    session_provider: P,
    transport: T,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InboxRead {
    pub query: crate::messages::InboxQuery,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HistoryRead {
    pub thread: crate::messages::ThreadRef,
    pub query: crate::messages::HistoryQuery,
    pub resolved_peer_did: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReadPageResult {
    pub page: crate::ids::Page<crate::messages::Message>,
    pub raw: Value,
}

impl<'a, P, T> MessageReadRuntime<'a, P, T>
where
    P: SessionProvider,
    T: AuthenticatedRpcTransport,
{
    pub(crate) fn new(
        client: &'a crate::core::ImClient,
        session_provider: P,
        transport: T,
    ) -> Self {
        Self {
            client,
            session_provider,
            transport,
        }
    }

    pub(crate) fn inbox(mut self, input: InboxRead) -> crate::ImResult<ReadPageResult> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::Messaging)?;
        let limit = page_limit(input.query.limit, 20);
        let params = crate::internal::wire::inbox::build_inbox_rpc_params(
            &crate::internal::wire::common::WireIdentity {
                did: self.client.did().as_str().to_string(),
            },
            crate::internal::wire::inbox::InboxWireRequest { limit },
        );
        let raw = self
            .transport
            .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "inbox.get", params)?;
        let page = page_from_raw(&raw, input.query.limit)?;
        Ok(ReadPageResult { page, raw })
    }

    pub(crate) fn history(mut self, input: HistoryRead) -> crate::ImResult<ReadPageResult> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::Messaging)?;
        let peer = direct_thread(input.thread, input.resolved_peer_did)?;
        let params = crate::internal::wire::history::build_history_rpc_params(
            &crate::internal::wire::common::WireIdentity {
                did: self.client.did().as_str().to_string(),
            },
            crate::internal::wire::history::HistoryWireRequest {
                peer_did: peer.resolved_did.clone(),
                limit: page_limit(input.query.limit, 50),
                cursor: input.query.cursor.map(|cursor| cursor.as_str().to_string()),
                skip: 0,
            },
        )?;
        let raw =
            self.transport
                .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "direct.get_history", params)?;
        let page = page_from_raw(&raw, input.query.limit)?;
        Ok(ReadPageResult { page, raw })
    }
}

struct DirectThread {
    resolved_did: String,
}

fn direct_thread(
    thread: crate::messages::ThreadRef,
    resolved_peer_did: Option<String>,
) -> crate::ImResult<DirectThread> {
    let crate::messages::ThreadRef::Direct(peer) = thread else {
        return Err(crate::ImError::unsupported("group-history"));
    };
    let resolved = resolved_peer_did
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| peer.as_str().trim());
    if !resolved.starts_with("did:") {
        return Err(crate::ImError::PeerNotFound {
            peer: peer.as_str().to_string(),
        });
    }
    Ok(DirectThread {
        resolved_did: resolved.to_string(),
    })
}

fn page_limit(limit: crate::ids::PageLimit, fallback: i64) -> i64 {
    if limit.0 == 0 {
        fallback
    } else {
        i64::from(limit.0)
    }
}

fn page_from_raw(
    raw: &Value,
    requested_limit: crate::ids::PageLimit,
) -> crate::ImResult<crate::ids::Page<crate::messages::Message>> {
    let messages = raw
        .get("messages")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| message_from_value(item).transpose())
                .collect::<crate::ImResult<Vec<_>>>()
        })
        .transpose()?
        .unwrap_or_default();
    let limit = usize::try_from(requested_limit.0).unwrap_or_default();
    let has_more = raw
        .get("has_more")
        .and_then(Value::as_bool)
        .unwrap_or(limit > 0 && messages.len() >= limit);
    let next_cursor = raw
        .get("next_cursor")
        .or_else(|| raw.get("next_since_seq"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(crate::ids::Cursor::parse)
        .transpose()?;
    Ok(crate::ids::Page {
        items: messages,
        next_cursor,
        has_more,
    })
}

fn message_from_value(value: &Value) -> crate::ImResult<Option<crate::messages::Message>> {
    let Some(object) = value.as_object() else {
        return Ok(None);
    };
    let id = message_identity(value);
    if id.trim().is_empty() {
        return Ok(None);
    }
    let sender_did = string_value(object.get("sender_did"));
    let receiver_did = string_value(object.get("receiver_did"));
    let group_did = string_value(object.get("group_did"));
    let retry_target = if group_did.trim().is_empty() {
        Some(crate::internal::message_runtime::state::MessageRetryTarget::DirectText)
    } else {
        Some(crate::internal::message_runtime::state::MessageRetryTarget::GroupText)
    };
    let metadata = message_metadata_from_object(object, &id, retry_target);
    let thread = if !group_did.trim().is_empty() {
        crate::messages::ThreadRef::Group(crate::ids::GroupRef::parse(&group_did)?)
    } else {
        let peer = if !receiver_did.trim().is_empty() {
            receiver_did.as_str()
        } else {
            sender_did.as_str()
        };
        crate::messages::ThreadRef::Direct(crate::ids::PeerRef::parse(peer, "")?)
    };
    Ok(Some(crate::messages::Message {
        id: crate::ids::MessageId::parse(id)?,
        thread,
        direction: message_direction(value),
        sender: crate::ids::PeerRef::parse(non_empty_or(&sender_did, "did:unknown:sender"), "")?,
        receiver: (!receiver_did.trim().is_empty())
            .then(|| crate::ids::PeerRef::parse(&receiver_did, ""))
            .transpose()?,
        group: (!group_did.trim().is_empty())
            .then(|| crate::ids::GroupRef::parse(&group_did))
            .transpose()?,
        body: message_body(value),
        sent_at: Some(string_value(object.get("sent_at"))).filter(|value| !value.trim().is_empty()),
        received_at: Some(string_value(object.get("received_at")))
            .filter(|value| !value.trim().is_empty()),
        metadata,
    }))
}

fn message_metadata_from_object(
    object: &serde_json::Map<String, Value>,
    message_id: &str,
    retry_target: Option<crate::internal::message_runtime::state::MessageRetryTarget>,
) -> crate::messages::MessageMetadata {
    let metadata_json = metadata_projection_json(object, message_id);
    let send_state = crate::internal::message_runtime::state::send_state_from_metadata(
        &metadata_json,
        message_id,
    );
    let retry_plan = crate::internal::message_runtime::state::retry_plan_from_metadata(
        &metadata_json,
        send_state.as_ref(),
        retry_target,
    );
    crate::messages::MessageMetadata {
        operation_id: Some(string_value(object.get("operation_id")))
            .filter(|value| !value.trim().is_empty()),
        delivery_state: Some(string_value(object.get("delivery_state")))
            .filter(|value| !value.trim().is_empty()),
        send_state,
        retry_plan,
        server_sequence: i64_value(object.get("server_seq"))
            .or_else(|| i64_value(object.get("sequence")))
            .or_else(|| i64_value(object.get("group_event_seq"))),
        content_type: Some(string_value(object.get("content_type")))
            .filter(|value| !value.trim().is_empty()),
        attributes: Vec::new(),
    }
}

fn metadata_projection_json(object: &serde_json::Map<String, Value>, message_id: &str) -> String {
    let mut metadata = serde_json::Map::new();
    metadata.insert(
        "message_id".to_string(),
        Value::String(message_id.to_string()),
    );
    for key in [
        "operation_id",
        "delivery_state",
        "failure_reason",
        "send_state_updated_at",
        "accepted_at",
        "send_state",
        "retry_plan",
    ] {
        if let Some(value) = object.get(key) {
            metadata.insert(key.to_string(), value.clone());
        }
    }
    Value::Object(metadata).to_string()
}

fn message_identity(message: &Value) -> String {
    message
        .as_object()
        .and_then(|object| {
            object
                .get("id")
                .or_else(|| object.get("message_id"))
                .or_else(|| object.get("msg_id"))
                .or_else(|| object.get("client_msg_id"))
        })
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn message_direction(value: &Value) -> crate::messages::MessageDirection {
    let direction = value.get("direction").and_then(Value::as_i64).or_else(|| {
        value
            .get("direction")
            .and_then(Value::as_str)
            .and_then(|value| value.parse().ok())
    });
    match direction {
        Some(1) => crate::messages::MessageDirection::Outgoing,
        Some(0) => crate::messages::MessageDirection::Incoming,
        _ => crate::messages::MessageDirection::Unknown,
    }
}

fn message_body(value: &Value) -> crate::messages::MessageBodyView {
    let content_type = value
        .get("content_type")
        .and_then(Value::as_str)
        .map(str::to_string);
    let Some(content) = value.get("content") else {
        return crate::messages::MessageBodyView::Unsupported { content_type };
    };
    let text = match content {
        Value::String(value) => value.clone(),
        value => serde_json::to_string(value).unwrap_or_default(),
    };
    let kind = match content_type.as_deref() {
        Some("text/markdown") => crate::messages::MessageKind::Markdown,
        Some("text/plain") | None | Some("") => crate::messages::MessageKind::Text,
        _ => return crate::messages::MessageBodyView::Unsupported { content_type },
    };
    crate::messages::MessageBodyView::Text { text, kind }
}

fn string_value(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn i64_value(value: Option<&Value>) -> Option<i64> {
    match value {
        Some(Value::Number(number)) => number
            .as_i64()
            .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok()))
            .or_else(|| number.as_f64().map(|value| value as i64)),
        Some(Value::String(value)) => value.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn non_empty_or<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.trim().is_empty() {
        fallback
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal::auth::session::SessionProvider;
    use crate::internal::transport::AuthenticatedRpcTransport;
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
        );

        let result = runtime
            .inbox(InboxRead {
                query: crate::messages::InboxQuery {
                    scope: crate::messages::InboxScope::DirectOnly,
                    limit: crate::ids::PageLimit(20),
                    cursor: None,
                    unread_only: false,
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
        );

        let result = runtime
            .history(HistoryRead {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                query: crate::messages::HistoryQuery {
                    limit: crate::ids::PageLimit(5),
                    cursor: None,
                },
                resolved_peer_did: None,
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
        );

        let result = runtime
            .history(HistoryRead {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("bob.awiki.test", "").unwrap(),
                ),
                query: crate::messages::HistoryQuery {
                    limit: crate::ids::PageLimit(5),
                    cursor: Some(crate::ids::Cursor::parse("42").unwrap()),
                },
                resolved_peer_did: Some("did:example:bob".to_string()),
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
            })
        }

        fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
            unreachable!("read runtime should not refresh through the session provider")
        }

        fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
            unreachable!("read runtime should not read status")
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

    struct RecordedCall {
        endpoint: String,
        method: String,
        params: Value,
    }

    struct Fixture {
        root: PathBuf,
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
                    service_base_url: crate::ServiceEndpoint::parse("https://example.test")
                        .unwrap(),
                    did_domain: "awiki.test".to_string(),
                    user_service_endpoint: None,
                    message_service_endpoint: None,
                    anp_service_endpoint: None,
                    anp_service_did: None,
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
    }

    fn unique_temp_root() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "im-core-read-runtime-{}-{nanos}",
            std::process::id()
        ))
    }
}
