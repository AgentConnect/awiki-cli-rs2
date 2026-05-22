use serde_json::Value;

use crate::internal::auth::session::SessionProvider;
use crate::internal::transport::AuthenticatedRpcTransport;

pub(crate) const MESSAGE_RPC_ENDPOINT: &str = "/im/rpc";

pub(crate) struct GroupLifecycleRuntime<'a, P, T> {
    client: &'a crate::core::ImClient,
    session_provider: P,
    transport: T,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GroupLifecycleCredentials {
    pub identity_name: String,
    pub did_document: Option<Value>,
    pub key1_private_pem: String,
}

impl<'a, P, T> GroupLifecycleRuntime<'a, P, T>
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

    pub(crate) fn create(
        mut self,
        request: crate::groups::GroupCreateRequest,
        credentials: Option<GroupLifecycleCredentials>,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        self.ensure_group_session()?;
        let payload = crate::internal::wire::group::build_group_create_payload(
            self.client.did().as_str(),
            &request,
        )?;
        self.signed_group_rpc(payload, credentials)
    }

    pub(crate) fn join(
        mut self,
        request: crate::groups::GroupJoinRequest,
        credentials: Option<GroupLifecycleCredentials>,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        self.ensure_group_session()?;
        let payload = crate::internal::wire::group::build_group_join_payload(
            self.client.did().as_str(),
            &request,
        )?;
        self.signed_group_rpc(payload, credentials)
    }

    pub(crate) fn leave(
        mut self,
        request: crate::groups::GroupLeaveRequest,
        credentials: Option<GroupLifecycleCredentials>,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        self.ensure_group_session()?;
        let payload = crate::internal::wire::group::build_group_leave_payload(
            self.client.did().as_str(),
            &request,
        )?;
        self.signed_group_rpc(payload, credentials)
    }

    pub(crate) fn add_member(
        mut self,
        request: crate::groups::GroupMemberMutationRequest,
        credentials: Option<GroupLifecycleCredentials>,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        self.ensure_group_session()?;
        let payload = crate::internal::wire::group::build_group_add_member_payload(
            self.client.did().as_str(),
            &request,
        )?;
        self.signed_group_rpc(payload, credentials)
    }

    pub(crate) fn remove_member(
        mut self,
        request: crate::groups::GroupMemberMutationRequest,
        credentials: Option<GroupLifecycleCredentials>,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        self.ensure_group_session()?;
        let payload = crate::internal::wire::group::build_group_remove_member_payload(
            self.client.did().as_str(),
            &request,
        )?;
        self.signed_group_rpc(payload, credentials)
    }

    pub(crate) fn update_profile(
        mut self,
        request: crate::groups::GroupUpdateProfileRequest,
        credentials: Option<GroupLifecycleCredentials>,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        self.ensure_group_session()?;
        let payload = crate::internal::wire::group::build_group_update_profile_payload(
            self.client.did().as_str(),
            &request,
        )?;
        self.signed_group_rpc(payload, credentials)
    }

    pub(crate) fn update_policy(
        mut self,
        request: crate::groups::GroupUpdatePolicyRequest,
        credentials: Option<GroupLifecycleCredentials>,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        self.ensure_group_session()?;
        let payload = crate::internal::wire::group::build_group_update_policy_payload(
            self.client.did().as_str(),
            &request,
        )?;
        self.signed_group_rpc(payload, credentials)
    }

    fn ensure_group_session(&self) -> crate::ImResult<crate::auth::SessionBundle> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::GroupMessaging)
    }

    fn signed_group_rpc(
        &mut self,
        payload: crate::internal::wire::direct::DirectPayload,
        credentials: Option<GroupLifecycleCredentials>,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        let credentials = match credentials {
            Some(credentials) => credentials,
            None => load_credentials(self.client)?,
        };
        let origin_proof = crate::internal::proof::origin::build_origin_proof(
            &crate::internal::proof::origin::OriginProofIdentity {
                identity_name: credentials.identity_name,
                did_document: credentials.did_document,
                key1_private_pem: credentials.key1_private_pem,
            },
            &payload,
        )?;
        let params = serde_json::json!({
            "meta": payload.meta,
            "auth": crate::internal::proof::origin::origin_auth_value(&origin_proof),
            "body": payload.body,
        });
        let raw = self.transport.authenticated_rpc(
            MESSAGE_RPC_ENDPOINT,
            payload.method.as_str(),
            params,
        )?;
        Ok(crate::groups::GroupReadResult::from_diagnostic_raw(
            raw,
            Vec::new(),
        ))
    }
}

fn load_credentials(client: &crate::core::ImClient) -> crate::ImResult<GroupLifecycleCredentials> {
    let runtime = client.runtime();
    let did_document = read_optional_json(&runtime.did_document_path)?;
    let key1_private_pem = std::fs::read_to_string(&runtime.private_key_path).map_err(|err| {
        crate::ImError::CredentialFileUnreadable {
            path_kind: "private_key".to_string(),
            detail: err.to_string(),
        }
    })?;
    Ok(GroupLifecycleCredentials {
        identity_name: runtime.owner.identity_id.as_str().to_string(),
        did_document,
        key1_private_pem,
    })
}

fn read_optional_json(path: &std::path::Path) -> crate::ImResult<Option<Value>> {
    let raw = match std::fs::read(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(crate::ImError::CredentialFileUnreadable {
                path_kind: "did_document".to_string(),
                detail: err.to_string(),
            });
        }
    };
    serde_json::from_slice(&raw)
        .map(Some)
        .map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })
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
    fn group_lifecycle_runtime_builds_create_join_and_leave_rpc() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let credentials = fixture.credentials();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let group = crate::ids::GroupRef::parse("did:example:group").unwrap();

        GroupLifecycleRuntime::new(
            &client,
            ReadyGroupSessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({"group_did":"did:example:group"}),
            },
        )
        .create(
            crate::groups::GroupCreateRequest {
                name: "  Demo Group  ".to_string(),
                description: Some(" group description ".to_string()),
                discoverability: Some(" public ".to_string()),
                admission_mode: Some(" open-join ".to_string()),
                message_security_profile: Some("transport-protected".to_string()),
                e2ee: false,
                slug: Some(" demo ".to_string()),
                goal: Some("ship".to_string()),
                rules: Some("be kind".to_string()),
                message_prompt: Some("reply clearly".to_string()),
                doc_url: Some("https://example.test/group".to_string()),
                attachments_allowed: Some(true),
                max_members: Some("500".to_string()),
                member_max_messages: Some(25),
                member_max_total_chars: Some(2048),
                service_did: crate::ids::Did::parse("did:example:service").unwrap(),
            },
            Some(credentials.clone()),
        )
        .unwrap();

        GroupLifecycleRuntime::new(
            &client,
            ReadyGroupSessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({"group_did":"did:example:group"}),
            },
        )
        .join(
            crate::groups::GroupJoinRequest {
                group: group.clone(),
                reason_text: Some("  hello  ".to_string()),
            },
            Some(credentials.clone()),
        )
        .unwrap();

        GroupLifecycleRuntime::new(
            &client,
            ReadyGroupSessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({"left":true}),
            },
        )
        .leave(
            crate::groups::GroupLeaveRequest { group },
            Some(credentials),
        )
        .unwrap();

        let calls = calls.borrow();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0].method, "group.create");
        assert_eq!(calls[0].endpoint, MESSAGE_RPC_ENDPOINT);
        assert_eq!(calls[0].params["meta"]["profile"], "anp.group.base.v1");
        assert_eq!(
            calls[0].params["meta"]["target"],
            json!({"kind":"service","did":"did:example:service"})
        );
        assert_eq!(
            calls[0].params["body"]["group_profile"]["display_name"],
            "Demo Group"
        );
        assert_eq!(
            calls[0].params["body"]["group_policy"]["message_security_profile"],
            "transport-protected"
        );
        assert_eq!(
            calls[0].params["body"]["group_policy"]["permissions"]["update_policy"],
            "owner"
        );
        assert_eq!(
            calls[0].params["auth"]["scheme"],
            crate::internal::proof::origin::ORIGIN_PROOF_SCHEME
        );
        assert_eq!(calls[1].method, "group.join");
        assert_eq!(calls[1].params["body"]["reason_text"], "hello");
        assert_eq!(
            calls[1].params["meta"]["target"],
            json!({"kind":"group","did":"did:example:group"})
        );
        assert_eq!(calls[2].method, "group.leave");
        assert_eq!(calls[2].params["body"], json!({}));
    }

    #[test]
    fn group_lifecycle_create_default_policy_and_e2ee_security_profile() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));

        GroupLifecycleRuntime::new(
            &client,
            ReadyGroupSessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({"group_did":"did:example:e2ee"}),
            },
        )
        .create(
            crate::groups::GroupCreateRequest {
                name: "Secure Group".to_string(),
                description: None,
                discoverability: None,
                admission_mode: None,
                message_security_profile: None,
                e2ee: true,
                slug: None,
                goal: None,
                rules: None,
                message_prompt: None,
                doc_url: None,
                attachments_allowed: None,
                max_members: None,
                member_max_messages: None,
                member_max_total_chars: None,
                service_did: crate::ids::Did::parse("did:example:service").unwrap(),
            },
            Some(fixture.credentials()),
        )
        .unwrap();

        let calls = calls.borrow();
        let policy = &calls[0].params["body"]["group_policy"];
        assert_eq!(policy["admission_mode"], "open-join");
        assert_eq!(policy["attachments_allowed"], true);
        assert_eq!(policy["max_members"], "500");
        assert_eq!(policy["message_security_profile"], "group-e2ee");
        assert_eq!(policy["bootstrap_security_profile"], "group-e2ee");
    }

    #[test]
    fn group_mutation_runtime_builds_add_remove_and_update_rpc() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let credentials = fixture.credentials();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let group = crate::ids::GroupRef::parse("did:example:group").unwrap();
        let member = crate::ids::Did::parse("did:example:bob").unwrap();

        GroupLifecycleRuntime::new(
            &client,
            ReadyGroupSessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({"accepted":true}),
            },
        )
        .add_member(
            crate::groups::GroupMemberMutationRequest {
                group: group.clone(),
                member: member.clone(),
                role: Some(" admin ".to_string()),
                reason_text: Some(" invite ".to_string()),
            },
            Some(credentials.clone()),
        )
        .unwrap();

        GroupLifecycleRuntime::new(
            &client,
            ReadyGroupSessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({"accepted":true}),
            },
        )
        .remove_member(
            crate::groups::GroupMemberMutationRequest {
                group: group.clone(),
                member,
                role: Some("ignored".to_string()),
                reason_text: Some(" cleanup ".to_string()),
            },
            Some(credentials.clone()),
        )
        .unwrap();

        GroupLifecycleRuntime::new(
            &client,
            ReadyGroupSessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({"accepted":true}),
            },
        )
        .update_profile(
            crate::groups::GroupUpdateProfileRequest {
                group: group.clone(),
                patch: crate::groups::GroupProfilePatch {
                    name: Some(" Renamed ".to_string()),
                    description: Some(" updated ".to_string()),
                    ..crate::groups::GroupProfilePatch::default()
                },
            },
            Some(credentials.clone()),
        )
        .unwrap();

        GroupLifecycleRuntime::new(
            &client,
            ReadyGroupSessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({"accepted":true}),
            },
        )
        .update_policy(
            crate::groups::GroupUpdatePolicyRequest {
                group,
                patch: crate::groups::GroupPolicyPatch {
                    admission_mode: Some(" invite-only ".to_string()),
                    attachments_allowed: Some(false),
                    max_members: Some(" 25 ".to_string()),
                    member_max_messages: Some(5),
                    member_max_total_chars: Some(4096),
                },
            },
            Some(credentials),
        )
        .unwrap();

        let calls = calls.borrow();
        assert_eq!(calls.len(), 4);
        assert_eq!(calls[0].method, "group.add");
        assert_eq!(calls[0].endpoint, MESSAGE_RPC_ENDPOINT);
        assert_eq!(
            calls[0].params["meta"]["target"],
            json!({"kind":"group","did":"did:example:group"})
        );
        assert_eq!(calls[0].params["body"]["member_did"], "did:example:bob");
        assert_eq!(calls[0].params["body"]["role"], "admin");
        assert_eq!(calls[0].params["body"]["reason_text"], "invite");
        assert_eq!(
            calls[0].params["auth"]["scheme"],
            crate::internal::proof::origin::ORIGIN_PROOF_SCHEME
        );
        assert_eq!(calls[1].method, "group.remove");
        assert_eq!(calls[1].params["body"]["member_did"], "did:example:bob");
        assert_eq!(calls[1].params["body"]["reason_text"], "cleanup");
        assert!(calls[1].params["body"].get("role").is_none());
        assert_eq!(calls[2].method, "group.update_profile");
        assert_eq!(
            calls[2].params["body"]["group_profile_patch"],
            json!({"display_name":"Renamed","description":"updated"})
        );
        assert_eq!(calls[3].method, "group.update_policy");
        let policy = &calls[3].params["body"]["group_policy_patch"];
        assert_eq!(policy["admission_mode"], "invite-only");
        assert_eq!(policy["attachments_allowed"], false);
        assert_eq!(policy["max_members"], "25");
        assert_eq!(policy["member_max_messages"], 5);
        assert_eq!(policy["member_max_total_chars"], 4096);
        assert_eq!(policy["message_security_profile"], "transport-protected");
        assert_eq!(policy["permissions"]["update_policy"], "owner");
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
            unreachable!("group lifecycle runtime should not refresh through the session provider")
        }

        fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
            unreachable!("group lifecycle runtime should not read status")
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

        fn credentials(&self) -> GroupLifecycleCredentials {
            let bundle = anp::authentication::create_did_wba_document(
                "awiki.test",
                anp::authentication::DidDocumentOptions {
                    path_segments: vec!["user".to_string()],
                    domain: Some("awiki.test".to_string()),
                    challenge: Some("group-lifecycle-test".to_string()),
                    ..anp::authentication::DidDocumentOptions::default()
                },
            )
            .unwrap();
            let key1_private_pem = bundle.private_key_pem("key-1").unwrap().to_string();
            GroupLifecycleCredentials {
                identity_name: "alice".to_string(),
                did_document: Some(bundle.did_document),
                key1_private_pem,
            }
        }
    }

    fn unique_temp_root() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "im-core-group-lifecycle-runtime-{}-{nanos}",
            std::process::id()
        ))
    }
}
