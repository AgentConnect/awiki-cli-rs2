use anp::group_e2ee::operations::{StatusInput, StatusOutput};
use anp::group_e2ee::GroupStateRef;
use serde_json::Value;

use crate::internal::auth::session::SessionProvider;
use crate::internal::message_runtime::group::{load_credentials, GroupTextCredentials};
use crate::internal::transport::AuthenticatedRpcTransport;

use super::provider::GroupMlsProvider;
use super::DEFAULT_GROUP_MLS_DEVICE_ID;

pub(crate) struct GroupStateRefResolver<'a, P, T, M> {
    client: &'a crate::core::ImClient,
    session_provider: P,
    transport: T,
    mls_provider: M,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolveGroupStateRef {
    pub group: crate::ids::GroupRef,
    pub credentials: Option<GroupTextCredentials>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GroupStateRefSource {
    LocalCache,
    ServiceHead,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolveGroupStateRefResult {
    pub group_state_ref: GroupStateRef,
    pub source: GroupStateRefSource,
    pub mls_status: StatusOutput,
    pub service_head_raw: Option<Value>,
}

impl<'a, P, T, M> GroupStateRefResolver<'a, P, T, M>
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

    pub(crate) fn resolve(
        mut self,
        input: ResolveGroupStateRef,
    ) -> crate::ImResult<ResolveGroupStateRefResult> {
        resolve_group_state_ref(
            self.client,
            &self.session_provider,
            &mut self.transport,
            &self.mls_provider,
            input,
        )
    }
}

pub(crate) fn resolve_group_state_ref<P, T, M>(
    client: &crate::core::ImClient,
    session_provider: &P,
    transport: &mut T,
    mls_provider: &M,
    input: ResolveGroupStateRef,
) -> crate::ImResult<ResolveGroupStateRefResult>
where
    P: SessionProvider,
    T: AuthenticatedRpcTransport,
    M: GroupMlsProvider,
{
    session_provider.ensure_session(crate::auth::AuthScope::GroupMessaging)?;
    let group_did = require_non_empty_group(input.group.as_str())?;
    let device_id = device_id_for_client(client);
    let mls_status = mls_provider.status(StatusInput {
        request_id: format!(
            "group-e2ee-status-{}",
            crate::internal::wire::common::generate_operation_id()
        ),
        device_id,
        agent_did: Some(client.did().as_str().to_owned()),
        group_did: Some(group_did.to_owned()),
    })?;
    ensure_active_status(group_did, &mls_status)?;

    if let Some(group_state_ref) = local_group_state_ref(client, group_did) {
        return Ok(ResolveGroupStateRefResult {
            group_state_ref,
            source: GroupStateRefSource::LocalCache,
            mls_status,
            service_head_raw: None,
        });
    }

    let credentials = match input.credentials {
        Some(credentials) => credentials,
        None => load_credentials(client)?,
    };
    let params = super::wire::build_group_e2ee_head_rpc_params(
        &credentials,
        client.did().as_str(),
        group_did,
    )?;
    let service_head_raw = transport.authenticated_rpc(
        crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT,
        "group.e2ee.head",
        params,
    )?;
    let group_state_ref = group_state_ref_from_service_head(group_did, &service_head_raw)?;
    Ok(ResolveGroupStateRefResult {
        group_state_ref,
        source: GroupStateRefSource::ServiceHead,
        mls_status,
        service_head_raw: Some(service_head_raw),
    })
}

pub(crate) fn local_group_state_ref(
    client: &crate::core::ImClient,
    group_did: &str,
) -> Option<GroupStateRef> {
    let Ok(group_did) = require_non_empty_group(group_did) else {
        return None;
    };
    let snapshot = local_group_snapshot(client, group_did).ok().flatten()?;
    group_state_ref_from_snapshot(group_did, &snapshot)
}

pub(crate) fn group_state_ref_from_service_head(
    group_did: &str,
    service_head: &Value,
) -> crate::ImResult<GroupStateRef> {
    let group_did = require_non_empty_group(group_did)?;
    let version = first_non_empty_string(&[
        service_head
            .get("group_state_ref")
            .and_then(|value| value.get("group_state_version")),
        service_head.get("group_state_version"),
        service_head
            .get("delivery")
            .and_then(|value| value.get("group_state_version")),
        service_head
            .get("mls")
            .and_then(|value| value.get("group_state_ref"))
            .and_then(|value| value.get("group_state_version")),
    ]);
    if version.is_empty() {
        return Err(crate::ImError::LocalStateUnavailable {
            detail: "group E2EE service head did not include group_state_version".to_owned(),
        });
    }
    let policy_hash = first_non_empty_string(&[
        service_head
            .get("group_state_ref")
            .and_then(|value| value.get("policy_hash")),
        service_head.get("policy_hash"),
    ]);
    Ok(GroupStateRef {
        group_did: group_did.to_owned(),
        group_state_version: version,
        policy_hash: non_empty_string(policy_hash),
    })
}

fn ensure_active_status(group_did: &str, status: &StatusOutput) -> crate::ImResult<()> {
    if status.status.trim().eq_ignore_ascii_case("active") {
        return Ok(());
    }
    Err(crate::ImError::LocalStateUnavailable {
        detail: format!(
            "group E2EE local MLS state for {group_did} is not active: {}",
            status.status
        ),
    })
}

#[cfg(feature = "sqlite")]
fn local_group_snapshot(
    client: &crate::core::ImClient,
    group_did: &str,
) -> crate::ImResult<Option<Value>> {
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    crate::internal::local_state::groups::get_group_snapshot_for_owner_identity(
        &connection,
        client.current_identity().id.as_str(),
        client.did().as_str(),
        group_did,
    )
}

#[cfg(not(feature = "sqlite"))]
fn local_group_snapshot(
    _client: &crate::core::ImClient,
    _group_did: &str,
) -> crate::ImResult<Option<Value>> {
    Ok(None)
}

fn group_state_ref_from_snapshot(group_did: &str, snapshot: &Value) -> Option<GroupStateRef> {
    let metadata = decoded_metadata(snapshot);
    let version = first_non_empty_string(&[
        snapshot.get("group_state_version"),
        metadata
            .as_ref()
            .and_then(|value| value.get("group_state_version")),
        metadata
            .as_ref()
            .and_then(|value| value.get("group_e2ee"))
            .and_then(|value| value.get("group_state_version")),
    ]);
    if version.is_empty() {
        return None;
    }
    let policy_hash = first_non_empty_string(&[
        snapshot.get("policy_hash"),
        metadata.as_ref().and_then(|value| value.get("policy_hash")),
        metadata
            .as_ref()
            .and_then(|value| value.get("group_e2ee"))
            .and_then(|value| value.get("policy_hash")),
    ]);
    Some(GroupStateRef {
        group_did: group_did.to_owned(),
        group_state_version: version,
        policy_hash: non_empty_string(policy_hash),
    })
}

fn decoded_metadata(snapshot: &Value) -> Option<Value> {
    snapshot
        .get("metadata")
        .and_then(Value::as_str)
        .and_then(|value| serde_json::from_str::<Value>(value).ok())
        .filter(Value::is_object)
}

fn first_non_empty_string(values: &[Option<&Value>]) -> String {
    for value in values.iter().flatten() {
        match value {
            Value::String(text) if !text.trim().is_empty() => {
                return text.trim().to_owned();
            }
            Value::Number(number) => return number.to_string(),
            _ => {}
        }
    }
    String::new()
}

fn non_empty_string(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}

fn require_non_empty_group(group_did: &str) -> crate::ImResult<&str> {
    let group_did = group_did.trim();
    if group_did.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("group".to_owned()),
            "group target is required",
        ));
    }
    Ok(group_did)
}

fn device_id_for_client(client: &crate::core::ImClient) -> String {
    client
        .current_identity()
        .device_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(DEFAULT_GROUP_MLS_DEVICE_ID)
        .to_owned()
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
        GenerateKeyPackageInput, GroupKeyPackageOutput, LeaveGroupInput, PreparedMlsCommitOutput,
        ProcessNoticeInput, ProcessNoticeOutput, ProcessWelcomeInput, ProcessWelcomeOutput,
        RecoverMemberInput, RemoveMemberInput, StatusInput, StatusOutput, UpdateMemberInput,
    };
    use serde_json::{json, Value};

    use super::*;

    #[test]
    fn resolver_uses_local_group_snapshot_before_service_head() {
        let fixture = Fixture::new();
        let client = fixture.client();
        write_cached_group_snapshot(
            &client,
            "did:example:groups:e2ee",
            json!({
                "group_state_version": "42",
                "group_e2ee": {
                    "policy_hash": "sha256:policy"
                }
            }),
        );
        let calls = Rc::new(RefCell::new(Vec::new()));
        let resolver = GroupStateRefResolver::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({
                    "group_state_ref": {
                        "group_did": "did:example:groups:e2ee",
                        "group_state_version": "service-version"
                    }
                }),
            },
            ActiveStatusProvider,
        );

        let result = resolver
            .resolve(ResolveGroupStateRef {
                group: crate::ids::GroupRef::parse("did:example:groups:e2ee").unwrap(),
                credentials: Some(fixture.credentials()),
            })
            .unwrap();

        assert_eq!(result.source, GroupStateRefSource::LocalCache);
        assert_eq!(result.group_state_ref.group_did, "did:example:groups:e2ee");
        assert_eq!(result.group_state_ref.group_state_version, "42");
        assert_eq!(
            result.group_state_ref.policy_hash.as_deref(),
            Some("sha256:policy")
        );
        assert_eq!(result.mls_status.status, "active");
        assert!(result.service_head_raw.is_none());
        assert!(calls.borrow().is_empty());
    }

    #[test]
    fn resolver_falls_back_to_signed_service_head() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let resolver = GroupStateRefResolver::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({
                    "group_state_ref": {
                        "group_did": "did:example:groups:e2ee",
                        "group_state_version": "service-43",
                        "policy_hash": "sha256:remote-policy"
                    }
                }),
            },
            ActiveStatusProvider,
        );

        let result = resolver
            .resolve(ResolveGroupStateRef {
                group: crate::ids::GroupRef::parse("did:example:groups:e2ee").unwrap(),
                credentials: Some(fixture.credentials()),
            })
            .unwrap();

        assert_eq!(result.source, GroupStateRefSource::ServiceHead);
        assert_eq!(result.group_state_ref.group_state_version, "service-43");
        assert_eq!(
            result.group_state_ref.policy_hash.as_deref(),
            Some("sha256:remote-policy")
        );
        assert!(result.service_head_raw.is_some());
        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].endpoint,
            crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT
        );
        assert_eq!(calls[0].method, "group.e2ee.head");
        assert_eq!(calls[0].params["meta"]["profile"], anp::group_e2ee::PROFILE);
        assert_eq!(
            calls[0].params["meta"]["security_profile"],
            anp::group_e2ee::TRANSPORT_SECURITY_PROFILE
        );
        assert_eq!(
            calls[0].params["body"]["group_state_ref"],
            json!({"group_did": "did:example:groups:e2ee"})
        );
        assert!(serde_json::to_string(&calls[0].params)
            .unwrap()
            .contains("origin_proof"));
    }

    #[test]
    fn resolver_rejects_inactive_local_mls_state() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let resolver = GroupStateRefResolver::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::new(RefCell::new(Vec::new())),
                response: json!({}),
            },
            InactiveStatusProvider,
        );

        let result = resolver.resolve(ResolveGroupStateRef {
            group: crate::ids::GroupRef::parse("did:example:groups:e2ee").unwrap(),
            credentials: Some(fixture.credentials()),
        });

        assert!(matches!(
            result,
            Err(crate::ImError::LocalStateUnavailable { detail })
                if detail.contains("is not active")
        ));
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
            unreachable!("resolver should not refresh through the session provider")
        }

        fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
            unreachable!("resolver should not read status")
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

    struct ActiveStatusProvider;

    impl GroupMlsProvider for ActiveStatusProvider {
        fn status(&self, input: StatusInput) -> crate::ImResult<StatusOutput> {
            assert_eq!(input.agent_did.as_deref(), Some("did:example:alice"));
            assert_eq!(input.group_did.as_deref(), Some("did:example:groups:e2ee"));
            Ok(StatusOutput {
                status: "active".to_owned(),
                epoch: Some("7".to_owned()),
                local_epoch: Some("7".to_owned()),
                pending_commits: Vec::new(),
                epoch_authenticator: Some("auth".to_owned()),
            })
        }

        fn generate_key_package(
            &self,
            _input: GenerateKeyPackageInput,
        ) -> crate::ImResult<GroupKeyPackageOutput> {
            unreachable!("resolver should not generate key packages")
        }

        fn create_group_prepare(
            &self,
            _input: CreateGroupInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("resolver should not create groups")
        }

        fn add_member_prepare(
            &self,
            _input: AddMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("resolver should not add members")
        }

        fn remove_member_prepare(
            &self,
            _input: RemoveMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("resolver should not remove members")
        }

        fn leave_prepare(
            &self,
            _input: LeaveGroupInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("resolver should not leave groups")
        }

        fn update_member_prepare(
            &self,
            _input: UpdateMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("resolver should not update members")
        }

        fn recover_member_prepare(
            &self,
            _input: RecoverMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("resolver should not recover members")
        }

        fn finalize_commit(
            &self,
            _input: FinalizeCommitInput,
        ) -> crate::ImResult<FinalizeCommitOutput> {
            unreachable!("resolver should not finalize commits")
        }

        fn abort_commit(&self, _input: AbortCommitInput) -> crate::ImResult<AbortCommitOutput> {
            unreachable!("resolver should not abort commits")
        }

        fn process_welcome(
            &self,
            _input: ProcessWelcomeInput,
        ) -> crate::ImResult<ProcessWelcomeOutput> {
            unreachable!("resolver should not process welcomes")
        }

        fn process_notice(
            &self,
            _input: ProcessNoticeInput,
        ) -> crate::ImResult<ProcessNoticeOutput> {
            unreachable!("resolver should not process notices")
        }

        fn encrypt(&self, _input: EncryptInput) -> crate::ImResult<EncryptOutput> {
            unreachable!("resolver should not encrypt")
        }

        fn decrypt(&self, _input: DecryptInput) -> crate::ImResult<DecryptOutput> {
            unreachable!("resolver should not decrypt")
        }
    }

    struct InactiveStatusProvider;

    impl GroupMlsProvider for InactiveStatusProvider {
        fn status(&self, _input: StatusInput) -> crate::ImResult<StatusOutput> {
            Ok(StatusOutput {
                status: "missing".to_owned(),
                epoch: None,
                local_epoch: None,
                pending_commits: Vec::new(),
                epoch_authenticator: None,
            })
        }

        fn generate_key_package(
            &self,
            _input: GenerateKeyPackageInput,
        ) -> crate::ImResult<GroupKeyPackageOutput> {
            unreachable!("resolver should not generate key packages")
        }

        fn create_group_prepare(
            &self,
            _input: CreateGroupInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("resolver should not create groups")
        }

        fn add_member_prepare(
            &self,
            _input: AddMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("resolver should not add members")
        }

        fn remove_member_prepare(
            &self,
            _input: RemoveMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("resolver should not remove members")
        }

        fn leave_prepare(
            &self,
            _input: LeaveGroupInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("resolver should not leave groups")
        }

        fn update_member_prepare(
            &self,
            _input: UpdateMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("resolver should not update members")
        }

        fn recover_member_prepare(
            &self,
            _input: RecoverMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("resolver should not recover members")
        }

        fn finalize_commit(
            &self,
            _input: FinalizeCommitInput,
        ) -> crate::ImResult<FinalizeCommitOutput> {
            unreachable!("resolver should not finalize commits")
        }

        fn abort_commit(&self, _input: AbortCommitInput) -> crate::ImResult<AbortCommitOutput> {
            unreachable!("resolver should not abort commits")
        }

        fn process_welcome(
            &self,
            _input: ProcessWelcomeInput,
        ) -> crate::ImResult<ProcessWelcomeOutput> {
            unreachable!("resolver should not process welcomes")
        }

        fn process_notice(
            &self,
            _input: ProcessNoticeInput,
        ) -> crate::ImResult<ProcessNoticeOutput> {
            unreachable!("resolver should not process notices")
        }

        fn encrypt(&self, _input: EncryptInput) -> crate::ImResult<EncryptOutput> {
            unreachable!("resolver should not encrypt")
        }

        fn decrypt(&self, _input: DecryptInput) -> crate::ImResult<DecryptOutput> {
            unreachable!("resolver should not decrypt")
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
                    mail_service_endpoint: None,
                    message_service_endpoint: None,
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
                    challenge: Some("group-e2ee-state-ref-test".to_owned()),
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

    fn write_cached_group_snapshot(
        client: &crate::core::ImClient,
        group_did: &str,
        metadata: Value,
    ) {
        let connection = crate::internal::local_state::open_writable(
            &client.core_inner().sdk_paths().local_state.sqlite_path,
        )
        .unwrap();
        crate::internal::local_state::groups::upsert_group(
            &connection,
            crate::internal::local_state::groups::GroupRecord {
                owner_identity_id: client.current_identity().id.as_str().to_owned(),
                owner_did: client.did().as_str().to_owned(),
                group_id: group_did.to_owned(),
                group_did: group_did.to_owned(),
                membership_status: "active".to_owned(),
                metadata: serde_json::to_string(&metadata).unwrap(),
                credential_name: client.current_identity().id.as_str().to_owned(),
                ..Default::default()
            },
        )
        .unwrap();
    }

    fn unique_temp_root() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "im-core-group-e2ee-state-ref-{}-{nanos}",
            std::process::id()
        ))
    }
}
