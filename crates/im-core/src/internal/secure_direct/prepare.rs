#[cfg(feature = "sqlite")]
use serde_json::{Map, Value};

#[cfg(feature = "sqlite")]
use crate::internal::auth::session::SessionProvider;
#[cfg(feature = "sqlite")]
use crate::internal::transport::AuthenticatedRpcTransport;

#[cfg(feature = "sqlite")]
use super::client::{
    prepare_direct_secure_client, DirectSecureClientInput, MessageServiceDirectSecureClient,
};

#[cfg(feature = "sqlite")]
#[derive(Debug)]
pub(crate) struct DirectSecurePreparePlan {
    pub(crate) status: super::status::DirectSecureLocalStatus,
    pub(crate) prepared_local_send_state: bool,
}

#[cfg(feature = "sqlite")]
pub(crate) fn prepare_direct_for_client(
    client: &crate::core::ImClient,
    peer: crate::ids::PeerRef,
) -> crate::ImResult<DirectSecurePreparePlan> {
    prepare_direct_for_client_with_transport(
        client,
        peer,
        crate::internal::auth::session::FileSessionProvider::new(client),
        crate::internal::transport::CoreHttpTransport::new(client),
    )
}

#[cfg(feature = "sqlite")]
pub(crate) fn prepare_direct_for_client_with_transport<P, T>(
    client: &crate::core::ImClient,
    peer: crate::ids::PeerRef,
    session_provider: P,
    transport: T,
) -> crate::ImResult<DirectSecurePreparePlan>
where
    P: SessionProvider,
    T: AuthenticatedRpcTransport,
{
    session_provider.ensure_session(crate::auth::AuthScope::Messaging)?;
    let runtime = client.runtime();
    let local_did_document = read_json_file(&runtime.did_document_path, "did_document")?;
    let signing_private_pem = read_text_file(&runtime.private_key_path, "private_key")?;
    let agreement_private_pem = read_text_file(
        &runtime.e2ee_agreement_private_key_path,
        "e2ee_agreement_private_key",
    )?;
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    let mut transport = transport;
    let rpc = Box::new(move |method: &str, params: Map<String, Value>| {
        let value = transport.authenticated_rpc(
            super::send::MESSAGE_RPC_ENDPOINT,
            method,
            Value::Object(params),
        )?;
        object_result(value)
    });
    let local_document_for_resolver = local_did_document.clone();
    let owner_did = client.did().as_str().to_owned();
    let resolver = Box::new(move |did: &str| {
        if did == owner_did {
            Ok(local_document_for_resolver.clone())
        } else {
            Err(crate::ImError::PeerNotFound {
                peer: did.to_owned(),
            })
        }
    });
    let mut direct_client = MessageServiceDirectSecureClient::new(
        prepare_direct_secure_client(DirectSecureClientInput {
            owner_identity_id: client.current_identity().id.as_str().to_owned(),
            owner_did: client.did().as_str().to_owned(),
            identity_name: client
                .current_identity()
                .local_alias
                .clone()
                .unwrap_or_else(|| client.current_identity().id.as_str().to_owned()),
            signing_key_id: format!("{}#key-1", client.did().as_str()),
            agreement_key_id: format!("{}#key-3", client.did().as_str()),
            signing_private_pem,
            agreement_private_pem,
            local_did_document,
            local_state: &connection,
        })?,
        rpc,
        resolver,
    );

    let mut warnings = Vec::new();
    let prepared_local_send_state = match direct_client.publish_prekey_bundle() {
        Ok(_) => true,
        Err(err) => {
            warnings.push(format!("direct E2EE prekey bundle publish failed: {err}"));
            false
        }
    };
    let mut status = super::status::direct_status_for_connection(client, &connection, peer)?;
    if !prepared_local_send_state && status.problem.is_some() {
        return Err(crate::ImError::LocalStateUnavailable {
            detail: "direct E2EE prekey preparation did not produce usable local prekeys"
                .to_owned(),
        });
    }
    warnings.extend(status.warnings);
    status.warnings = compact_warnings(warnings);
    Ok(DirectSecurePreparePlan {
        status,
        prepared_local_send_state,
    })
}

#[cfg(not(feature = "sqlite"))]
#[derive(Debug)]
pub(crate) struct DirectSecurePreparePlan {
    pub(crate) status: super::status::DirectSecureLocalStatus,
    pub(crate) prepared_local_send_state: bool,
}

#[cfg(not(feature = "sqlite"))]
pub(crate) fn prepare_direct_for_client(
    client: &crate::core::ImClient,
    peer: crate::ids::PeerRef,
) -> crate::ImResult<DirectSecurePreparePlan> {
    Ok(DirectSecurePreparePlan {
        status: super::status::direct_status_for_client(client, peer)?,
        prepared_local_send_state: false,
    })
}

#[cfg(feature = "sqlite")]
fn read_json_file(path: &std::path::Path, path_kind: &str) -> crate::ImResult<Value> {
    let raw = std::fs::read(path).map_err(|err| crate::ImError::CredentialFileUnreadable {
        path_kind: path_kind.to_owned(),
        detail: err.to_string(),
    })?;
    serde_json::from_slice(&raw).map_err(|err| crate::ImError::Serialization {
        detail: err.to_string(),
    })
}

#[cfg(feature = "sqlite")]
fn read_text_file(path: &std::path::Path, path_kind: &str) -> crate::ImResult<String> {
    std::fs::read_to_string(path).map_err(|err| crate::ImError::CredentialFileUnreadable {
        path_kind: path_kind.to_owned(),
        detail: err.to_string(),
    })
}

#[cfg(feature = "sqlite")]
fn object_result(value: Value) -> crate::ImResult<Map<String, Value>> {
    match value {
        Value::Object(object) => Ok(object),
        Value::Null => Ok(Map::new()),
        other => Err(crate::ImError::Serialization {
            detail: format!("expected object RPC result, got {other}"),
        }),
    }
}

#[cfg(feature = "sqlite")]
fn compact_warnings(warnings: Vec<String>) -> Vec<String> {
    let mut compact = Vec::new();
    for warning in warnings {
        let warning = warning.trim().to_owned();
        if warning.is_empty() || compact.iter().any(|known| known == &warning) {
            continue;
        }
        compact.push(warning);
    }
    compact
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use std::cell::RefCell;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::rc::Rc;

    use serde_json::{json, Value};

    use super::*;
    use crate::internal::secure_direct::sqlite_store::SqliteDirectSecureStateStore;

    #[test]
    fn prepare_direct_generates_sqlite_prekeys_and_publishes_bundle_without_public_leakage() {
        let identity = TestIdentity::new("alice.prepare.example", "alice");
        let fixture = Fixture::new(&identity);
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::<RecordedCall>::new()));

        let plan = prepare_direct_for_client_with_transport(
            &client,
            crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ReadySessionProvider,
            RecordingAuthenticatedTransport {
                calls: calls.clone(),
                responses: Rc::new(RefCell::new(vec![json!({}), json!({})])),
            },
        )
        .unwrap();

        assert!(plan.prepared_local_send_state);
        assert_eq!(
            plan.status.state,
            crate::secure::DirectSecureState::WaitingForPeer
        );
        assert!(!plan.status.can_send_secure);
        assert!(!format!("{plan:?}").contains("private_key_blob"));
        assert!(!format!("{plan:?}").contains("one_time_prekeys"));

        let calls = calls.borrow();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].method, "direct.e2ee.publish_prekey_bundle");
        assert_eq!(calls[1].method, "direct.e2ee.publish_prekey_bundle");
        assert!(calls[1].params.pointer("/body/prekey_bundle").is_some());
        assert_eq!(
            calls[1]
                .params
                .pointer("/body/one_time_prekeys")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(16)
        );

        let db = crate::internal::local_state::open_writable(
            &client.core_inner().sdk_paths().local_state.sqlite_path,
        )
        .unwrap();
        let store = SqliteDirectSecureStateStore::new(&db).unwrap();
        assert!(store.active_signed_prekey("alice-id").unwrap().is_some());
        assert_eq!(
            store
                .list_available_one_time_prekeys("alice-id")
                .unwrap()
                .len(),
            16
        );
    }

    #[test]
    fn prepare_direct_keeps_local_prekeys_when_publish_fails() {
        let identity = TestIdentity::new("alice.prepare-fail.example", "alice");
        let fixture = Fixture::new(&identity);
        let client = fixture.client();

        let plan = prepare_direct_for_client_with_transport(
            &client,
            crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ReadySessionProvider,
            FailingAuthenticatedTransport,
        )
        .unwrap();

        assert!(!plan.prepared_local_send_state);
        assert_eq!(
            plan.status.state,
            crate::secure::DirectSecureState::WaitingForPeer
        );
        assert!(plan
            .status
            .warnings
            .iter()
            .any(|warning| warning.contains("prekey bundle publish failed")));

        let db = crate::internal::local_state::open_writable(
            &client.core_inner().sdk_paths().local_state.sqlite_path,
        )
        .unwrap();
        let store = SqliteDirectSecureStateStore::new(&db).unwrap();
        assert!(store.active_signed_prekey("alice-id").unwrap().is_some());
        assert_eq!(
            store
                .list_available_one_time_prekeys("alice-id")
                .unwrap()
                .len(),
            16
        );
    }

    struct ReadySessionProvider;

    impl SessionProvider for ReadySessionProvider {
        fn ensure_session(
            &self,
            scope: crate::auth::AuthScope,
        ) -> crate::ImResult<crate::auth::SessionBundle> {
            Ok(crate::auth::SessionBundle {
                subject: crate::ids::Did::parse("did:example:alice")?,
                scope,
                expires_at: None,
                refreshed: false,
            })
        }

        fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
            unreachable!("direct prepare should not refresh in this unit test")
        }

        fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
            unreachable!("direct prepare should not read auth status")
        }
    }

    #[derive(Clone)]
    struct RecordedCall {
        method: String,
        params: Value,
    }

    struct RecordingAuthenticatedTransport {
        calls: Rc<RefCell<Vec<RecordedCall>>>,
        responses: Rc<RefCell<Vec<Value>>>,
    }

    impl AuthenticatedRpcTransport for RecordingAuthenticatedTransport {
        fn authenticated_rpc(
            &mut self,
            _endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            self.calls.borrow_mut().push(RecordedCall {
                method: method.to_owned(),
                params,
            });
            Ok(self.responses.borrow_mut().remove(0))
        }
    }

    struct FailingAuthenticatedTransport;

    impl AuthenticatedRpcTransport for FailingAuthenticatedTransport {
        fn authenticated_rpc(
            &mut self,
            _endpoint: &str,
            method: &str,
            _params: Value,
        ) -> crate::ImResult<Value> {
            Err(crate::ImError::TransportUnavailable {
                detail: format!("{method} unavailable"),
            })
        }
    }

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new(identity: &TestIdentity) -> Self {
            let root = unique_temp_root("im-core-secure-direct-prepare");
            write_identity_fixture(&root, "alice", identity);
            Self { root }
        }

        fn client(&self) -> crate::core::ImClient {
            crate::core::ImCore::new(
                crate::ImCoreConfig {
                    service_base_url: crate::ServiceEndpoint::parse("https://example.test")
                        .unwrap(),
                    did_domain: "awiki.test".to_owned(),
                    user_service_endpoint: None,
                    mail_service_endpoint: None,
                    message_service_endpoint: None,
                    anp_service_endpoint: None,
                    anp_service_did: None,
                    ca_bundle: None,
                    transport_policy: crate::MessageTransportPolicy::HttpOnly,
                },
                crate::ImCorePaths {
                    identities: crate::IdentityRegistryPaths {
                        identity_root_dir: self.root.join("identities"),
                        registry_path: self.root.join("identities").join("registry.json"),
                        default_identity_path: Some(self.root.join("identities").join("default")),
                    },
                    local_state: crate::LocalStatePaths {
                        sqlite_path: self.root.join("local").join("im.sqlite"),
                    },
                    runtime: crate::RuntimePaths {
                        cache_dir: self.root.join("cache"),
                        temp_dir: self.root.join("tmp"),
                    },
                },
            )
            .unwrap()
            .client(crate::identity::IdentitySelector::LocalAlias(
                "alice".to_owned(),
            ))
            .unwrap()
        }
    }

    struct TestIdentity {
        did: String,
        document: Value,
        key1_private_pem: String,
        agreement_private_pem: String,
    }

    impl TestIdentity {
        fn new(domain: &str, label: &str) -> Self {
            let service = anp::authentication::build_agent_message_service_with_options(
                "#message",
                format!("https://{domain}/anp-im/rpc"),
                anp::authentication::AnpMessageServiceOptions::default()
                    .with_service_did(format!("did:wba:{domain}")),
            );
            let bundle = anp::authentication::create_did_wba_document(
                domain,
                anp::authentication::DidDocumentOptions {
                    path_segments: vec!["agents".to_owned(), label.to_owned()],
                    domain: Some(domain.to_owned()),
                    challenge: Some(format!("secure-direct-prepare-{label}")),
                    services: vec![service],
                    did_profile: anp::authentication::DidProfile::E1,
                    ..Default::default()
                },
            )
            .unwrap();
            let did = bundle.did().unwrap().to_owned();
            Self {
                did,
                document: bundle.did_document.clone(),
                key1_private_pem: bundle.private_key_pem("key-1").unwrap().to_owned(),
                agreement_private_pem: bundle.private_key_pem("key-3").unwrap().to_owned(),
            }
        }
    }

    fn write_identity_fixture(root: &Path, alias: &str, identity: &TestIdentity) {
        let identity_root = root.join("identities");
        let identity_dir = identity_root.join(alias);
        fs::create_dir_all(&identity_dir).unwrap();
        fs::write(identity_root.join("default"), format!("{alias}\n")).unwrap();
        fs::write(
            identity_root.join("registry.json"),
            json!({
                "default_identity": alias,
                "identities": [{
                    "id": "alice-id",
                    "did": identity.did,
                    "local_alias": alias,
                    "ready_for_auth": true,
                    "ready_for_messaging": true,
                    "missing": []
                }]
            })
            .to_string(),
        )
        .unwrap();
        fs::write(
            identity_dir.join("did.json"),
            serde_json::to_vec_pretty(&identity.document).unwrap(),
        )
        .unwrap();
        fs::write(identity_dir.join("private.key"), &identity.key1_private_pem).unwrap();
        fs::write(
            identity_dir.join("e2ee-agreement-private.pem"),
            &identity.agreement_private_pem,
        )
        .unwrap();
        fs::write(
            identity_dir.join("auth.json"),
            r#"{"jwt_token":"test-token"}"#,
        )
        .unwrap();
    }

    fn unique_temp_root(prefix: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()))
    }
}
