use serde_json::Value;

use crate::internal::auth::session::{AsyncSessionProvider, SessionProvider};
use crate::internal::transport::{AsyncAuthenticatedRpcTransport, AuthenticatedRpcTransport};

pub(crate) struct MessageMarkReadRuntime<'a, P, T> {
    client: &'a crate::core::ImClient,
    session_provider: P,
    transport: T,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MarkReadInput {
    pub message_ids: Vec<crate::ids::MessageId>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MarkReadRuntimeResult {
    pub sdk_result: crate::messages::MarkReadResult,
    pub raw: Option<Value>,
    pub direct_ids: Vec<String>,
    pub group_ids: Vec<String>,
    pub local_only_ids: Vec<String>,
}

impl<'a, P, T> MessageMarkReadRuntime<'a, P, T>
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

    pub(crate) fn mark_read(
        mut self,
        input: MarkReadInput,
    ) -> crate::ImResult<MarkReadRuntimeResult> {
        if input.message_ids.is_empty() {
            return Err(crate::ImError::MessageNotFound {
                message_id: "message_ids".to_string(),
            });
        }
        let ids = input
            .message_ids
            .iter()
            .map(|id| id.as_str().to_string())
            .collect::<Vec<_>>();
        let classification = classify_mark_read_ids(self.client, &ids);
        let direct_ids = classification
            .as_ref()
            .map(|value| value.direct_ids.clone())
            .unwrap_or_else(|_| ids.clone());
        let group_ids = classification
            .as_ref()
            .map(|value| value.group_ids.clone())
            .unwrap_or_default();
        let local_only_ids = classification
            .as_ref()
            .map(|value| value.local_only_ids.clone())
            .unwrap_or_default();

        let mut warnings = Vec::new();
        let mut updated_count = 0_i64;
        let mut raw = None;
        if !direct_ids.is_empty() {
            self.session_provider
                .ensure_session(crate::auth::AuthScope::Messaging)?;
            let params = crate::internal::wire::inbox::build_mark_read_rpc_params(
                &crate::internal::wire::common::WireIdentity {
                    did: self.client.did().as_str().to_string(),
                },
                crate::internal::wire::inbox::MarkReadWireRequest {
                    message_ids: direct_ids.clone(),
                },
            )?;
            let response = self.transport.authenticated_rpc(
                super::read::MESSAGE_RPC_ENDPOINT,
                "inbox.mark_read",
                params,
            )?;
            updated_count += int_value(response.get("updated_count"), direct_ids.len() as i64);
            warnings.extend(warnings_from_raw(&response));
            raw = Some(response);
        }

        if let Ok(local_updated) = mark_local_messages_read(self.client, classification.as_ref()) {
            if updated_count == 0 {
                updated_count = local_updated;
            } else {
                updated_count += (group_ids.len() + local_only_ids.len()) as i64;
            }
        } else if classification.is_ok() {
            warnings.push("Failed to mark local messages read".to_string());
        }

        Ok(MarkReadRuntimeResult {
            sdk_result: crate::messages::MarkReadResult {
                updated_count: u32::try_from(updated_count).unwrap_or(u32::MAX),
                message_ids: input.message_ids,
                warnings,
            },
            raw,
            direct_ids,
            group_ids,
            local_only_ids,
        })
    }
}

impl<'a, P, T> MessageMarkReadRuntime<'a, P, T>
where
    P: AsyncSessionProvider,
    T: AsyncAuthenticatedRpcTransport,
{
    pub(crate) async fn mark_read_async(
        mut self,
        input: MarkReadInput,
    ) -> crate::ImResult<MarkReadRuntimeResult> {
        if input.message_ids.is_empty() {
            return Err(crate::ImError::MessageNotFound {
                message_id: "message_ids".to_string(),
            });
        }
        let ids = input
            .message_ids
            .iter()
            .map(|id| id.as_str().to_string())
            .collect::<Vec<_>>();
        let classification = classify_mark_read_ids_async(self.client, &ids).await;
        let direct_ids = classification
            .as_ref()
            .map(|value| value.direct_ids.clone())
            .unwrap_or_else(|_| ids.clone());
        let group_ids = classification
            .as_ref()
            .map(|value| value.group_ids.clone())
            .unwrap_or_default();
        let local_only_ids = classification
            .as_ref()
            .map(|value| value.local_only_ids.clone())
            .unwrap_or_default();

        let mut warnings = Vec::new();
        let mut updated_count = 0_i64;
        let mut raw = None;
        if !direct_ids.is_empty() {
            self.session_provider
                .ensure_session(crate::auth::AuthScope::Messaging)
                .await?;
            let params = crate::internal::wire::inbox::build_mark_read_rpc_params(
                &crate::internal::wire::common::WireIdentity {
                    did: self.client.did().as_str().to_string(),
                },
                crate::internal::wire::inbox::MarkReadWireRequest {
                    message_ids: direct_ids.clone(),
                },
            )?;
            let response = self
                .transport
                .authenticated_rpc(super::read::MESSAGE_RPC_ENDPOINT, "inbox.mark_read", params)
                .await?;
            updated_count += int_value(response.get("updated_count"), direct_ids.len() as i64);
            warnings.extend(warnings_from_raw(&response));
            raw = Some(response);
        }

        if let Ok(local_updated) =
            mark_local_messages_read_async(self.client, classification.as_ref()).await
        {
            if updated_count == 0 {
                updated_count = local_updated;
            } else {
                updated_count += (group_ids.len() + local_only_ids.len()) as i64;
            }
        } else if classification.is_ok() {
            warnings.push("Failed to mark local messages read".to_string());
        }

        Ok(MarkReadRuntimeResult {
            sdk_result: crate::messages::MarkReadResult {
                updated_count: u32::try_from(updated_count).unwrap_or(u32::MAX),
                message_ids: input.message_ids,
                warnings,
            },
            raw,
            direct_ids,
            group_ids,
            local_only_ids,
        })
    }
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
fn classify_mark_read_ids(
    client: &crate::core::ImClient,
    ids: &[String],
) -> crate::ImResult<crate::internal::local_state::messages::MarkReadClassification> {
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    crate::internal::local_state::messages::classify_mark_read_ids_for_owner_identity(
        &connection,
        client.current_identity().id.as_str(),
        client.did().as_str(),
        ids,
    )
}

#[cfg(all(feature = "sqlite", not(any(feature = "blocking", test))))]
fn classify_mark_read_ids(
    _client: &crate::core::ImClient,
    _ids: &[String],
) -> crate::ImResult<crate::internal::local_state::messages::MarkReadClassification> {
    Err(crate::ImError::unsupported("sync-message-mark-read"))
}

#[cfg(not(feature = "sqlite"))]
fn classify_mark_read_ids(
    _client: &crate::core::ImClient,
    ids: &[String],
) -> crate::ImResult<NoSqliteMarkReadClassification> {
    Ok(NoSqliteMarkReadClassification {
        direct_ids: ids.to_vec(),
        group_ids: Vec::new(),
        local_only_ids: Vec::new(),
    })
}

#[cfg(feature = "sqlite")]
async fn classify_mark_read_ids_async(
    client: &crate::core::ImClient,
    ids: &[String],
) -> crate::ImResult<crate::internal::local_state::messages::MarkReadClassification> {
    client
        .core_inner()
        .local_state_db()
        .await?
        .classify_mark_read_ids(
            client.current_identity().id.as_str(),
            client.did().as_str(),
            ids.to_vec(),
        )
        .await
}

#[cfg(not(feature = "sqlite"))]
async fn classify_mark_read_ids_async(
    _client: &crate::core::ImClient,
    ids: &[String],
) -> crate::ImResult<NoSqliteMarkReadClassification> {
    Ok(NoSqliteMarkReadClassification {
        direct_ids: ids.to_vec(),
        group_ids: Vec::new(),
        local_only_ids: Vec::new(),
    })
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
fn mark_local_messages_read(
    client: &crate::core::ImClient,
    classification: Result<
        &crate::internal::local_state::messages::MarkReadClassification,
        &crate::ImError,
    >,
) -> crate::ImResult<i64> {
    let classification = classification.map_err(Clone::clone)?;
    let local_ids = classification.local_ids();
    if local_ids.is_empty() {
        return Ok(0);
    }
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    crate::internal::local_state::messages::mark_messages_read_for_owner_identity(
        &connection,
        client.current_identity().id.as_str(),
        client.did().as_str(),
        &local_ids,
    )
}

#[cfg(all(feature = "sqlite", not(any(feature = "blocking", test))))]
fn mark_local_messages_read(
    _client: &crate::core::ImClient,
    _classification: Result<
        &crate::internal::local_state::messages::MarkReadClassification,
        &crate::ImError,
    >,
) -> crate::ImResult<i64> {
    Err(crate::ImError::unsupported("sync-message-mark-read"))
}

#[cfg(feature = "sqlite")]
async fn mark_local_messages_read_async(
    client: &crate::core::ImClient,
    classification: Result<
        &crate::internal::local_state::messages::MarkReadClassification,
        &crate::ImError,
    >,
) -> crate::ImResult<i64> {
    let classification = classification.map_err(Clone::clone)?;
    let local_ids = classification.local_ids();
    if local_ids.is_empty() {
        return Ok(0);
    }
    client
        .core_inner()
        .local_state_db()
        .await?
        .mark_messages_read(
            client.current_identity().id.as_str(),
            client.did().as_str(),
            local_ids,
        )
        .await
}

#[cfg(not(feature = "sqlite"))]
async fn mark_local_messages_read_async(
    _client: &crate::core::ImClient,
    _classification: Result<&NoSqliteMarkReadClassification, &crate::ImError>,
) -> crate::ImResult<i64> {
    Ok(0)
}

#[cfg(not(feature = "sqlite"))]
fn mark_local_messages_read(
    _client: &crate::core::ImClient,
    _classification: Result<&NoSqliteMarkReadClassification, &crate::ImError>,
) -> crate::ImResult<i64> {
    Ok(0)
}

fn int_value(value: Option<&Value>, fallback: i64) -> i64 {
    match value {
        Some(Value::Number(number)) => number
            .as_i64()
            .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok()))
            .or_else(|| number.as_f64().map(|value| value as i64))
            .unwrap_or(fallback),
        Some(Value::String(value)) => value.trim().parse().unwrap_or(fallback),
        _ => fallback,
    }
}

fn warnings_from_raw(value: &Value) -> Vec<String> {
    value
        .get("warnings")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(not(feature = "sqlite"))]
struct NoSqliteMarkReadClassification {
    direct_ids: Vec<String>,
    group_ids: Vec<String>,
    local_only_ids: Vec<String>,
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
    fn mark_read_runtime_marks_direct_remote_and_local_rows() {
        let fixture = Fixture::new();
        let client = fixture.client();
        fixture.seed_message(&client, "direct-1", "", "text/plain", "", 0);
        let calls = Rc::new(RefCell::new(Vec::new()));

        let result = MessageMarkReadRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({"updated_count": 3}),
            },
        )
        .mark_read(MarkReadInput {
            message_ids: vec![crate::ids::MessageId::parse("direct-1").unwrap()],
        })
        .unwrap();

        assert_eq!(result.sdk_result.updated_count, 3);
        assert_eq!(result.direct_ids, vec!["direct-1"]);
        assert_eq!(fixture.is_read(&client, "direct-1"), 1);
        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].endpoint, super::super::read::MESSAGE_RPC_ENDPOINT);
        assert_eq!(calls[0].method, "inbox.mark_read");
        assert_eq!(calls[0].params["body"]["message_ids"], json!(["direct-1"]));
    }

    #[test]
    fn mark_read_runtime_keeps_group_and_mail_local_only() {
        let fixture = Fixture::new();
        let client = fixture.client();
        fixture.seed_message(&client, "group-1", "did:example:group", "text/plain", "", 0);
        fixture.seed_message(
            &client,
            "mail-1",
            "",
            "mail.notification",
            r#"{"source_kind":"mail"}"#,
            0,
        );
        let calls = Rc::new(RefCell::new(Vec::new()));

        let result = MessageMarkReadRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({}),
            },
        )
        .mark_read(MarkReadInput {
            message_ids: vec![
                crate::ids::MessageId::parse("group-1").unwrap(),
                crate::ids::MessageId::parse("mail-1").unwrap(),
            ],
        })
        .unwrap();

        assert_eq!(result.sdk_result.updated_count, 2);
        assert!(result.direct_ids.is_empty());
        assert_eq!(result.group_ids, vec!["group-1"]);
        assert_eq!(result.local_only_ids, vec!["mail-1"]);
        assert!(calls.borrow().is_empty());
        assert_eq!(fixture.is_read(&client, "group-1"), 1);
        assert_eq!(fixture.is_read(&client, "mail-1"), 1);
    }

    #[tokio::test]
    async fn mark_read_runtime_async_marks_direct_remote_and_actor_local_rows() {
        let fixture = Fixture::new();
        let client = fixture.client();
        fixture.seed_message(&client, "direct-async-1", "", "text/plain", "", 0);
        let calls = Rc::new(RefCell::new(Vec::new()));

        let result = MessageMarkReadRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({"updated_count": 1}),
            },
        )
        .mark_read_async(MarkReadInput {
            message_ids: vec![crate::ids::MessageId::parse("direct-async-1").unwrap()],
        })
        .await
        .unwrap();

        assert_eq!(result.sdk_result.updated_count, 1);
        assert_eq!(result.direct_ids, vec!["direct-async-1"]);
        assert_eq!(fixture.is_read(&client, "direct-async-1"), 1);
        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].endpoint, super::super::read::MESSAGE_RPC_ENDPOINT);
        assert_eq!(calls[0].method, "inbox.mark_read");
        assert_eq!(
            calls[0].params["body"]["message_ids"],
            json!(["direct-async-1"])
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
            unreachable!("mark_read runtime refresh is transport-owned in migration")
        }

        fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
            unreachable!("mark_read runtime should not read status")
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

    impl crate::internal::transport::AsyncAuthenticatedRpcTransport for RecordingTransport {
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

        fn seed_message(
            &self,
            client: &crate::core::ImClient,
            message_id: &str,
            group_did: &str,
            content_type: &str,
            metadata: &str,
            is_read: i64,
        ) {
            let connection = crate::internal::local_state::open_writable(
                &client.core_inner().sdk_paths().local_state.sqlite_path,
            )
            .unwrap();
            connection
                .execute(
                    r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction, sender_did, receiver_did, group_id, group_did,
     content_type, content, stored_at, metadata, is_read)
VALUES (?1, ?2, ?3, ?4, ?4, 0, 'did:example:bob', ?3, ?5, ?5, ?6, 'hello', '2026-05-21T00:00:00Z', ?7, ?8)"#,
                    (
                        message_id,
                        client.current_identity().id.as_str(),
                        client.did().as_str(),
                        crate::internal::local_state::owner_scope::group_conversation_id(
                            group_did,
                        ),
                        group_did,
                        content_type,
                        metadata,
                        is_read,
                    ),
                )
                .unwrap();
        }

        fn is_read(&self, client: &crate::core::ImClient, message_id: &str) -> i64 {
            let connection = rusqlite::Connection::open(
                &client.core_inner().sdk_paths().local_state.sqlite_path,
            )
            .unwrap();
            connection
                .query_row(
                    "SELECT is_read FROM messages WHERE owner_identity_id = ?1 AND msg_id = ?2",
                    (client.current_identity().id.as_str(), message_id),
                    |row| row.get(0),
                )
                .unwrap()
        }
    }

    fn unique_temp_root() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "im-core-mark-read-runtime-{}-{nanos}",
            std::process::id()
        ))
    }
}
