use serde_json::Value;

use crate::internal::auth::session::{AsyncSessionProvider, SessionProvider};
use crate::internal::transport::{AsyncAuthenticatedRpcTransport, AuthenticatedRpcTransport};

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

impl<'a, P, T> GroupLifecycleRuntime<'a, P, T> {
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

impl<'a, P, T> GroupLifecycleRuntime<'a, P, T>
where
    P: SessionProvider,
    T: AuthenticatedRpcTransport,
{
    pub(crate) fn create(
        mut self,
        request: crate::groups::GroupCreateRequest,
        credentials: Option<GroupLifecycleCredentials>,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        self.ensure_group_session()?;
        let service_did = self
            .client
            .core_inner()
            .sdk_config()
            .anp_service_did
            .as_ref()
            .ok_or_else(|| {
                crate::ImError::invalid_input(
                    Some("anp_service_did".to_string()),
                    "group create requires ImCoreConfig.anp_service_did",
                )
            })?;
        let payload = crate::internal::wire::group::build_group_create_payload(
            self.client.did().as_str(),
            &request,
            service_did,
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

    pub(crate) fn rebind_member(
        mut self,
        request: crate::groups::GroupRebindMemberRequest,
        credentials: Option<GroupLifecycleCredentials>,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        self.ensure_group_session()?;
        let payload = crate::internal::wire::group::build_group_rebind_member_payload(
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
                verification_method: None,
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
        Ok(crate::groups::GroupReadResult::from_raw_response(
            raw,
            Vec::new(),
        ))
    }
}

impl<'a, P, T> GroupLifecycleRuntime<'a, P, T>
where
    P: AsyncSessionProvider,
    T: AsyncAuthenticatedRpcTransport,
{
    pub(crate) async fn create_async(
        mut self,
        request: crate::groups::GroupCreateRequest,
        credentials: Option<GroupLifecycleCredentials>,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        self.ensure_group_session_async().await?;
        let service_did = self
            .client
            .core_inner()
            .sdk_config()
            .anp_service_did
            .as_ref()
            .ok_or_else(|| {
                crate::ImError::invalid_input(
                    Some("anp_service_did".to_string()),
                    "group create requires ImCoreConfig.anp_service_did",
                )
            })?;
        let payload = crate::internal::wire::group::build_group_create_payload(
            self.client.did().as_str(),
            &request,
            service_did,
        )?;
        self.signed_group_rpc_async(payload, credentials).await
    }

    pub(crate) async fn join_async(
        mut self,
        request: crate::groups::GroupJoinRequest,
        credentials: Option<GroupLifecycleCredentials>,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        self.ensure_group_session_async().await?;
        let payload = crate::internal::wire::group::build_group_join_payload(
            self.client.did().as_str(),
            &request,
        )?;
        self.signed_group_rpc_async(payload, credentials).await
    }

    pub(crate) async fn leave_async(
        mut self,
        request: crate::groups::GroupLeaveRequest,
        credentials: Option<GroupLifecycleCredentials>,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        self.ensure_group_session_async().await?;
        let payload = crate::internal::wire::group::build_group_leave_payload(
            self.client.did().as_str(),
            &request,
        )?;
        self.signed_group_rpc_async(payload, credentials).await
    }

    pub(crate) async fn add_member_async(
        mut self,
        request: crate::groups::GroupMemberMutationRequest,
        credentials: Option<GroupLifecycleCredentials>,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        self.ensure_group_session_async().await?;
        let payload = crate::internal::wire::group::build_group_add_member_payload(
            self.client.did().as_str(),
            &request,
        )?;
        self.signed_group_rpc_async(payload, credentials).await
    }

    pub(crate) async fn remove_member_async(
        mut self,
        request: crate::groups::GroupMemberMutationRequest,
        credentials: Option<GroupLifecycleCredentials>,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        self.ensure_group_session_async().await?;
        let payload = crate::internal::wire::group::build_group_remove_member_payload(
            self.client.did().as_str(),
            &request,
        )?;
        self.signed_group_rpc_async(payload, credentials).await
    }

    pub(crate) async fn rebind_member_async(
        mut self,
        request: crate::groups::GroupRebindMemberRequest,
        credentials: Option<GroupLifecycleCredentials>,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        self.ensure_group_session_async().await?;
        let payload = crate::internal::wire::group::build_group_rebind_member_payload(
            self.client.did().as_str(),
            &request,
        )?;
        self.signed_group_rpc_async(payload, credentials).await
    }

    pub(crate) async fn rebind_member_with_operation_id_async(
        mut self,
        request: crate::groups::GroupRebindMemberRequest,
        operation_id: &str,
        credentials: Option<GroupLifecycleCredentials>,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        self.ensure_group_session_async().await?;
        let mut payload = crate::internal::wire::group::build_group_rebind_member_payload(
            self.client.did().as_str(),
            &request,
        )?;
        let operation_id = operation_id.trim();
        if operation_id.is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("operation_id".to_owned()),
                "durable rebind operation ID is required",
            ));
        }
        payload.meta["operation_id"] = serde_json::Value::String(operation_id.to_owned());
        self.signed_group_rpc_async(payload, credentials).await
    }

    pub(crate) async fn update_profile_async(
        mut self,
        request: crate::groups::GroupUpdateProfileRequest,
        credentials: Option<GroupLifecycleCredentials>,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        self.ensure_group_session_async().await?;
        let payload = crate::internal::wire::group::build_group_update_profile_payload(
            self.client.did().as_str(),
            &request,
        )?;
        self.signed_group_rpc_async(payload, credentials).await
    }

    pub(crate) async fn update_policy_async(
        mut self,
        request: crate::groups::GroupUpdatePolicyRequest,
        credentials: Option<GroupLifecycleCredentials>,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        self.ensure_group_session_async().await?;
        let payload = crate::internal::wire::group::build_group_update_policy_payload(
            self.client.did().as_str(),
            &request,
        )?;
        self.signed_group_rpc_async(payload, credentials).await
    }

    async fn ensure_group_session_async(&self) -> crate::ImResult<crate::auth::SessionBundle> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::GroupMessaging)
            .await
    }

    async fn signed_group_rpc_async(
        &mut self,
        payload: crate::internal::wire::direct::DirectPayload,
        credentials: Option<GroupLifecycleCredentials>,
    ) -> crate::ImResult<crate::groups::GroupReadResult> {
        let credentials = match credentials {
            Some(credentials) => credentials,
            None => load_credentials_async(self.client).await?,
        };
        let origin_proof = crate::internal::proof::origin::build_origin_proof(
            &crate::internal::proof::origin::OriginProofIdentity {
                identity_name: credentials.identity_name,
                did_document: credentials.did_document,
                key1_private_pem: credentials.key1_private_pem,
                verification_method: None,
            },
            &payload,
        )?;
        let params = serde_json::json!({
            "meta": payload.meta,
            "auth": crate::internal::proof::origin::origin_auth_value(&origin_proof),
            "body": payload.body,
        });
        let raw = self
            .transport
            .authenticated_rpc(MESSAGE_RPC_ENDPOINT, payload.method.as_str(), params)
            .await?;
        Ok(crate::groups::GroupReadResult::from_raw_response(
            raw,
            Vec::new(),
        ))
    }
}

fn load_credentials(client: &crate::core::ImClient) -> crate::ImResult<GroupLifecycleCredentials> {
    let runtime = client.runtime();
    let did_document = runtime.key_provider.optional_did_document()?;
    let key1_private_pem = runtime.key_provider.device_request_signing_private_pem()?;
    Ok(GroupLifecycleCredentials {
        identity_name: runtime.owner.identity_id.as_str().to_string(),
        did_document,
        key1_private_pem,
    })
}

async fn load_credentials_async(
    client: &crate::core::ImClient,
) -> crate::ImResult<GroupLifecycleCredentials> {
    let runtime = client.runtime();
    let did_document = runtime.key_provider.optional_did_document()?;
    let key1_private_pem = runtime.key_provider.device_request_signing_private_pem()?;
    Ok(GroupLifecycleCredentials {
        identity_name: runtime.owner.identity_id.as_str().to_string(),
        did_document,
        key1_private_pem,
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
                creator_handle: None,
                description: Some(" group description ".to_string()),
                avatar_uri: Some(" https://example.test/group.png ".to_string()),
                discoverability: Some(crate::groups::GroupDiscoverability::Public),
                admission_mode: Some(crate::groups::GroupAdmissionMode::OpenJoin),
                message_security_profile: Some(
                    crate::groups::GroupMessageSecurityProfile::TransportProtected,
                ),
                security: crate::groups::GroupSecurityRequirement::Default,
                e2ee: false,
                slug: Some(" demo ".to_string()),
                goal: Some("ship".to_string()),
                rules: Some("be kind".to_string()),
                message_prompt: Some("reply clearly".to_string()),
                doc_url: Some("https://example.test/group".to_string()),
                attachments_allowed: Some(true),
                max_members: Some(crate::groups::GroupMemberLimit::new(500).unwrap()),
                member_max_messages: Some(25),
                member_max_total_chars: Some(2048),
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
                member_handle: None,
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
            crate::groups::GroupLeaveRequest {
                group,
                reason_text: None,
                security: crate::groups::GroupSecurityRequirement::Default,
            },
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
            calls[0].params["body"]["group_profile"]["avatar_uri"],
            "https://example.test/group.png"
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

    #[tokio::test]
    async fn group_lifecycle_runtime_builds_create_join_and_leave_rpc_async() {
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
        .create_async(
            crate::groups::GroupCreateRequest {
                name: "  Demo Group  ".to_string(),
                creator_handle: None,
                description: Some(" group description ".to_string()),
                avatar_uri: Some(" https://example.test/group.png ".to_string()),
                discoverability: Some(crate::groups::GroupDiscoverability::Public),
                admission_mode: Some(crate::groups::GroupAdmissionMode::OpenJoin),
                message_security_profile: Some(
                    crate::groups::GroupMessageSecurityProfile::TransportProtected,
                ),
                security: crate::groups::GroupSecurityRequirement::Default,
                e2ee: false,
                slug: Some(" demo ".to_string()),
                goal: Some("ship".to_string()),
                rules: Some("be kind".to_string()),
                message_prompt: Some("reply clearly".to_string()),
                doc_url: Some("https://example.test/group".to_string()),
                attachments_allowed: Some(true),
                max_members: Some(crate::groups::GroupMemberLimit::new(500).unwrap()),
                member_max_messages: Some(25),
                member_max_total_chars: Some(2048),
            },
            Some(credentials.clone()),
        )
        .await
        .unwrap();

        GroupLifecycleRuntime::new(
            &client,
            ReadyGroupSessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({"group_did":"did:example:group"}),
            },
        )
        .join_async(
            crate::groups::GroupJoinRequest {
                group: group.clone(),
                member_handle: None,
                reason_text: Some("  hello  ".to_string()),
            },
            Some(credentials.clone()),
        )
        .await
        .unwrap();

        GroupLifecycleRuntime::new(
            &client,
            ReadyGroupSessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({"left":true}),
            },
        )
        .leave_async(
            crate::groups::GroupLeaveRequest {
                group,
                reason_text: None,
                security: crate::groups::GroupSecurityRequirement::Default,
            },
            Some(credentials),
        )
        .await
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
            calls[0].params["body"]["group_profile"]["avatar_uri"],
            "https://example.test/group.png"
        );
        assert_eq!(
            calls[0].params["body"]["group_policy"]["message_security_profile"],
            "transport-protected"
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

    #[tokio::test]
    async fn group_lifecycle_async_requires_service_did_for_create() {
        let fixture = Fixture::new();
        let client = fixture.client_with_service_did(None);
        let calls = Rc::new(RefCell::new(Vec::new()));

        let error = GroupLifecycleRuntime::new(
            &client,
            ReadyGroupSessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({"group_did":"did:example:group"}),
            },
        )
        .create_async(
            crate::groups::GroupCreateRequest {
                name: "Demo Group".to_string(),
                creator_handle: None,
                description: None,
                avatar_uri: None,
                discoverability: None,
                admission_mode: None,
                message_security_profile: None,
                security: crate::groups::GroupSecurityRequirement::Default,
                e2ee: false,
                slug: None,
                goal: None,
                rules: None,
                message_prompt: None,
                doc_url: None,
                attachments_allowed: None,
                max_members: None,
                member_max_messages: None,
                member_max_total_chars: None,
            },
            Some(fixture.credentials()),
        )
        .await
        .unwrap_err();

        match error {
            crate::ImError::InvalidInput { field, .. } => {
                assert_eq!(field.as_deref(), Some("anp_service_did"));
            }
            other => panic!("expected invalid input for service DID, got {other:?}"),
        }
        assert!(calls.borrow().is_empty());
    }

    #[test]
    fn group_lifecycle_rejects_group_e2ee_create_until_phase6() {
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
                creator_handle: None,
                description: None,
                avatar_uri: None,
                discoverability: None,
                admission_mode: None,
                message_security_profile: None,
                security: crate::groups::GroupSecurityRequirement::Required,
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
            },
            Some(fixture.credentials()),
        )
        .unwrap();

        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].method, "group.create");
        assert_eq!(
            calls[0].params["body"]["group_policy"]["message_security_profile"],
            "group-e2ee"
        );
        assert_eq!(
            calls[0].params["body"]["group_policy"]["bootstrap_security_profile"],
            "group-e2ee"
        );
    }

    #[test]
    fn group_lifecycle_rejects_group_e2ee_security_profile_until_phase6() {
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
                creator_handle: None,
                description: None,
                avatar_uri: None,
                discoverability: None,
                admission_mode: None,
                message_security_profile: Some(
                    crate::groups::GroupMessageSecurityProfile::GroupE2ee,
                ),
                security: crate::groups::GroupSecurityRequirement::Default,
                e2ee: false,
                slug: None,
                goal: None,
                rules: None,
                message_prompt: None,
                doc_url: None,
                attachments_allowed: None,
                max_members: None,
                member_max_messages: None,
                member_max_total_chars: None,
            },
            Some(fixture.credentials()),
        )
        .unwrap();

        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].method, "group.create");
        assert_eq!(
            calls[0].params["body"]["group_policy"]["message_security_profile"],
            "group-e2ee"
        );
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
                member: crate::groups::GroupMemberRef::from(member.clone()),
                role: Some(crate::groups::GroupMemberRole::Admin),
                reason_text: Some(" invite ".to_string()),
                leave_request_id: None,
                security: crate::groups::GroupSecurityRequirement::Default,
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
                member: crate::groups::GroupMemberRef::from(member),
                role: Some(crate::groups::GroupMemberRole::Custom(
                    "ignored".to_string(),
                )),
                reason_text: Some(" cleanup ".to_string()),
                leave_request_id: None,
                security: crate::groups::GroupSecurityRequirement::Default,
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
                    avatar_uri: Some(" https://example.test/new.png ".to_string()),
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
                    admission_mode: Some(crate::groups::GroupAdmissionMode::InviteOnly),
                    attachments_allowed: Some(false),
                    max_members: Some(crate::groups::GroupMemberLimit::new(25).unwrap()),
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
            json!({
                "display_name": "Renamed",
                "description": "updated",
                "avatar_uri": "https://example.test/new.png",
            })
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
                bearer_token: None,
            })
        }

        fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
            unreachable!("group lifecycle runtime should not refresh through the session provider")
        }

        fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
            unreachable!("group lifecycle runtime should not read status")
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
            unreachable!("group lifecycle runtime should not refresh through the session provider")
        }

        async fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
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
            self.client_with_service_did(Some(
                crate::ids::Did::parse("did:example:service").unwrap(),
            ))
        }

        fn client_with_service_did(
            &self,
            anp_service_did: Option<crate::ids::Did>,
        ) -> crate::core::ImClient {
            crate::core::ImCore::new(
                crate::ImCoreConfig {
                    service_base_url: crate::ServiceEndpoint::parse("https://example.test")
                        .unwrap(),
                    did_domain: "awiki.test".to_string(),
                    user_service_endpoint: None,
                    message_service_endpoint: None,
                    mail_service_endpoint: None,
                    anp_service_endpoint: None,
                    anp_service_did,
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
