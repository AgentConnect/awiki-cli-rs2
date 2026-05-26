use anp::group_e2ee::operations::{StatusInput, StatusOutput};
use serde_json::Value;

use crate::internal::auth::session::SessionProvider;
use crate::internal::message_runtime::group::{load_credentials, GroupTextCredentials};
use crate::internal::transport::AuthenticatedRpcTransport;

use super::provider::GroupMlsProvider;
use super::DEFAULT_GROUP_MLS_DEVICE_ID;

pub(crate) struct GroupE2eeStatusRuntime<'a, P, T, M> {
    client: &'a crate::core::ImClient,
    session_provider: P,
    transport: T,
    mls_provider: M,
}

#[derive(Debug, Clone)]
pub(crate) struct GroupE2eeStatusInput {
    pub(crate) group: crate::ids::GroupRef,
    pub(crate) credentials: Option<GroupTextCredentials>,
    pub(crate) notice_limit: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GroupE2eeStatusResult {
    pub(crate) group: crate::ids::GroupRef,
    pub(crate) state: crate::secure::GroupSecureState,
    pub(crate) can_send_secure: bool,
    pub(crate) local_readiness: crate::secure::GroupSecureLocalReadiness,
    pub(crate) pending_work: crate::secure::GroupSecurePendingWork,
    pub(crate) problem: Option<crate::secure::SecureProblem>,
    pub(crate) warnings: Vec<String>,
}

impl<'a, P, T, M> GroupE2eeStatusRuntime<'a, P, T, M>
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

    pub(crate) fn status(
        mut self,
        input: GroupE2eeStatusInput,
    ) -> crate::ImResult<GroupE2eeStatusResult> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::GroupMessaging)?;
        let group_did = require_group(input.group.as_str())?;
        let device_id = device_id_for_client(self.client);
        let mut warnings = Vec::new();
        let (local_status, local_error) = match self.mls_provider.status(StatusInput {
            request_id: format!(
                "group-e2ee-status-{}",
                crate::internal::wire::common::generate_operation_id()
            ),
            device_id,
            agent_did: Some(self.client.did().as_str().to_owned()),
            group_did: Some(group_did.to_owned()),
        }) {
            Ok(status) => (Some(status), None),
            Err(err) => {
                warnings.push("group E2EE local MLS status is unavailable".to_owned());
                (None, Some(err.to_string()))
            }
        };

        let credentials = match input.credentials {
            Some(credentials) => Some(credentials),
            None => match load_credentials(self.client) {
                Ok(credentials) => Some(credentials),
                Err(_err) => {
                    warnings
                        .push("group E2EE service status credentials are unavailable".to_owned());
                    return Ok(group_status_from_parts(
                        input.group,
                        local_status.as_ref(),
                        None,
                        0,
                        local_error.as_deref(),
                        warnings,
                    ));
                }
            },
        };

        let mut service_head = None;
        let mut service_disabled = None;
        let mut pending_notice_total = 0;
        if let Some(credentials) = credentials.as_ref() {
            match super::wire::build_group_e2ee_head_rpc_params(
                credentials,
                self.client.did().as_str(),
                group_did,
            )
            .and_then(|params| {
                self.transport.authenticated_rpc(
                    crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT,
                    "group.e2ee.head",
                    params,
                )
            }) {
                Ok(head) => service_head = Some(head),
                Err(err) if super::lifecycle::is_group_e2ee_service_disabled(&err) => {
                    service_disabled = Some(err.to_string());
                    warnings.push(format!("group E2EE service head unavailable: {err}"));
                }
                Err(err) => warnings.push(format!("group E2EE service head unavailable: {err}")),
            }

            if service_disabled.is_none() {
                match super::wire::build_group_e2ee_notice_rpc_params(
                    credentials,
                    self.client.did().as_str(),
                    group_did,
                    input.notice_limit,
                    false,
                    &[],
                )
                .and_then(|params| {
                    self.transport.authenticated_rpc(
                        crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT,
                        "group.e2ee.notice",
                        params,
                    )
                }) {
                    Ok(pending) => pending_notice_total = pending_notice_count(&pending),
                    Err(err) if super::lifecycle::is_group_e2ee_service_disabled(&err) => {
                        service_disabled = Some(err.to_string());
                        warnings.push(format!(
                            "group E2EE pending notice status unavailable: {err}"
                        ));
                    }
                    Err(err) => warnings.push(format!(
                        "group E2EE pending notice status unavailable: {err}"
                    )),
                }
            }
        }

        let mut status = group_status_from_parts(
            input.group,
            local_status.as_ref(),
            service_head.as_ref(),
            pending_notice_total,
            local_error.as_deref(),
            warnings,
        );
        if let Some(message) = service_disabled {
            status.state = crate::secure::GroupSecureState::Unknown;
            status.can_send_secure = false;
            status.problem = Some(secure_problem(
                crate::secure::SecureProblemCode::Unsupported,
                format!("group-e2ee-status is unavailable: {message}"),
                false,
            ));
        }
        Ok(status)
    }
}

pub(super) fn group_status_from_parts(
    group: crate::ids::GroupRef,
    local_status: Option<&StatusOutput>,
    service_head: Option<&Value>,
    pending_notice_count: u32,
    local_error: Option<&str>,
    warnings: Vec<String>,
) -> GroupE2eeStatusResult {
    let pending_commit_count = local_status
        .map(|status| status.pending_commits.len() as u32)
        .unwrap_or_default();
    let has_local_state = local_status
        .map(|status| {
            !matches!(
                normalized_status(status.status.as_str()).as_str(),
                "" | "empty" | "missing"
            ) && local_epoch(status).is_some()
        })
        .unwrap_or(false);
    let has_active_membership = local_status
        .map(|status| normalized_status(status.status.as_str()) == "active")
        .unwrap_or(false);
    let diagnosis = diagnose_group_status(
        local_status,
        service_head,
        pending_notice_count,
        pending_commit_count,
        local_error,
    );
    let (state, can_send_secure, problem) = public_state_from_diagnosis(&diagnosis, local_error);
    GroupE2eeStatusResult {
        group,
        state,
        can_send_secure,
        local_readiness: crate::secure::GroupSecureLocalReadiness {
            has_local_state,
            has_active_membership,
        },
        pending_work: crate::secure::GroupSecurePendingWork {
            pending_notices: pending_notice_count,
            pending_commits: pending_commit_count,
        },
        problem,
        warnings,
    }
}

fn diagnose_group_status(
    local_status: Option<&StatusOutput>,
    service_head: Option<&Value>,
    pending_notice_count: u32,
    pending_commit_count: u32,
    local_error: Option<&str>,
) -> GroupStatusDiagnosis {
    let actor_status = service_head
        .and_then(actor_membership_status)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(
        actor_status.as_str(),
        "removed" | "left" | "non_member" | "inactive"
    ) {
        return GroupStatusDiagnosis::Inactive;
    }
    if pending_commit_count > 0 {
        return GroupStatusDiagnosis::PendingCommit;
    }
    if pending_notice_count > 0 {
        return GroupStatusDiagnosis::PendingNotices;
    }

    let local_epoch = local_status.and_then(local_epoch);
    let local_state = local_status
        .map(|status| normalized_status(status.status.as_str()))
        .unwrap_or_default();
    if local_error.is_some()
        || local_state.is_empty()
        || local_state == "empty"
        || local_state == "missing"
        || local_epoch.is_none()
    {
        return GroupStatusDiagnosis::MissingState;
    }

    if local_state != "active" {
        return GroupStatusDiagnosis::Inactive;
    }

    let service_epoch = service_head.and_then(service_epoch);
    match (local_epoch, service_epoch) {
        (Some(local), Some(service)) if local == service => GroupStatusDiagnosis::InSync,
        (Some(local), Some(service)) if local < service => GroupStatusDiagnosis::EpochLag,
        (Some(local), Some(service)) if local > service => GroupStatusDiagnosis::LocalAhead,
        (Some(_), None) => GroupStatusDiagnosis::LocalOnly,
        _ => GroupStatusDiagnosis::Unknown,
    }
}

fn public_state_from_diagnosis(
    diagnosis: &GroupStatusDiagnosis,
    local_error: Option<&str>,
) -> (
    crate::secure::GroupSecureState,
    bool,
    Option<crate::secure::SecureProblem>,
) {
    match diagnosis {
        GroupStatusDiagnosis::InSync => (crate::secure::GroupSecureState::Ready, true, None),
        GroupStatusDiagnosis::PendingCommit => (
            crate::secure::GroupSecureState::Syncing,
            false,
            Some(secure_problem(
                crate::secure::SecureProblemCode::SessionNeedsRepair,
                "group E2EE has a pending local commit that must be finalized or aborted",
                true,
            )),
        ),
        GroupStatusDiagnosis::PendingNotices => (
            crate::secure::GroupSecureState::Syncing,
            false,
            Some(secure_problem(
                crate::secure::SecureProblemCode::SessionNeedsRepair,
                "group E2EE has pending membership notices to process",
                true,
            )),
        ),
        GroupStatusDiagnosis::MissingState => (
            crate::secure::GroupSecureState::MissingLocalState,
            false,
            Some(secure_problem(
                crate::secure::SecureProblemCode::GroupStateUnavailable,
                local_error
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or("group E2EE local MLS state is unavailable"),
                true,
            )),
        ),
        GroupStatusDiagnosis::EpochLag | GroupStatusDiagnosis::LocalAhead => (
            crate::secure::GroupSecureState::NeedsRepair,
            false,
            Some(secure_problem(
                crate::secure::SecureProblemCode::SessionNeedsRepair,
                "group E2EE local state is out of sync with the service state",
                true,
            )),
        ),
        GroupStatusDiagnosis::Inactive => (
            crate::secure::GroupSecureState::WaitingForMembershipUpdate,
            false,
            Some(secure_problem(
                crate::secure::SecureProblemCode::GroupStateUnavailable,
                "group E2EE membership is not active for this identity",
                true,
            )),
        ),
        GroupStatusDiagnosis::LocalOnly | GroupStatusDiagnosis::Unknown => (
            crate::secure::GroupSecureState::NeedsRepair,
            false,
            Some(secure_problem(
                crate::secure::SecureProblemCode::TransportUnavailable,
                "group E2EE service status could not be confirmed",
                true,
            )),
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum GroupStatusDiagnosis {
    InSync,
    PendingCommit,
    PendingNotices,
    MissingState,
    EpochLag,
    LocalAhead,
    Inactive,
    LocalOnly,
    Unknown,
}

fn pending_notice_count(value: &Value) -> u32 {
    for key in ["pending_count", "pending_notice_count", "count"] {
        if let Some(count) = u32_value(value.get(key)) {
            return count;
        }
    }
    value
        .get("notices")
        .and_then(Value::as_array)
        .map(|items| items.len() as u32)
        .unwrap_or_default()
}

fn actor_membership_status(value: &Value) -> Option<String> {
    first_string(&[
        value.get("actor_membership_status"),
        value.get("membership_status"),
        value
            .get("actor")
            .and_then(|actor| actor.get("membership_status")),
    ])
}

fn service_epoch(value: &Value) -> Option<i64> {
    first_i64(&[
        value.get("epoch"),
        value
            .get("group_state_ref")
            .and_then(|reference| reference.get("epoch")),
        value.get("mls").and_then(|mls| mls.get("epoch")),
    ])
}

fn local_epoch(status: &StatusOutput) -> Option<i64> {
    status
        .local_epoch
        .as_deref()
        .or(status.epoch.as_deref())
        .and_then(parse_i64)
}

fn u32_value(value: Option<&Value>) -> Option<u32> {
    first_i64(&[value]).map(|value| value.max(0) as u32)
}

fn first_i64(values: &[Option<&Value>]) -> Option<i64> {
    for value in values.iter().flatten() {
        match value {
            Value::Number(number) => {
                if let Some(value) = number.as_i64() {
                    return Some(value);
                }
                if let Some(value) = number.as_u64() {
                    return Some(value.min(i64::MAX as u64) as i64);
                }
            }
            Value::String(text) => {
                if let Some(value) = parse_i64(text) {
                    return Some(value);
                }
            }
            _ => {}
        }
    }
    None
}

fn first_string(values: &[Option<&Value>]) -> Option<String> {
    for value in values.iter().flatten() {
        match value {
            Value::String(text) if !text.trim().is_empty() => {
                return Some(text.trim().to_owned());
            }
            Value::Number(number) => return Some(number.to_string()),
            _ => {}
        }
    }
    None
}

fn parse_i64(value: &str) -> Option<i64> {
    value.trim().parse::<i64>().ok()
}

fn normalized_status(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn secure_problem(
    code: crate::secure::SecureProblemCode,
    message: impl Into<String>,
    retryable: bool,
) -> crate::secure::SecureProblem {
    crate::secure::SecureProblem {
        code,
        message: message.into(),
        retryable,
    }
}

fn require_group(group_did: &str) -> crate::ImResult<&str> {
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
        GenerateKeyPackageInput, GroupKeyPackageOutput, LeaveGroupInput, PendingCommitStatus,
        PreparedMlsCommitOutput, ProcessNoticeInput, ProcessNoticeOutput, ProcessWelcomeInput,
        ProcessWelcomeOutput, RecoverMemberInput, RemoveMemberInput, StatusInput, StatusOutput,
        UpdateMemberInput,
    };
    use serde_json::{json, Value};

    use super::*;

    #[test]
    fn status_reports_ready_without_exposing_mls_details() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let status = GroupE2eeStatusRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                responses: vec![
                    (
                        "group.e2ee.head".to_owned(),
                        json!({
                            "epoch": "7",
                            "actor_membership_status": "active",
                            "group_state_ref": {
                                "group_did": "did:example:groups:e2ee",
                                "group_state_version": "42"
                            }
                        }),
                    ),
                    (
                        "group.e2ee.notice".to_owned(),
                        json!({"pending_count": 0, "notices": []}),
                    ),
                ],
            },
            StatusProvider::active("7", Vec::new()),
        )
        .status(GroupE2eeStatusInput {
            group: crate::ids::GroupRef::parse("did:example:groups:e2ee").unwrap(),
            credentials: Some(fixture.credentials()),
            notice_limit: 50,
        })
        .unwrap();

        assert_eq!(status.state, crate::secure::GroupSecureState::Ready);
        assert!(status.can_send_secure);
        assert!(status.local_readiness.has_local_state);
        assert!(status.local_readiness.has_active_membership);
        assert_eq!(status.pending_work.pending_notices, 0);
        assert_eq!(status.pending_work.pending_commits, 0);
        assert!(status.problem.is_none());
        let debug = format!("{status:?}");
        assert!(!debug.contains("epoch_authenticator"));
        assert!(!debug.contains("KeyPackage"));
        assert!(!debug.contains("OpenMLS"));

        let calls = calls.borrow();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].method, "group.e2ee.head");
        assert_eq!(calls[1].method, "group.e2ee.notice");
        assert_eq!(calls[1].params["body"]["limit"], 50);
        assert_eq!(
            calls[1].params["meta"]["security_profile"],
            anp::group_e2ee::TRANSPORT_SECURITY_PROFILE
        );
    }

    #[test]
    fn status_fails_closed_for_pending_notices() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let status = GroupE2eeStatusRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::new(RefCell::new(Vec::new())),
                responses: vec![
                    (
                        "group.e2ee.head".to_owned(),
                        json!({"epoch": "7", "actor_membership_status": "active"}),
                    ),
                    (
                        "group.e2ee.notice".to_owned(),
                        json!({
                            "notices": [{
                                "notice_type": "commit-delivery",
                                "commit_b64u": "secret-commit"
                            }]
                        }),
                    ),
                ],
            },
            StatusProvider::active("7", Vec::new()),
        )
        .status(GroupE2eeStatusInput {
            group: crate::ids::GroupRef::parse("did:example:groups:e2ee").unwrap(),
            credentials: Some(fixture.credentials()),
            notice_limit: 50,
        })
        .unwrap();

        assert_eq!(status.state, crate::secure::GroupSecureState::Syncing);
        assert!(!status.can_send_secure);
        assert_eq!(status.pending_work.pending_notices, 1);
        assert_eq!(
            status.problem.as_ref().map(|problem| &problem.code),
            Some(&crate::secure::SecureProblemCode::SessionNeedsRepair)
        );
        assert!(!format!("{status:?}").contains("secret-commit"));
    }

    #[test]
    fn status_reports_missing_state_when_local_mls_is_empty() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let status = GroupE2eeStatusRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::new(RefCell::new(Vec::new())),
                responses: vec![
                    (
                        "group.e2ee.head".to_owned(),
                        json!({"epoch": "7", "actor_membership_status": "active"}),
                    ),
                    (
                        "group.e2ee.notice".to_owned(),
                        json!({"pending_count": 0, "notices": []}),
                    ),
                ],
            },
            StatusProvider::empty(),
        )
        .status(GroupE2eeStatusInput {
            group: crate::ids::GroupRef::parse("did:example:groups:e2ee").unwrap(),
            credentials: Some(fixture.credentials()),
            notice_limit: 50,
        })
        .unwrap();

        assert_eq!(
            status.state,
            crate::secure::GroupSecureState::MissingLocalState
        );
        assert!(!status.local_readiness.has_local_state);
        assert!(!status.can_send_secure);
        assert_eq!(
            status.problem.as_ref().map(|problem| &problem.code),
            Some(&crate::secure::SecureProblemCode::GroupStateUnavailable)
        );
    }

    #[test]
    fn status_reports_unknown_when_group_e2ee_service_is_disabled() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let status = GroupE2eeStatusRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                responses: vec![(
                    "group.e2ee.head".to_owned(),
                    json!({"error": {"code": "1405", "message": "group E2EE contract-test APIs are disabled"}}),
                )],
            },
            StatusProvider::empty(),
        )
        .status(GroupE2eeStatusInput {
            group: crate::ids::GroupRef::parse("did:example:groups:e2ee").unwrap(),
            credentials: Some(fixture.credentials()),
            notice_limit: 50,
        })
        .unwrap();

        assert_eq!(status.state, crate::secure::GroupSecureState::Unknown);
        assert!(!status.can_send_secure);
        assert_eq!(
            status.problem.as_ref().map(|problem| &problem.code),
            Some(&crate::secure::SecureProblemCode::Unsupported)
        );
        assert!(status
            .problem
            .as_ref()
            .map(|problem| problem.message.contains("group-e2ee-status")
                && problem.message.contains("contract-test APIs are disabled"))
            .unwrap_or(false));
        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].method, "group.e2ee.head");
    }

    #[test]
    fn status_reports_syncing_for_pending_commit() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let status = GroupE2eeStatusRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::new(RefCell::new(Vec::new())),
                responses: vec![
                    (
                        "group.e2ee.head".to_owned(),
                        json!({"epoch": "7", "actor_membership_status": "active"}),
                    ),
                    (
                        "group.e2ee.notice".to_owned(),
                        json!({"pending_count": 0, "notices": []}),
                    ),
                ],
            },
            StatusProvider::active("7", vec![pending_commit()]),
        )
        .status(GroupE2eeStatusInput {
            group: crate::ids::GroupRef::parse("did:example:groups:e2ee").unwrap(),
            credentials: Some(fixture.credentials()),
            notice_limit: 50,
        })
        .unwrap();

        assert_eq!(status.state, crate::secure::GroupSecureState::Syncing);
        assert_eq!(status.pending_work.pending_commits, 1);
        assert!(!status.can_send_secure);
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
            unreachable!("group E2EE status should not refresh through the session provider")
        }

        fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
            unreachable!("group E2EE status should not read auth status")
        }
    }

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
                let value = self.responses.remove(index).1;
                if let Some(error) = value.get("error").and_then(Value::as_object) {
                    return Err(crate::ImError::Service {
                        status_code: None,
                        code: error.get("code").and_then(Value::as_str).map(str::to_owned),
                        message: error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("service error")
                            .to_owned(),
                    });
                }
                return Ok(value);
            }
            Err(crate::ImError::Service {
                status_code: None,
                code: Some("missing_test_response".to_owned()),
                message: format!("missing response for {method}"),
            })
        }
    }

    struct RecordedCall {
        endpoint: String,
        method: String,
        params: Value,
    }

    struct StatusProvider {
        status: StatusOutput,
    }

    impl StatusProvider {
        fn active(epoch: &str, pending_commits: Vec<PendingCommitStatus>) -> Self {
            Self {
                status: StatusOutput {
                    status: "active".to_owned(),
                    epoch: Some(epoch.to_owned()),
                    local_epoch: Some(epoch.to_owned()),
                    pending_commits,
                    epoch_authenticator: Some("secret-authenticator".to_owned()),
                },
            }
        }

        fn empty() -> Self {
            Self {
                status: StatusOutput {
                    status: "empty".to_owned(),
                    epoch: None,
                    local_epoch: None,
                    pending_commits: Vec::new(),
                    epoch_authenticator: None,
                },
            }
        }
    }

    impl GroupMlsProvider for StatusProvider {
        fn status(&self, input: StatusInput) -> crate::ImResult<StatusOutput> {
            assert_eq!(input.agent_did.as_deref(), Some("did:example:alice"));
            assert_eq!(input.group_did.as_deref(), Some("did:example:groups:e2ee"));
            Ok(self.status.clone())
        }

        fn generate_key_package(
            &self,
            _input: GenerateKeyPackageInput,
        ) -> crate::ImResult<GroupKeyPackageOutput> {
            unreachable!("status should not generate key packages")
        }

        fn create_group_prepare(
            &self,
            _input: CreateGroupInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("status should not create groups")
        }

        fn add_member_prepare(
            &self,
            _input: AddMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("status should not add members")
        }

        fn remove_member_prepare(
            &self,
            _input: RemoveMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("status should not remove members")
        }

        fn leave_prepare(
            &self,
            _input: LeaveGroupInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("status should not leave groups")
        }

        fn update_member_prepare(
            &self,
            _input: UpdateMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("status should not update members")
        }

        fn recover_member_prepare(
            &self,
            _input: RecoverMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("status should not recover members")
        }

        fn finalize_commit(
            &self,
            _input: FinalizeCommitInput,
        ) -> crate::ImResult<FinalizeCommitOutput> {
            unreachable!("status should not finalize commits")
        }

        fn abort_commit(&self, _input: AbortCommitInput) -> crate::ImResult<AbortCommitOutput> {
            unreachable!("status should not abort commits")
        }

        fn process_welcome(
            &self,
            _input: ProcessWelcomeInput,
        ) -> crate::ImResult<ProcessWelcomeOutput> {
            unreachable!("status should not process welcomes")
        }

        fn process_notice(
            &self,
            _input: ProcessNoticeInput,
        ) -> crate::ImResult<ProcessNoticeOutput> {
            unreachable!("status should not process notices")
        }

        fn encrypt(&self, _input: EncryptInput) -> crate::ImResult<EncryptOutput> {
            unreachable!("status should not encrypt")
        }

        fn decrypt(&self, _input: DecryptInput) -> crate::ImResult<DecryptOutput> {
            unreachable!("status should not decrypt")
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
                    challenge: Some("group-e2ee-status-test".to_owned()),
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

    fn pending_commit() -> PendingCommitStatus {
        PendingCommitStatus {
            pending_commit_id: "pc-test".to_owned(),
            operation_id: "op-test".to_owned(),
            agent_did: "did:example:alice".to_owned(),
            device_id: DEFAULT_GROUP_MLS_DEVICE_ID.to_owned(),
            group_did: "did:example:groups:e2ee".to_owned(),
            subject_did: "did:example:bob".to_owned(),
            subject_status: "active".to_owned(),
            from_epoch: "7".to_owned(),
            to_epoch: "8".to_owned(),
            status: "pending".to_owned(),
        }
    }

    fn unique_temp_root() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "im-core-group-e2ee-status-{}-{nanos}",
            std::process::id()
        ))
    }
}
