use anp::group_e2ee::operations::EncryptInput;
use anp::group_e2ee::{GroupApplicationPlaintext, GroupStateRef};

use crate::internal::auth::session::SessionProvider;
use crate::internal::message_runtime::group::{
    content_type_for_message_type, group_target, load_credentials, message_type,
    sdk_result_from_group_result, text_body, GroupRpcResult, GroupTextCredentials,
};
use crate::internal::transport::AuthenticatedRpcTransport;

use super::provider::GroupMlsProvider;

pub(crate) struct GroupE2eeTextSender<'a, P, T, M> {
    client: &'a crate::core::ImClient,
    session_provider: P,
    transport: T,
    mls_provider: M,
}

pub(crate) struct GroupE2eeTextSend {
    pub request: crate::messages::SendMessageRequest,
    pub group_state_ref: GroupStateRef,
    pub credentials: Option<GroupTextCredentials>,
}

pub(crate) struct GroupE2eeTextSendResult {
    pub sdk_result: crate::messages::SendMessageResult,
    pub group_did: String,
    pub operation_id: String,
    pub message_id: String,
    pub raw: serde_json::Value,
}

impl<'a, P, T, M> GroupE2eeTextSender<'a, P, T, M>
where
    P: SessionProvider,
    T: AuthenticatedRpcTransport,
    M: GroupMlsProvider,
{
    pub(crate) fn new(
        client: &'a crate::core::ImClient,
        session_provider: P,
        transport: T,
        mls_provider: M,
    ) -> Self {
        Self {
            client,
            session_provider,
            transport,
            mls_provider,
        }
    }

    pub(crate) fn send(
        mut self,
        input: GroupE2eeTextSend,
    ) -> crate::ImResult<GroupE2eeTextSendResult> {
        let group = group_target(&input.request.target)?;
        let (text, kind) = text_body(&input.request.body)?;
        validate_group_e2ee_security(&input.request.security)?;
        self.session_provider
            .ensure_session(crate::auth::AuthScope::GroupMessaging)?;

        let operation_id = input
            .request
            .delivery
            .idempotency_key
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| {
                format!(
                    "op-{}",
                    crate::internal::wire::common::generate_operation_id()
                )
            });
        let message_id = input
            .request
            .client_message_id
            .as_ref()
            .map(|value| value.as_str().to_owned())
            .unwrap_or_else(|| {
                format!(
                    "msg-{}",
                    crate::internal::wire::common::generate_operation_id()
                )
            });
        let device_id = self
            .client
            .current_identity()
            .device_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(anp::group_e2ee::commands::DEVICE_ID_DEFAULT)
            .to_owned();
        let content_type = content_type_for_message_type(message_type(&kind));
        let encrypted = self.mls_provider.encrypt(EncryptInput {
            sender_did: self.client.did().as_str().to_owned(),
            device_id,
            group_state_ref: input.group_state_ref,
            message_id: message_id.clone(),
            operation_id: operation_id.clone(),
            application_plaintext: GroupApplicationPlaintext {
                application_content_type: content_type.to_owned(),
                thread_id: Some(group.as_str().to_owned()),
                reply_to_message_id: None,
                annotations: Default::default(),
                text: Some(text.to_owned()),
                payload: None,
                payload_b64u: None,
            },
            request_id: format!("group-e2ee-encrypt-{operation_id}"),
        })?;
        let credentials = match input.credentials {
            Some(credentials) => credentials,
            None => load_credentials(self.client)?,
        };
        let params = super::wire::build_group_e2ee_send_rpc_params(
            &credentials,
            self.client.did().as_str(),
            group.as_str(),
            &encrypted.group_cipher_object,
            &operation_id,
            &message_id,
        )?;
        let raw = self.transport.authenticated_rpc(
            crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT,
            "group.e2ee.send",
            params,
        )?;
        let mut result: GroupRpcResult =
            serde_json::from_value(raw.clone()).map_err(|err| crate::ImError::Serialization {
                detail: err.to_string(),
            })?;
        if result.group_did.trim().is_empty() {
            result.group_did = group.as_str().to_owned();
        }
        if result.message_id.trim().is_empty() {
            result.message_id = message_id.clone();
        }
        if result.operation_id.trim().is_empty() {
            result.operation_id = operation_id.clone();
        }
        let sdk_result = sdk_result_from_group_result(
            &result,
            self.client.did().clone(),
            group.clone(),
            text,
            kind,
        )?;
        Ok(GroupE2eeTextSendResult {
            sdk_result,
            group_did: group.as_str().to_owned(),
            operation_id,
            message_id,
            raw,
        })
    }
}

fn validate_group_e2ee_security(
    security: &crate::messages::MessageSecurityMode,
) -> crate::ImResult<()> {
    match security {
        crate::messages::MessageSecurityMode::GroupE2ee => Ok(()),
        crate::messages::MessageSecurityMode::DefaultPlain
        | crate::messages::MessageSecurityMode::Plain => {
            Err(crate::ImError::unsupported("plain-group-e2ee-runtime"))
        }
        crate::messages::MessageSecurityMode::SecureDirect => {
            Err(crate::ImError::unsupported("secure-direct"))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::fs;
    use std::path::PathBuf;
    use std::rc::Rc;

    use anp::group_e2ee::operations::{
        AbortCommitInput, AbortCommitOutput, AddMemberInput, CreateGroupInput, DecryptInput,
        DecryptOutput, EncryptInput, EncryptOutput, FinalizeCommitInput, FinalizeCommitOutput,
        GenerateKeyPackageInput, GroupKeyPackageOutput, PreparedMlsCommitOutput,
        ProcessNoticeInput, ProcessNoticeOutput, ProcessWelcomeInput, ProcessWelcomeOutput,
        RecoverMemberInput, RemoveMemberInput, StatusInput, StatusOutput, UpdateMemberInput,
    };
    use anp::group_e2ee::{GroupCipherObject, GroupStateRef};
    use serde_json::{json, Value};

    use super::*;

    #[test]
    fn group_e2ee_text_sender_encrypts_then_sends_cipher_without_plaintext() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let group_did = "did:example:groups:e2ee";
        let sender = GroupE2eeTextSender::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({
                    "accepted": true,
                    "final_acceptance": true,
                    "group_did": group_did,
                    "message_id": "server-e2ee-message-id",
                    "operation_id": "op-client-e2ee",
                    "group_event_seq": "77",
                    "group_state_version": "11",
                    "accepted_at": "2026-05-21T00:00:00Z"
                }),
            },
            StaticMlsProvider,
        );

        let result = sender
            .send(GroupE2eeTextSend {
                request: group_e2ee_text_request(group_did, "secret group text"),
                group_state_ref: GroupStateRef {
                    group_did: group_did.to_owned(),
                    group_state_version: "10".to_owned(),
                    policy_hash: None,
                },
                credentials: Some(fixture.credentials()),
            })
            .unwrap();

        assert_eq!(result.group_did, group_did);
        assert_eq!(result.operation_id, "op-client-e2ee");
        assert_eq!(result.sdk_result.message.metadata.server_sequence, Some(77));
        assert!(matches!(
            result.sdk_result.delivery,
            crate::messages::DeliveryState::Accepted
        ));

        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        let call = &calls[0];
        assert_eq!(
            call.endpoint,
            crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT
        );
        assert_eq!(call.method, "group.e2ee.send");
        assert_eq!(call.params["meta"]["profile"], anp::group_e2ee::PROFILE);
        assert_eq!(call.params["meta"]["security_profile"], "group-e2ee");
        assert_eq!(
            call.params["meta"]["content_type"],
            anp::group_e2ee::commands::GROUP_CIPHER_CONTENT_TYPE
        );
        assert_eq!(call.params["meta"]["operation_id"], "op-client-e2ee");
        assert_eq!(
            call.params["body"]["private_message_b64u"],
            "c2VhbGVkLW1scy1jaXBoZXI"
        );
        let encoded = serde_json::to_string(&call.params).unwrap();
        assert!(!encoded.contains("secret group text"));
        assert!(!encoded.contains("application_plaintext"));
        assert!(encoded.contains("origin_proof"));
    }

    #[derive(Clone)]
    struct ReadySessionProvider;

    impl SessionProvider for ReadySessionProvider {
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
            unreachable!("group E2EE sender should not refresh through the session provider")
        }

        fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
            unreachable!("group E2EE sender should not read status")
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
                endpoint: endpoint.to_owned(),
                method: method.to_owned(),
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

    struct StaticMlsProvider;

    impl GroupMlsProvider for StaticMlsProvider {
        fn encrypt(&self, input: EncryptInput) -> crate::ImResult<EncryptOutput> {
            assert_eq!(input.sender_did, "did:example:alice");
            assert_eq!(input.operation_id, "op-client-e2ee");
            assert_eq!(input.message_id, "msg-client-e2ee");
            assert_eq!(
                input.application_plaintext.text.as_deref(),
                Some("secret group text")
            );
            Ok(EncryptOutput {
                group_cipher_object: GroupCipherObject {
                    crypto_group_id_b64u: "Y3J5cHRvLWdyb3Vw".to_owned(),
                    epoch: "10".to_owned(),
                    private_message_b64u: "c2VhbGVkLW1scy1jaXBoZXI".to_owned(),
                    group_state_ref: input.group_state_ref,
                    epoch_authenticator: Some("YXV0aGVudGljYXRvcg".to_owned()),
                    non_cryptographic: false,
                    artifact_mode: None,
                },
                authenticated_data_sha256_b64u: "YWFkLWRpZ2VzdA".to_owned(),
            })
        }

        fn generate_key_package(
            &self,
            _input: GenerateKeyPackageInput,
        ) -> crate::ImResult<GroupKeyPackageOutput> {
            unreachable!("send should not generate key packages")
        }

        fn create_group_prepare(
            &self,
            _input: CreateGroupInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("send should not create groups")
        }

        fn add_member_prepare(
            &self,
            _input: AddMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("send should not add members")
        }

        fn remove_member_prepare(
            &self,
            _input: RemoveMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("send should not remove members")
        }

        fn update_member_prepare(
            &self,
            _input: UpdateMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("send should not update members")
        }

        fn recover_member_prepare(
            &self,
            _input: RecoverMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("send should not recover members")
        }

        fn finalize_commit(
            &self,
            _input: FinalizeCommitInput,
        ) -> crate::ImResult<FinalizeCommitOutput> {
            unreachable!("send should not finalize commits")
        }

        fn abort_commit(&self, _input: AbortCommitInput) -> crate::ImResult<AbortCommitOutput> {
            unreachable!("send should not abort commits")
        }

        fn process_welcome(
            &self,
            _input: ProcessWelcomeInput,
        ) -> crate::ImResult<ProcessWelcomeOutput> {
            unreachable!("send should not process welcomes")
        }

        fn process_notice(
            &self,
            _input: ProcessNoticeInput,
        ) -> crate::ImResult<ProcessNoticeOutput> {
            unreachable!("send should not process notices")
        }

        fn decrypt(&self, _input: DecryptInput) -> crate::ImResult<DecryptOutput> {
            unreachable!("send should not decrypt")
        }

        fn status(&self, _input: StatusInput) -> crate::ImResult<StatusOutput> {
            unreachable!("send should not read status")
        }
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
                    did_domain: "awiki.test".to_owned(),
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
                "alice".to_owned(),
            ))
            .unwrap()
        }

        fn credentials(&self) -> GroupTextCredentials {
            let bundle = anp::authentication::create_did_wba_document(
                "awiki.test",
                anp::authentication::DidDocumentOptions {
                    path_segments: vec!["user".to_owned()],
                    domain: Some("awiki.test".to_owned()),
                    challenge: Some("group-e2ee-runtime-test".to_owned()),
                    ..anp::authentication::DidDocumentOptions::default()
                },
            )
            .unwrap();
            let key1_private_pem = bundle.private_key_pem("key-1").unwrap().to_owned();
            GroupTextCredentials {
                identity_name: "alice".to_owned(),
                did_document: Some(bundle.did_document),
                key1_private_pem,
            }
        }
    }

    fn group_e2ee_text_request(group: &str, text: &str) -> crate::messages::SendMessageRequest {
        crate::messages::SendMessageRequest {
            target: crate::messages::MessageTarget::Group(
                crate::ids::GroupRef::parse(group).unwrap(),
            ),
            body: crate::messages::MessageBody::Text {
                text: text.to_owned(),
                kind: crate::messages::MessageKind::Text,
            },
            security: crate::messages::MessageSecurityMode::GroupE2ee,
            client_message_id: Some(crate::ids::MessageId::parse("msg-client-e2ee").unwrap()),
            delivery: crate::messages::MessageDeliveryOptions {
                idempotency_key: Some("op-client-e2ee".to_owned()),
                wait_for_final_acceptance: true,
            },
        }
    }

    fn unique_temp_root() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "im-core-group-e2ee-runtime-{}-{nanos}",
            std::process::id()
        ))
    }
}
