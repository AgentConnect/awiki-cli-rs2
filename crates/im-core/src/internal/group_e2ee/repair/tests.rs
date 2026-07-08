use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use anp::group_e2ee::operations::{
    AbortCommitInput, AbortCommitOutput, AddMemberInput, CreateGroupInput, DecryptInput,
    DecryptOutput, EncryptInput, EncryptOutput, FinalizeCommitInput, FinalizeCommitOutput,
    GenerateKeyPackageInput, GroupKeyPackageOutput, LeaveGroupInput, PendingCommitStatus,
    PreparedMlsCommitOutput, ProcessNoticeInput, ProcessNoticeOutput, ProcessWelcomeInput,
    ProcessWelcomeOutput, RecoverMemberInput, RemoveMemberInput, StatusInput, StatusOutput,
    UpdateMemberInput,
};
use serde_json::json;

use super::*;

#[test]
fn repair_processes_commit_notice_and_marks_delivered_without_public_raw_notice() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let provider = RecordingMlsProvider::active("1");
    let result = GroupE2eeRepairRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::clone(&calls),
            responses: vec![
                (
                    "group.e2ee.head".to_owned(),
                    json!({"epoch": "2", "actor_membership_status": "active"}),
                ),
                (
                    "group.e2ee.notice".to_owned(),
                    json!({
                        "notices": [{
                            "notice_id": "notice-1",
                            "notice_type": "commit-delivery",
                            "group_did": "did:example:groups:e2ee",
                            "commit_b64u": "secret-commit",
                            "from_epoch": "1",
                            "to_epoch": "2",
                            "subject_did": "did:example:bob",
                            "subject_status": "active"
                        }]
                    }),
                ),
                (
                    "group.e2ee.head".to_owned(),
                    json!({"epoch": "2", "actor_membership_status": "active"}),
                ),
                (
                    "group.e2ee.notice".to_owned(),
                    json!({"pending_count": 0, "notices": []}),
                ),
                ("group.e2ee.notice".to_owned(), json!({"delivered": true})),
            ],
        },
        provider.clone(),
    )
    .repair(GroupE2eeRepairInput {
        group: crate::ids::GroupRef::parse("did:example:groups:e2ee").unwrap(),
        credentials: Some(fixture.credentials()),
        notice_limit: 50,
    })
    .unwrap();

    assert!(result.repaired);
    assert_eq!(result.state, crate::secure::GroupSecureState::Ready);
    assert!(!format!("{result:?}").contains("secret-commit"));
    assert_eq!(
        provider.processed_notices().as_slice(),
        ["did:example:groups:e2ee:1"]
    );
    let calls = calls.borrow();
    assert!(calls.iter().any(|call| {
        call.method == "group.e2ee.notice"
            && call.params["body"]["mark_delivered"] == Value::Bool(true)
            && call.params["body"]["notice_ids"][0] == "notice-1"
    }));
}

#[tokio::test]
async fn repair_async_uses_async_transport_and_db_actor_summary() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let provider = RecordingMlsProvider::active("1");
    let result = GroupE2eeRepairRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::clone(&calls),
            responses: vec![
                (
                    "group.e2ee.head".to_owned(),
                    json!({"epoch": "2", "actor_membership_status": "active"}),
                ),
                (
                    "group.e2ee.notice".to_owned(),
                    json!({
                        "notices": [{
                            "notice_id": "notice-1",
                            "notice_type": "commit-delivery",
                            "group_did": "did:example:groups:e2ee",
                            "group_state_version": "state-2",
                            "commit_b64u": "secret-commit",
                            "from_epoch": "1",
                            "to_epoch": "2",
                            "subject_did": "did:example:bob",
                            "subject_status": "active"
                        }]
                    }),
                ),
                (
                    "group.e2ee.head".to_owned(),
                    json!({"epoch": "2", "actor_membership_status": "active"}),
                ),
                (
                    "group.e2ee.notice".to_owned(),
                    json!({"pending_count": 0, "notices": []}),
                ),
                ("group.e2ee.notice".to_owned(), json!({"delivered": true})),
            ],
        },
        provider.clone(),
    )
    .repair_async(GroupE2eeRepairInput {
        group: crate::ids::GroupRef::parse("did:example:groups:e2ee").unwrap(),
        credentials: Some(fixture.credentials()),
        notice_limit: 50,
    })
    .await
    .unwrap();

    assert!(result.repaired);
    assert_eq!(result.state, crate::secure::GroupSecureState::Ready);
    assert!(!format!("{result:?}").contains("secret-commit"));
    assert_eq!(
        provider.processed_notices().as_slice(),
        ["did:example:groups:e2ee:1"]
    );
    let metadata = stored_group_metadata(&fixture, &client, "did:example:groups:e2ee");
    assert_eq!(metadata["group_e2ee"]["epoch"], "2");
    assert_eq!(metadata["group_e2ee"]["crypto_group_id_b64u"], "crypto");
    assert_eq!(metadata["group_e2ee"]["group_state_version"], "state-2");

    let calls = calls.borrow();
    assert!(calls.iter().any(|call| {
        call.method == "group.e2ee.notice"
            && call.params["body"]["mark_delivered"] == Value::Bool(true)
            && call.params["body"]["notice_ids"][0] == "notice-1"
    }));
}

#[test]
fn repair_finalizes_pending_commit_when_service_head_accepted_target_epoch() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let provider = RecordingMlsProvider::with_pending_commit("7", "8");
    let result = GroupE2eeRepairRuntime::new(
        &client,
        ReadySessionProvider,
        RecordingTransport {
            calls: Rc::new(RefCell::new(Vec::new())),
            responses: vec![
                (
                    "group.e2ee.head".to_owned(),
                    json!({"epoch": "8", "actor_membership_status": "active"}),
                ),
                (
                    "group.e2ee.notice".to_owned(),
                    json!({"pending_count": 0, "notices": []}),
                ),
                (
                    "group.e2ee.head".to_owned(),
                    json!({"epoch": "8", "actor_membership_status": "active"}),
                ),
                (
                    "group.e2ee.notice".to_owned(),
                    json!({"pending_count": 0, "notices": []}),
                ),
            ],
        },
        provider.clone(),
    )
    .repair(GroupE2eeRepairInput {
        group: crate::ids::GroupRef::parse("did:example:groups:e2ee").unwrap(),
        credentials: Some(fixture.credentials()),
        notice_limit: 50,
    })
    .unwrap();

    assert!(result.repaired);
    assert_eq!(result.state, crate::secure::GroupSecureState::Ready);
    assert_eq!(provider.finalized().as_slice(), ["pc-test"]);
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
            bearer_token: None,
        })
    }

    fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
        unreachable!("group E2EE repair should not refresh through the session provider")
    }

    fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
        unreachable!("group E2EE repair should not read auth status")
    }
}

impl AsyncSessionProvider for ReadySessionProvider {
    async fn ensure_session(
        &self,
        scope: crate::auth::AuthScope,
    ) -> crate::ImResult<crate::auth::SessionBundle> {
        SessionProvider::ensure_session(self, scope)
    }

    async fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
        unreachable!("group E2EE repair should not refresh through the session provider")
    }

    async fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
        unreachable!("group E2EE repair should not read auth status")
    }
}

#[derive(Clone)]
struct RecordingTransport {
    calls: Rc<RefCell<Vec<RecordedCall>>>,
    responses: Vec<(String, Value)>,
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
        if let Some((index, _)) = self
            .responses
            .iter()
            .enumerate()
            .find(|(_, (candidate, _))| candidate == method)
        {
            return Ok(self.responses.remove(index).1);
        }
        Err(crate::ImError::Service {
            status_code: None,
            code: Some("missing_test_response".to_owned()),
            message: format!("missing response for {method}"),
            data: None,
        })
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

struct RecordedCall {
    endpoint: String,
    method: String,
    params: Value,
}

#[derive(Clone)]
struct RecordingMlsProvider {
    status: Arc<Mutex<StatusOutput>>,
    processed_notices: Arc<Mutex<Vec<String>>>,
    finalized: Arc<Mutex<Vec<String>>>,
}

impl RecordingMlsProvider {
    fn active(epoch: &str) -> Self {
        Self {
            status: Arc::new(Mutex::new(active_status(epoch, Vec::new()))),
            processed_notices: Arc::new(Mutex::new(Vec::new())),
            finalized: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn with_pending_commit(from_epoch: &str, to_epoch: &str) -> Self {
        Self {
            status: Arc::new(Mutex::new(active_status(
                from_epoch,
                vec![PendingCommitStatus {
                    pending_commit_id: "pc-test".to_owned(),
                    operation_id: "op-test".to_owned(),
                    agent_did: "did:example:alice".to_owned(),
                    device_id: DEFAULT_GROUP_MLS_DEVICE_ID.to_owned(),
                    group_did: "did:example:groups:e2ee".to_owned(),
                    subject_did: "did:example:bob".to_owned(),
                    subject_status: "active".to_owned(),
                    from_epoch: from_epoch.to_owned(),
                    to_epoch: to_epoch.to_owned(),
                    status: "pending".to_owned(),
                }],
            ))),
            processed_notices: Arc::new(Mutex::new(Vec::new())),
            finalized: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn processed_notices(&self) -> Vec<String> {
        self.processed_notices.lock().unwrap().clone()
    }

    fn finalized(&self) -> Vec<String> {
        self.finalized.lock().unwrap().clone()
    }
}

impl GroupMlsProvider for RecordingMlsProvider {
    fn status(&self, _input: StatusInput) -> crate::ImResult<StatusOutput> {
        Ok(self.status.lock().unwrap().clone())
    }

    fn finalize_commit(&self, input: FinalizeCommitInput) -> crate::ImResult<FinalizeCommitOutput> {
        self.finalized
            .lock()
            .unwrap()
            .push(input.pending_commit_id.clone());
        let mut status = self.status.lock().unwrap();
        status.epoch = Some("8".to_owned());
        status.local_epoch = Some("8".to_owned());
        status.pending_commits.clear();
        Ok(FinalizeCommitOutput {
            pending_commit_id: input.pending_commit_id,
            operation_id: "op-test".to_owned(),
            group_did: "did:example:groups:e2ee".to_owned(),
            crypto_group_id_b64u: "crypto".to_owned(),
            status: "finalized".to_owned(),
            from_epoch: "7".to_owned(),
            epoch: "8".to_owned(),
            local_epoch: "8".to_owned(),
            subject_did: "did:example:bob".to_owned(),
            subject_status: "active".to_owned(),
            epoch_authenticator: None,
        })
    }

    fn process_notice(&self, input: ProcessNoticeInput) -> crate::ImResult<ProcessNoticeOutput> {
        self.processed_notices
            .lock()
            .unwrap()
            .push(format!("{}:{}", input.group_did, input.from_epoch));
        let mut status = self.status.lock().unwrap();
        status.epoch = Some("2".to_owned());
        status.local_epoch = Some("2".to_owned());
        Ok(ProcessNoticeOutput {
            crypto_group_id_b64u: "crypto".to_owned(),
            status: "active".to_owned(),
            self_removed: false,
            from_epoch: input.from_epoch,
            epoch: "2".to_owned(),
            epoch_authenticator: None,
            ratchet_tree_b64u: None,
            subject_did: "did:example:bob".to_owned(),
            subject_status: "active".to_owned(),
        })
    }

    fn generate_key_package(
        &self,
        _input: GenerateKeyPackageInput,
    ) -> crate::ImResult<GroupKeyPackageOutput> {
        unreachable!("repair should not generate key packages")
    }

    fn create_group_prepare(
        &self,
        _input: CreateGroupInput,
    ) -> crate::ImResult<PreparedMlsCommitOutput> {
        unreachable!("repair should not create groups")
    }

    fn add_member_prepare(
        &self,
        _input: AddMemberInput,
    ) -> crate::ImResult<PreparedMlsCommitOutput> {
        unreachable!("repair should not add members")
    }

    fn remove_member_prepare(
        &self,
        _input: RemoveMemberInput,
    ) -> crate::ImResult<PreparedMlsCommitOutput> {
        unreachable!("repair should not remove members")
    }

    fn leave_prepare(&self, _input: LeaveGroupInput) -> crate::ImResult<PreparedMlsCommitOutput> {
        unreachable!("repair should not leave groups")
    }

    fn update_member_prepare(
        &self,
        _input: UpdateMemberInput,
    ) -> crate::ImResult<PreparedMlsCommitOutput> {
        unreachable!("repair should not update members")
    }

    fn recover_member_prepare(
        &self,
        _input: RecoverMemberInput,
    ) -> crate::ImResult<PreparedMlsCommitOutput> {
        unreachable!("repair should not recover members")
    }

    fn abort_commit(&self, _input: AbortCommitInput) -> crate::ImResult<AbortCommitOutput> {
        unreachable!("repair should not abort commits")
    }

    fn process_welcome(
        &self,
        _input: ProcessWelcomeInput,
    ) -> crate::ImResult<ProcessWelcomeOutput> {
        unreachable!("commit repair test should not process welcomes")
    }

    fn encrypt(&self, _input: EncryptInput) -> crate::ImResult<EncryptOutput> {
        unreachable!("repair should not encrypt")
    }

    fn decrypt(&self, _input: DecryptInput) -> crate::ImResult<DecryptOutput> {
        unreachable!("repair should not decrypt")
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
                service_base_url: crate::ServiceEndpoint::parse("https://example.test").unwrap(),
                did_domain: "awiki.test".to_owned(),
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
                challenge: Some("group-e2ee-repair-test".to_owned()),
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

fn stored_group_metadata(
    fixture: &Fixture,
    client: &crate::core::ImClient,
    group_did: &str,
) -> Value {
    let connection =
        rusqlite::Connection::open(fixture.root.join("local").join("im.sqlite")).unwrap();
    let raw: String = connection
        .query_row(
            "SELECT metadata FROM groups WHERE owner_did = ?1 AND group_did = ?2",
            rusqlite::params![client.did().as_str(), group_did],
            |row| row.get(0),
        )
        .unwrap();
    serde_json::from_str(&raw).unwrap()
}

fn active_status(epoch: &str, pending_commits: Vec<PendingCommitStatus>) -> StatusOutput {
    StatusOutput {
        status: "active".to_owned(),
        epoch: Some(epoch.to_owned()),
        local_epoch: Some(epoch.to_owned()),
        pending_commits,
        epoch_authenticator: None,
    }
}

fn unique_temp_root() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "im-core-group-e2ee-repair-{}-{nanos}",
        std::process::id()
    ))
}
