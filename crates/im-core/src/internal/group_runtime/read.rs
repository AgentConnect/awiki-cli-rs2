use serde_json::Value;

use crate::internal::auth::session::SessionProvider;
use crate::internal::transport::AuthenticatedRpcTransport;

pub(crate) const MESSAGE_RPC_ENDPOINT: &str = "/im/rpc";

pub(crate) struct GroupReadRuntime<'a, P, T> {
    client: &'a crate::core::ImClient,
    session_provider: P,
    transport: T,
}

impl<'a, P, T> GroupReadRuntime<'a, P, T>
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
        self.group_rpc("group.list_messages", params)
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
        Ok(crate::groups::GroupReadResult::from_diagnostic_raw(
            raw,
            Vec::new(),
        ))
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
            })
        }

        fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
            unreachable!("group read runtime should not refresh through the session provider")
        }

        fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
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
            "im-core-group-read-runtime-{}-{nanos}",
            std::process::id()
        ))
    }
}
