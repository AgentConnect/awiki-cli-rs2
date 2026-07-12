use serde_json::Value;

use crate::internal::auth::session::{AsyncSessionProvider, SessionProvider};
use crate::internal::transport::{AsyncAuthenticatedRpcTransport, AuthenticatedRpcTransport};

pub(crate) const MESSAGE_RPC_ENDPOINT: &str = "/im/rpc";

pub(crate) struct GroupReadRuntime<'a, P, T> {
    client: &'a crate::core::ImClient,
    session_provider: P,
    transport: T,
}

impl<'a, P, T> GroupReadRuntime<'a, P, T> {
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
}

impl<'a, P, T> GroupReadRuntime<'a, P, T>
where
    P: SessionProvider,
    T: AuthenticatedRpcTransport,
{
    pub(crate) fn get(
        mut self,
        group: crate::ids::GroupRef,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        self.ensure_group_session()?;
        let params = crate::internal::wire::group::build_group_get_rpc_params(
            self.client.did().as_str(),
            group.as_str(),
        )?;
        self.group_rpc("group.get", params)
    }

    pub(crate) fn list(
        mut self,
        request: crate::groups::GroupListRequest,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        self.ensure_group_session()?;
        let params = crate::internal::wire::group::build_group_list_rpc_params(
            self.client.did().as_str(),
            page_limit(request.limit, 50),
        );
        self.group_rpc("group.list", params)
    }

    pub(crate) fn members(
        mut self,
        request: crate::groups::GroupMembersRequest,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        self.ensure_group_session()?;
        let params = crate::internal::wire::group::build_group_members_rpc_params(
            self.client.did().as_str(),
            request.group.as_str(),
            page_limit(request.limit, 100),
        )?;
        self.group_rpc("group.list_members", params)
    }

    pub(crate) fn messages(
        mut self,
        request: crate::groups::GroupMessagesRequest,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        self.ensure_group_session()?;
        let params = crate::internal::wire::group::build_group_messages_rpc_params(
            self.client.did().as_str(),
            request.group.as_str(),
            page_limit(request.limit, 50),
            request.cursor.as_ref().map(crate::ids::Cursor::as_str),
            0,
        )?;
        let mut raw = self.transport.authenticated_rpc(
            MESSAGE_RPC_ENDPOINT,
            "group.list_messages",
            params,
        )?;
        project_group_e2ee_messages(self.client, &mut raw);
        Ok(crate::groups::GroupReadResult::from_raw_response(
            raw,
            Vec::new(),
        ))
    }

    fn ensure_group_session(&self) -> crate::ImResult<crate::auth::SessionBundle> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::GroupMessaging)
    }

    fn group_rpc(
        &mut self,
        method: &str,
        params: Value,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        let raw = self
            .transport
            .authenticated_rpc(MESSAGE_RPC_ENDPOINT, method, params)?;
        Ok(crate::groups::GroupReadResult::from_raw_response(
            raw,
            Vec::new(),
        ))
    }
}

impl<'a, P, T> GroupReadRuntime<'a, P, T>
where
    P: AsyncSessionProvider,
    T: AsyncAuthenticatedRpcTransport,
{
    pub(crate) async fn get_async(
        mut self,
        group: crate::ids::GroupRef,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        self.ensure_group_session_async().await?;
        let params = crate::internal::wire::group::build_group_get_rpc_params(
            self.client.did().as_str(),
            group.as_str(),
        )?;
        self.group_rpc_async("group.get", params).await
    }

    pub(crate) async fn list_async(
        mut self,
        request: crate::groups::GroupListRequest,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        self.ensure_group_session_async().await?;
        let params = crate::internal::wire::group::build_group_list_rpc_params(
            self.client.did().as_str(),
            page_limit(request.limit, 50),
        );
        self.group_rpc_async("group.list", params).await
    }

    pub(crate) async fn members_async(
        mut self,
        request: crate::groups::GroupMembersRequest,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        self.ensure_group_session_async().await?;
        let params = crate::internal::wire::group::build_group_members_rpc_params(
            self.client.did().as_str(),
            request.group.as_str(),
            page_limit(request.limit, 100),
        )?;
        self.group_rpc_async("group.list_members", params).await
    }

    pub(crate) async fn messages_async(
        mut self,
        request: crate::groups::GroupMessagesRequest,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        self.ensure_group_session_async().await?;
        let params = crate::internal::wire::group::build_group_messages_rpc_params(
            self.client.did().as_str(),
            request.group.as_str(),
            page_limit(request.limit, 50),
            request.cursor.as_ref().map(crate::ids::Cursor::as_str),
            0,
        )?;
        let mut raw = self
            .transport
            .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "group.list_messages", params)
            .await?;
        project_group_e2ee_messages_async(self.client, &mut raw).await;
        Ok(crate::groups::GroupReadResult::from_raw_response(
            raw,
            Vec::new(),
        ))
    }

    async fn ensure_group_session_async(&self) -> crate::ImResult<crate::auth::SessionBundle> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::GroupMessaging)
            .await
    }

    async fn group_rpc_async(
        &mut self,
        method: &str,
        params: Value,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        let raw = self
            .transport
            .authenticated_rpc(MESSAGE_RPC_ENDPOINT, method, params)
            .await?;
        Ok(crate::groups::GroupReadResult::from_raw_response(
            raw,
            Vec::new(),
        ))
    }
}

#[cfg(feature = "group-e2ee")]
fn project_group_e2ee_messages(client: &crate::core::ImClient, raw: &mut Value) {
    let Some(messages) = raw.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };
    let mut message_values = std::mem::take(messages);
    crate::internal::message_runtime::read::apply_cached_group_e2ee_messages(
        client,
        &mut message_values,
    );
    let warnings =
        crate::internal::group_e2ee::incoming::maybe_decrypt_group_e2ee_messages_for_client(
            client,
            &mut message_values,
        );
    crate::internal::message_runtime::read::cache_attachment_manifests_for_internal_download(
        client,
        &message_values,
    );
    crate::internal::message_runtime::read::redact_attachment_manifests_for_public_projection(
        &mut message_values,
    );
    *messages = message_values;
    append_warnings(raw, warnings);
}

#[cfg(not(feature = "group-e2ee"))]
fn project_group_e2ee_messages(_client: &crate::core::ImClient, _raw: &mut Value) {}

#[cfg(feature = "group-e2ee")]
async fn project_group_e2ee_messages_async(client: &crate::core::ImClient, raw: &mut Value) {
    let Some(messages) = raw.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };
    let mut message_values = std::mem::take(messages);
    crate::internal::message_runtime::read::apply_cached_group_e2ee_messages_async(
        client,
        &mut message_values,
    )
    .await;
    let warnings =
        crate::internal::group_e2ee::incoming::maybe_decrypt_group_e2ee_messages_for_client_async(
            client,
            &mut message_values,
        )
        .await;
    crate::internal::message_runtime::read::cache_attachment_manifests_for_internal_download_async(
        client,
        &message_values,
    )
    .await;
    crate::internal::message_runtime::read::redact_attachment_manifests_for_public_projection(
        &mut message_values,
    );
    *messages = message_values;
    append_warnings(raw, warnings);
}

#[cfg(not(feature = "group-e2ee"))]
async fn project_group_e2ee_messages_async(_client: &crate::core::ImClient, _raw: &mut Value) {}

fn append_warnings(raw: &mut Value, warnings: Vec<String>) {
    if warnings.is_empty() {
        return;
    }
    let Some(object) = raw.as_object_mut() else {
        return;
    };
    let entry = object
        .entry("warnings")
        .or_insert_with(|| Value::Array(Vec::new()));
    if let Value::Array(items) = entry {
        items.extend(warnings.into_iter().map(Value::String));
    }
}

fn page_limit(limit: crate::ids::PageLimit, fallback: i64) -> i64 {
    if limit.0 == 0 {
        fallback
    } else {
        i64::from(limit.0)
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
    fn groups_read_runtime_builds_get_list_members_and_messages_rpc() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let group = crate::ids::GroupRef::parse("did:example:group").unwrap();

        GroupReadRuntime::new(
            &client,
            ReadyGroupSessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({"group_did":"did:example:group"}),
            },
        )
        .get(group.clone())
        .unwrap();

        GroupReadRuntime::new(
            &client,
            ReadyGroupSessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({"groups":[]}),
            },
        )
        .list(crate::groups::GroupListRequest {
            limit: crate::ids::PageLimit(25),
        })
        .unwrap();

        GroupReadRuntime::new(
            &client,
            ReadyGroupSessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({"members":[]}),
            },
        )
        .members(crate::groups::GroupMembersRequest {
            group: group.clone(),
            limit: crate::ids::PageLimit(10),
        })
        .unwrap();

        GroupReadRuntime::new(
            &client,
            ReadyGroupSessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({"messages":[]}),
            },
        )
        .messages(crate::groups::GroupMessagesRequest {
            group,
            limit: crate::ids::PageLimit(5),
            cursor: Some(crate::ids::Cursor::parse("42").unwrap()),
        })
        .unwrap();

        let calls = calls.borrow();
        assert_eq!(calls.len(), 4);
        assert_eq!(calls[0].method, "group.get");
        assert_eq!(calls[0].endpoint, MESSAGE_RPC_ENDPOINT);
        assert_eq!(calls[0].params["meta"]["sender_did"], "did:example:alice");
        assert_eq!(
            calls[0].params["meta"]["target"],
            json!({"kind":"group","did":"did:example:group"})
        );
        assert_eq!(calls[0].params["body"]["group_did"], "did:example:group");
        assert_eq!(calls[1].method, "group.list");
        assert_eq!(calls[1].params["body"]["limit"], 25);
        assert_eq!(calls[2].method, "group.list_members");
        assert_eq!(calls[2].params["body"]["limit"], 10);
        assert_eq!(calls[3].method, "group.list_messages");
        assert_eq!(calls[3].params["body"]["limit"], 5);
        assert_eq!(calls[3].params["body"]["since_seq"], "42");
    }

    #[tokio::test]
    async fn groups_read_runtime_builds_get_list_members_and_messages_rpc_async() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let group = crate::ids::GroupRef::parse("did:example:group").unwrap();

        GroupReadRuntime::new(
            &client,
            ReadyGroupSessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({"group_did":"did:example:group"}),
            },
        )
        .get_async(group.clone())
        .await
        .unwrap();

        GroupReadRuntime::new(
            &client,
            ReadyGroupSessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({"groups":[]}),
            },
        )
        .list_async(crate::groups::GroupListRequest {
            limit: crate::ids::PageLimit(25),
        })
        .await
        .unwrap();

        GroupReadRuntime::new(
            &client,
            ReadyGroupSessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({"members":[]}),
            },
        )
        .members_async(crate::groups::GroupMembersRequest {
            group: group.clone(),
            limit: crate::ids::PageLimit(10),
        })
        .await
        .unwrap();

        GroupReadRuntime::new(
            &client,
            ReadyGroupSessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({"messages":[]}),
            },
        )
        .messages_async(crate::groups::GroupMessagesRequest {
            group,
            limit: crate::ids::PageLimit(5),
            cursor: Some(crate::ids::Cursor::parse("42").unwrap()),
        })
        .await
        .unwrap();

        let calls = calls.borrow();
        assert_eq!(calls.len(), 4);
        assert_eq!(calls[0].method, "group.get");
        assert_eq!(calls[0].endpoint, MESSAGE_RPC_ENDPOINT);
        assert_eq!(calls[0].params["meta"]["sender_did"], "did:example:alice");
        assert_eq!(
            calls[0].params["meta"]["target"],
            json!({"kind":"group","did":"did:example:group"})
        );
        assert_eq!(calls[0].params["body"]["group_did"], "did:example:group");
        assert_eq!(calls[1].method, "group.list");
        assert_eq!(calls[1].params["body"]["limit"], 25);
        assert_eq!(calls[2].method, "group.list_members");
        assert_eq!(calls[2].params["body"]["limit"], 10);
        assert_eq!(calls[3].method, "group.list_messages");
        assert_eq!(calls[3].params["body"]["limit"], 5);
        assert_eq!(calls[3].params["body"]["since_seq"], "42");
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
            unreachable!("group read runtime should not refresh through the session provider")
        }

        fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
            unreachable!("group read runtime should not read status")
        }
    }

    impl crate::internal::auth::session::AsyncSessionProvider for ReadyGroupSessionProvider {
        async fn ensure_session(
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

        async fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
            unreachable!("group read runtime should not refresh through the session provider")
        }

        async fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
            unreachable!("group read runtime should not read status")
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
    }

    fn unique_temp_root() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "im-core-group-read-runtime-{}-{nanos}",
            std::process::id()
        ))
    }
}
