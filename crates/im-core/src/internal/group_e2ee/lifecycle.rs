use anp::group_e2ee::operations::{
    AbortCommitInput, AddMemberInput, CreateGroupInput, FinalizeCommitInput, LeaveGroupInput,
    RemoveMemberInput,
};
use anp::group_e2ee::{GroupKeyPackage, GroupStateRef};
use serde_json::{Map, Value};

use crate::internal::auth::session::SessionProvider;
use crate::internal::message_runtime::group::{load_credentials, GroupTextCredentials};
use crate::internal::transport::AuthenticatedRpcTransport;

use super::provider::GroupMlsProvider;
use super::state_ref::{group_state_ref_from_service_head, local_group_state_ref};
use super::DEFAULT_GROUP_MLS_DEVICE_ID;

pub(crate) struct GroupE2eeLifecycleRuntime<'a, P, T, M> {
    client: &'a crate::core::ImClient,
    session_provider: P,
    transport: T,
    mls_provider: M,
}

#[derive(Debug, Clone)]
pub(crate) struct GroupE2eeCreateInput {
    pub(crate) group: crate::ids::GroupRef,
    pub(crate) credentials: Option<GroupTextCredentials>,
    pub(crate) service_did: Option<crate::ids::Did>,
}

#[derive(Debug, Clone)]
pub(crate) struct GroupE2eeMemberMutationInput {
    pub(crate) group: crate::ids::GroupRef,
    pub(crate) member: crate::ids::Did,
    pub(crate) reason_text: Option<String>,
    pub(crate) leave_request_id: Option<String>,
    pub(crate) credentials: Option<GroupTextCredentials>,
    pub(crate) service_did: Option<crate::ids::Did>,
}

#[derive(Debug, Clone)]
pub(crate) struct GroupE2eeLeaveInput {
    pub(crate) group: crate::ids::GroupRef,
    pub(crate) reason_text: Option<String>,
    pub(crate) owner_leave_commit: bool,
    pub(crate) credentials: Option<GroupTextCredentials>,
}

#[derive(Debug, Clone)]
pub(crate) struct GroupE2eeServiceAvailabilityInput {
    pub(crate) credentials: Option<GroupTextCredentials>,
    pub(crate) service_did: Option<crate::ids::Did>,
    pub(crate) check_key_package: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct GroupE2eeLifecycleResult {
    pub(crate) delivery: Value,
    pub(crate) warnings: Vec<String>,
}

pub(crate) fn ensure_group_e2ee_service_available<P, T>(
    client: &crate::core::ImClient,
    session_provider: &P,
    transport: &mut T,
    input: GroupE2eeServiceAvailabilityInput,
) -> crate::ImResult<()>
where
    P: SessionProvider,
    T: AuthenticatedRpcTransport,
{
    session_provider.ensure_session(crate::auth::AuthScope::GroupMessaging)?;
    let credentials = input
        .credentials
        .map(Ok)
        .unwrap_or_else(|| load_credentials(client))?;
    let preflight_group_did = group_e2ee_availability_group_did(client);
    let head_params = super::wire::build_group_e2ee_head_rpc_params(
        &credentials,
        client.did().as_str(),
        &preflight_group_did,
    )?;
    match transport.authenticated_rpc(
        crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT,
        "group.e2ee.head",
        head_params,
    ) {
        Ok(_) => {}
        Err(err) if is_group_e2ee_service_disabled(&err) => return Err(err),
        Err(_) => {}
    }
    if !input.check_key_package {
        return Ok(());
    }
    let service_did = input
        .service_did
        .map(Ok)
        .unwrap_or_else(|| group_e2ee_service_did(client))?;
    let key_package_params = super::wire::build_group_e2ee_get_key_package_rpc_params(
        &credentials,
        client.did().as_str(),
        service_did.as_str(),
        &preflight_group_did,
        client.did().as_str(),
    )?;
    match transport.authenticated_rpc(
        crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT,
        "group.e2ee.get_key_package",
        key_package_params,
    ) {
        Ok(_) => Ok(()),
        Err(err) if is_group_e2ee_service_disabled(&err) => Err(err),
        Err(_) => Ok(()),
    }
}

impl<'a, P, T, M> GroupE2eeLifecycleRuntime<'a, P, T, M>
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

    pub(crate) fn create_secure_group(
        mut self,
        input: GroupE2eeCreateInput,
    ) -> crate::ImResult<GroupE2eeLifecycleResult> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::GroupMessaging)?;
        let credentials = self.credentials(input.credentials)?;
        let service_did = self.service_did(input.service_did)?;
        let group_did = require_group(input.group.as_str())?.to_owned();
        let operation_id = format!(
            "op-{}",
            crate::internal::wire::common::generate_operation_id()
        );
        let prepared = self.mls_provider.create_group_prepare(CreateGroupInput {
            creator_did: self.client.did().as_str().to_owned(),
            device_id: device_id_for_client(self.client),
            group_did: group_did.clone(),
            operation_id: operation_id.clone(),
            request_id: format!("group-e2ee-create-{operation_id}"),
            pending_commit_id: Some(format!("pc-{operation_id}")),
        })?;
        let group_state_ref = local_group_state_ref(self.client, &group_did);
        let params = super::wire::build_group_e2ee_create_rpc_params(
            &credentials,
            self.client.did().as_str(),
            service_did.as_str(),
            &group_did,
            &prepared,
            group_state_ref.as_ref(),
        )?;
        let delivery = self.transport.authenticated_rpc(
            crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT,
            "group.e2ee.create",
            params,
        )?;
        let mut warnings = Vec::new();
        match self.finalize_prepared(&prepared, &group_did, &delivery) {
            Ok(finalized) => persist_group_e2ee_summary(
                self.client,
                &group_did,
                summary_epoch(&finalized.epoch, Some(prepared.epoch.as_str())),
                group_state_version(&delivery, group_state_ref.as_ref()).as_deref(),
                Some(finalized.crypto_group_id_b64u.as_str()),
                finalized.epoch_authenticator.as_deref(),
                Some(prepared.suite.as_str()),
                Some(finalized.operation_id.as_str()),
                "active",
            ),
            Err(err) => warnings.push(format!(
                "group E2EE create was accepted by service but local finalize failed: {err}"
            )),
        }
        Ok(GroupE2eeLifecycleResult {
            delivery: public_lifecycle_delivery(
                "secure_group_create",
                &group_did,
                None,
                Some("active"),
                &delivery,
            ),
            warnings,
        })
    }

    pub(crate) fn add_secure_member(
        mut self,
        input: GroupE2eeMemberMutationInput,
    ) -> crate::ImResult<GroupE2eeLifecycleResult> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::GroupMessaging)?;
        let credentials = self.credentials(input.credentials)?;
        let service_did = self.service_did(input.service_did)?;
        let group_did = require_group(input.group.as_str())?.to_owned();
        let member_did = require_did(input.member.as_str(), "member")?.to_owned();
        let key_package = self.lookup_member_key_package(
            &credentials,
            service_did.as_str(),
            &group_did,
            &member_did,
        )?;
        let operation_id = format!(
            "op-{}",
            crate::internal::wire::common::generate_operation_id()
        );
        let prepared = self.mls_provider.add_member_prepare(AddMemberInput {
            actor_did: self.client.did().as_str().to_owned(),
            device_id: device_id_for_client(self.client),
            group_did: group_did.clone(),
            member_did: member_did.clone(),
            group_key_package: key_package.clone(),
            operation_id: operation_id.clone(),
            request_id: format!("group-e2ee-add-{operation_id}"),
            pending_commit_id: Some(format!("pc-{operation_id}")),
        })?;
        let group_state_ref = local_group_state_ref(self.client, &group_did);
        let params = super::wire::build_group_e2ee_add_rpc_params(
            &credentials,
            self.client.did().as_str(),
            &group_did,
            &member_did,
            &prepared,
            &key_package,
            group_state_ref.as_ref(),
        )?;
        let delivery = match self.transport.authenticated_rpc(
            crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT,
            "group.e2ee.add",
            params,
        ) {
            Ok(delivery) => delivery,
            Err(err) => return self.service_rejected_prepared(&prepared, &group_did, err),
        };
        let mut warnings = Vec::new();
        match self.finalize_prepared(&prepared, &group_did, &delivery) {
            Ok(finalized) => persist_group_e2ee_summary(
                self.client,
                &group_did,
                summary_epoch(&finalized.epoch, Some(prepared.epoch.as_str())),
                group_state_version(&delivery, group_state_ref.as_ref()).as_deref(),
                Some(finalized.crypto_group_id_b64u.as_str()),
                finalized.epoch_authenticator.as_deref(),
                Some(prepared.suite.as_str()),
                Some(finalized.operation_id.as_str()),
                "active",
            ),
            Err(err) => warnings.push(format!(
                "group E2EE add was accepted by service but local finalize failed: {err}"
            )),
        }
        Ok(GroupE2eeLifecycleResult {
            delivery: public_lifecycle_delivery(
                "secure_group_add_member",
                &group_did,
                Some(&member_did),
                Some("active"),
                &delivery,
            ),
            warnings,
        })
    }

    pub(crate) fn remove_secure_member(
        mut self,
        input: GroupE2eeMemberMutationInput,
    ) -> crate::ImResult<GroupE2eeLifecycleResult> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::GroupMessaging)?;
        let credentials = self.credentials(input.credentials)?;
        let group_did = require_group(input.group.as_str())?.to_owned();
        let member_did = require_did(input.member.as_str(), "member")?.to_owned();
        let group_state_ref =
            self.resolved_group_state_ref(&credentials, &input.group, &group_did)?;
        let operation_id = format!(
            "op-{}",
            crate::internal::wire::common::generate_operation_id()
        );
        let prepared = self.mls_provider.remove_member_prepare(RemoveMemberInput {
            actor_did: self.client.did().as_str().to_owned(),
            device_id: device_id_for_client(self.client),
            group_did: group_did.clone(),
            member_did: member_did.clone(),
            group_state_ref: group_state_ref.clone(),
            operation_id: operation_id.clone(),
            request_id: format!("group-e2ee-remove-{operation_id}"),
            pending_commit_id: Some(format!("pc-{operation_id}")),
        })?;
        let params = super::wire::build_group_e2ee_remove_rpc_params(
            &credentials,
            self.client.did().as_str(),
            &group_did,
            &member_did,
            &prepared,
            group_state_ref.as_ref(),
            input.reason_text.as_deref(),
            input.leave_request_id.as_deref(),
        )?;
        let delivery = match self.transport.authenticated_rpc(
            crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT,
            "group.e2ee.remove",
            params,
        ) {
            Ok(delivery) => delivery,
            Err(err) => return self.service_rejected_prepared(&prepared, &group_did, err),
        };
        let mut warnings = Vec::new();
        match self.finalize_prepared(&prepared, &group_did, &delivery) {
            Ok(finalized) => persist_group_e2ee_summary(
                self.client,
                &group_did,
                summary_epoch(&finalized.epoch, Some(prepared.epoch.as_str())),
                group_state_version(&delivery, group_state_ref.as_ref()).as_deref(),
                Some(finalized.crypto_group_id_b64u.as_str()),
                finalized.epoch_authenticator.as_deref(),
                Some(prepared.suite.as_str()),
                Some(finalized.operation_id.as_str()),
                "active",
            ),
            Err(err) => warnings.push(format!(
                "group E2EE remove was accepted by service but local finalize failed: {err}"
            )),
        }
        Ok(GroupE2eeLifecycleResult {
            delivery: public_lifecycle_delivery(
                "secure_group_remove_member",
                &group_did,
                Some(&member_did),
                Some("removed"),
                &delivery,
            ),
            warnings,
        })
    }

    pub(crate) fn leave_secure_group(
        mut self,
        input: GroupE2eeLeaveInput,
    ) -> crate::ImResult<GroupE2eeLifecycleResult> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::GroupMessaging)?;
        let credentials = self.credentials(input.credentials)?;
        let group_did = require_group(input.group.as_str())?.to_owned();
        if !input.owner_leave_commit {
            let params = super::wire::build_group_e2ee_leave_request_rpc_params(
                &credentials,
                self.client.did().as_str(),
                &group_did,
                input.reason_text.as_deref(),
            )?;
            let delivery = self.transport.authenticated_rpc(
                crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT,
                "group.e2ee.leave_request",
                params,
            )?;
            return Ok(GroupE2eeLifecycleResult {
                delivery: public_lifecycle_delivery(
                    "secure_group_leave_request",
                    &group_did,
                    Some(self.client.did().as_str()),
                    Some("leave_requested"),
                    &delivery,
                ),
                warnings: vec![
                    "group E2EE leave request created; the group owner must process it before the MLS epoch advances"
                        .to_owned(),
                ],
            });
        }

        let group_state_ref =
            self.resolved_group_state_ref(&credentials, &input.group, &group_did)?;
        let operation_id = format!(
            "op-{}",
            crate::internal::wire::common::generate_operation_id()
        );
        let prepared = self.mls_provider.leave_prepare(LeaveGroupInput {
            actor_did: self.client.did().as_str().to_owned(),
            device_id: device_id_for_client(self.client),
            group_did: group_did.clone(),
            group_state_ref: group_state_ref.clone(),
            operation_id: operation_id.clone(),
            request_id: format!("group-e2ee-leave-{operation_id}"),
            pending_commit_id: Some(format!("pc-{operation_id}")),
        })?;
        let params = super::wire::build_group_e2ee_leave_rpc_params(
            &credentials,
            self.client.did().as_str(),
            &group_did,
            &prepared,
            group_state_ref.as_ref(),
        )?;
        let delivery = match self.transport.authenticated_rpc(
            crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT,
            "group.e2ee.leave",
            params,
        ) {
            Ok(delivery) => delivery,
            Err(err) => return self.service_rejected_prepared(&prepared, &group_did, err),
        };
        let mut warnings = Vec::new();
        if let Err(err) = self.finalize_prepared(&prepared, &group_did, &delivery) {
            warnings.push(format!(
                "group E2EE leave was accepted by service but local finalize failed: {err}"
            ));
        }
        persist_group_e2ee_summary(
            self.client,
            &group_did,
            Some(prepared.epoch.as_str()),
            group_state_version(&delivery, group_state_ref.as_ref()).as_deref(),
            Some(prepared.crypto_group_id_b64u.as_str()),
            prepared
                .epoch_authenticator
                .as_deref()
                .or(prepared.epoch_authenticator_b64u.as_deref()),
            Some(prepared.suite.as_str()),
            Some(prepared.operation_id.as_str()),
            "left",
        );
        Ok(GroupE2eeLifecycleResult {
            delivery: public_lifecycle_delivery(
                "secure_group_leave",
                &group_did,
                Some(self.client.did().as_str()),
                Some("left"),
                &delivery,
            ),
            warnings,
        })
    }

    fn credentials(
        &self,
        credentials: Option<GroupTextCredentials>,
    ) -> crate::ImResult<GroupTextCredentials> {
        credentials
            .map(Ok)
            .unwrap_or_else(|| load_credentials(self.client))
    }

    fn service_did(
        &self,
        service_did: Option<crate::ids::Did>,
    ) -> crate::ImResult<crate::ids::Did> {
        if let Some(service_did) = service_did {
            return Ok(service_did);
        }
        self.client
            .core_inner()
            .sdk_config()
            .anp_service_did
            .clone()
            .ok_or_else(|| {
                crate::ImError::invalid_input(
                    Some("anp_service_did".to_owned()),
                    "group E2EE lifecycle requires ImCoreConfig.anp_service_did",
                )
            })
    }

    fn lookup_member_key_package(
        &mut self,
        credentials: &GroupTextCredentials,
        service_did: &str,
        group_did: &str,
        member_did: &str,
    ) -> crate::ImResult<GroupKeyPackage> {
        let params = super::wire::build_group_e2ee_get_key_package_rpc_params(
            credentials,
            self.client.did().as_str(),
            service_did,
            group_did,
            member_did,
        )?;
        let raw = self.transport.authenticated_rpc(
            crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT,
            "group.e2ee.get_key_package",
            params,
        )?;
        group_key_package_from_value(&raw)
    }

    fn resolved_group_state_ref(
        &mut self,
        credentials: &GroupTextCredentials,
        group: &crate::ids::GroupRef,
        group_did: &str,
    ) -> crate::ImResult<Option<GroupStateRef>> {
        if let Some(reference) = local_group_state_ref(self.client, group_did) {
            return Ok(Some(reference));
        }
        match super::state_ref::resolve_group_state_ref(
            self.client,
            &self.session_provider,
            &mut self.transport,
            &self.mls_provider,
            super::state_ref::ResolveGroupStateRef {
                group: group.clone(),
                credentials: Some(credentials.clone()),
            },
        ) {
            Ok(result) => Ok(Some(result.group_state_ref)),
            Err(crate::ImError::LocalStateUnavailable { .. }) => Ok(None),
            Err(err) => Err(err),
        }
    }

    fn finalize_prepared(
        &self,
        prepared: &anp::group_e2ee::operations::PreparedMlsCommitOutput,
        group_did: &str,
        delivery: &Value,
    ) -> crate::ImResult<anp::group_e2ee::operations::FinalizeCommitOutput> {
        if !service_delivery_accepts_commit(prepared, delivery) {
            return Err(crate::ImError::Service {
                status_code: None,
                code: Some("group_e2ee_not_accepted".to_owned()),
                message: "group E2EE service response did not accept the prepared commit"
                    .to_owned(),
            });
        }
        let finalized = self.mls_provider.finalize_commit(FinalizeCommitInput {
            pending_commit_id: prepared.pending_commit_id.clone(),
            request_id: format!(
                "group-e2ee-finalize-{}",
                crate::internal::wire::common::generate_operation_id()
            ),
        })?;
        if finalized.group_did.trim().is_empty() || finalized.group_did == group_did {
            return Ok(finalized);
        }
        Err(crate::ImError::Internal {
            message: format!(
                "group E2EE finalized unexpected group {} while handling {group_did}",
                finalized.group_did
            ),
        })
    }

    fn service_rejected_prepared(
        &self,
        prepared: &anp::group_e2ee::operations::PreparedMlsCommitOutput,
        group_did: &str,
        err: crate::ImError,
    ) -> crate::ImResult<GroupE2eeLifecycleResult> {
        if should_abort_pending_commit(&err) {
            match self.mls_provider.abort_commit(AbortCommitInput {
                pending_commit_id: prepared.pending_commit_id.clone(),
                request_id: format!(
                    "group-e2ee-abort-{}",
                    crate::internal::wire::common::generate_operation_id()
                ),
            }) {
                Ok(_) => {
                    return Err(crate::ImError::Internal {
                        message: format!(
                            "{err}; local group E2EE pending commit for {group_did} was aborted"
                        ),
                    });
                }
                Err(abort_err) => {
                    return Err(crate::ImError::Internal {
                        message: format!(
                            "{err}; local group E2EE pending commit abort failed: {abort_err}"
                        ),
                    });
                }
            }
        }
        Err(crate::ImError::Internal {
            message: format!("{err}; local group E2EE pending commit retained for repair"),
        })
    }
}

fn group_key_package_from_value(value: &Value) -> crate::ImResult<GroupKeyPackage> {
    let candidate = value
        .get("group_key_package")
        .cloned()
        .unwrap_or_else(|| value.clone());
    serde_json::from_value(candidate).map_err(|err| crate::ImError::Serialization {
        detail: format!("decode group E2EE KeyPackage: {err}"),
    })
}

fn public_lifecycle_delivery(
    action: &str,
    group_did: &str,
    subject_did: Option<&str>,
    fallback_subject_status: Option<&str>,
    delivery: &Value,
) -> Value {
    let mut output = Map::new();
    insert_string(&mut output, "action", Some(action));
    output.insert("secure".to_owned(), Value::Bool(true));
    insert_string(&mut output, "group_did", Some(group_did));
    let member_did = subject_did
        .map(str::to_owned)
        .or_else(|| optional_string(delivery.get("member_did")))
        .or_else(|| optional_string(delivery.get("subject_did")));
    let subject_did = subject_did
        .map(str::to_owned)
        .or_else(|| optional_string(delivery.get("subject_did")))
        .or_else(|| optional_string(delivery.get("member_did")));
    let subject_status = optional_string(delivery.get("subject_status"))
        .or_else(|| fallback_subject_status.map(str::to_owned));
    let leave_request_id = optional_string(delivery.get("leave_request_id"));
    insert_string(&mut output, "member_did", member_did.as_deref());
    insert_string(&mut output, "subject_did", subject_did.as_deref());
    insert_string(&mut output, "subject_status", subject_status.as_deref());
    insert_string(&mut output, "leave_request_id", leave_request_id.as_deref());
    if let Some(accepted) = bool_value(delivery.get("accepted")) {
        output.insert("accepted".to_owned(), Value::Bool(accepted));
    }
    if let Some(accepted) = bool_value(delivery.get("final_acceptance")) {
        output.insert("final_acceptance".to_owned(), Value::Bool(accepted));
    }

    let mut state = Map::new();
    insert_string(
        &mut state,
        "epoch",
        lifecycle_delivery_epoch(delivery).as_deref(),
    );
    insert_string(
        &mut state,
        "group_state_version",
        group_state_version(delivery, None).as_deref(),
    );
    if !state.is_empty() {
        output.insert("group_state".to_owned(), Value::Object(state));
    }
    Value::Object(output)
}

fn lifecycle_delivery_epoch(delivery: &Value) -> Option<String> {
    first_non_empty_string(&[
        delivery.get("epoch"),
        delivery.get("to_epoch"),
        delivery
            .get("group_state_ref")
            .and_then(|reference| reference.get("epoch")),
        delivery
            .get("delivery")
            .and_then(|value| value.get("epoch")),
        delivery
            .get("e2ee_notice")
            .and_then(|notice| notice.get("to_epoch")),
        delivery
            .get("e2ee_notice")
            .and_then(|notice| notice.get("epoch")),
    ])
}

fn optional_string(value: Option<&Value>) -> Option<String> {
    value.and_then(string_from_value)
}

fn service_delivery_accepts_commit(
    prepared: &anp::group_e2ee::operations::PreparedMlsCommitOutput,
    delivery: &Value,
) -> bool {
    if bool_value(delivery.get("accepted")) == Some(false)
        || bool_value(delivery.get("final_acceptance")) == Some(false)
    {
        return false;
    }
    let service_epoch = first_i64(&[
        delivery.get("epoch"),
        delivery.get("to_epoch"),
        delivery
            .get("group_state_ref")
            .and_then(|reference| reference.get("epoch")),
        delivery
            .get("e2ee_notice")
            .and_then(|notice| notice.get("to_epoch")),
    ]);
    let prepared_epoch = parse_i64(&prepared.epoch).or_else(|| parse_i64(&prepared.to_epoch));
    match (service_epoch, prepared_epoch) {
        (Some(service_epoch), Some(prepared_epoch)) => service_epoch >= prepared_epoch,
        _ => true,
    }
}

fn should_abort_pending_commit(err: &crate::ImError) -> bool {
    match err {
        crate::ImError::Service {
            status_code: Some(status),
            ..
        } if *status >= 500 => false,
        crate::ImError::Service {
            status_code: Some(status),
            ..
        } if *status >= 400 => true,
        crate::ImError::Service {
            code: Some(code), ..
        } => code
            .parse::<i64>()
            .map(|code| code >= 2000)
            .unwrap_or_else(|_| {
                matches!(
                    code.as_str(),
                    "invalid_request"
                        | "invalid_argument"
                        | "permission_denied"
                        | "forbidden"
                        | "not_found"
                        | "conflict"
                )
            }),
        _ => false,
    }
}

fn group_state_version(delivery: &Value, local_ref: Option<&GroupStateRef>) -> Option<String> {
    first_non_empty_string(&[
        delivery
            .get("group_state_ref")
            .and_then(|reference| reference.get("group_state_version")),
        delivery.get("group_state_version"),
        delivery
            .get("delivery")
            .and_then(|value| value.get("group_state_version")),
        delivery
            .get("e2ee_notice")
            .and_then(|notice| notice.get("group_state_ref"))
            .and_then(|reference| reference.get("group_state_version")),
        delivery
            .get("e2ee_notice")
            .and_then(|notice| notice.get("group_state_version")),
    ])
    .or_else(|| {
        local_ref
            .map(|reference| reference.group_state_version.trim().to_owned())
            .filter(|value| !value.is_empty())
    })
}

fn summary_epoch<'a>(epoch: &'a str, fallback: Option<&'a str>) -> Option<&'a str> {
    if epoch.trim().is_empty() {
        fallback
    } else {
        Some(epoch)
    }
}

#[cfg(feature = "sqlite")]
fn persist_group_e2ee_summary(
    client: &crate::core::ImClient,
    group_did: &str,
    epoch: Option<&str>,
    group_state_version: Option<&str>,
    crypto_group_id_b64u: Option<&str>,
    epoch_authenticator: Option<&str>,
    suite: Option<&str>,
    operation_id: Option<&str>,
    membership_status: &str,
) {
    let Ok(connection) = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    ) else {
        return;
    };
    let mut group_e2ee = Map::new();
    insert_string(
        &mut group_e2ee,
        "crypto_group_id_b64u",
        crypto_group_id_b64u,
    );
    insert_string(&mut group_e2ee, "epoch", epoch);
    insert_string(&mut group_e2ee, "epoch_authenticator", epoch_authenticator);
    insert_string(&mut group_e2ee, "suite", suite);
    insert_string(&mut group_e2ee, "group_state_version", group_state_version);
    insert_string(&mut group_e2ee, "operation_id", operation_id);
    insert_string(
        &mut group_e2ee,
        "updated_at",
        Some(crate::internal::wire::common::now_rfc3339().as_str()),
    );
    let mut metadata = Map::new();
    metadata.insert(
        "message_security_profile".to_owned(),
        Value::String(super::wire::GROUP_E2EE_SECURITY_PROFILE.to_owned()),
    );
    metadata.insert("group_e2ee".to_owned(), Value::Object(group_e2ee));
    if let Some(group_state_version) = group_state_version.filter(|value| !value.trim().is_empty())
    {
        metadata.insert(
            "group_state_version".to_owned(),
            Value::String(group_state_version.to_owned()),
        );
    }
    let _ = crate::internal::local_state::groups::upsert_group(
        &connection,
        crate::internal::local_state::groups::GroupRecord {
            owner_identity_id: client.current_identity().id.as_str().to_owned(),
            owner_did: client.did().as_str().to_owned(),
            group_id: group_did.to_owned(),
            group_did: group_did.to_owned(),
            membership_status: if membership_status.trim().is_empty() {
                "active".to_owned()
            } else {
                membership_status.trim().to_owned()
            },
            metadata: Value::Object(metadata).to_string(),
            credential_name: client.current_identity().id.as_str().to_owned(),
            ..crate::internal::local_state::groups::GroupRecord::default()
        },
    );
}

#[cfg(not(feature = "sqlite"))]
fn persist_group_e2ee_summary(
    _client: &crate::core::ImClient,
    _group_did: &str,
    _epoch: Option<&str>,
    _group_state_version: Option<&str>,
    _crypto_group_id_b64u: Option<&str>,
    _epoch_authenticator: Option<&str>,
    _suite: Option<&str>,
    _operation_id: Option<&str>,
    _membership_status: &str,
) {
}

fn insert_string(map: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        map.insert(key.to_owned(), Value::String(value.to_owned()));
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

fn group_e2ee_availability_group_did(client: &crate::core::ImClient) -> String {
    format!(
        "did:wba:{}:groups:group-e2ee-preflight",
        client.core_inner().sdk_config().did_domain
    )
}

fn group_e2ee_service_did(client: &crate::core::ImClient) -> crate::ImResult<crate::ids::Did> {
    client
        .core_inner()
        .sdk_config()
        .anp_service_did
        .clone()
        .ok_or_else(|| {
            crate::ImError::invalid_input(
                Some("anp_service_did".to_owned()),
                "group E2EE lifecycle requires ImCoreConfig.anp_service_did",
            )
        })
}

pub(super) fn is_group_e2ee_service_disabled(err: &crate::ImError) -> bool {
    let crate::ImError::Service {
        code: Some(code),
        message,
        ..
    } = err
    else {
        return false;
    };
    if code != "1405" {
        return false;
    }
    let message = message.to_ascii_lowercase();
    message.contains("group e2ee contract-test apis are disabled")
        || message.contains("group e2ee p6 apis are disabled")
}

fn require_did<'a>(did: &'a str, field: &'static str) -> crate::ImResult<&'a str> {
    let did = did.trim();
    if did.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            format!("{field} DID is required"),
        ));
    }
    Ok(did)
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

fn bool_value(value: Option<&Value>) -> Option<bool> {
    match value {
        Some(Value::Bool(value)) => Some(*value),
        Some(Value::String(value)) if value.eq_ignore_ascii_case("true") => Some(true),
        Some(Value::String(value)) if value.eq_ignore_ascii_case("false") => Some(false),
        _ => None,
    }
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

fn parse_i64(value: &str) -> Option<i64> {
    value.trim().parse::<i64>().ok()
}

fn first_non_empty_string(values: &[Option<&Value>]) -> Option<String> {
    values
        .iter()
        .flatten()
        .find_map(|value| string_from_value(value))
}

fn string_from_value(value: &Value) -> Option<String> {
    match value {
        Value::String(text) if !text.trim().is_empty() => Some(text.trim().to_owned()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn value_object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

#[allow(dead_code)]
fn _group_state_ref_from_delivery(group_did: &str, delivery: &Value) -> Option<GroupStateRef> {
    group_state_ref_from_service_head(group_did, delivery).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal::auth::session::SessionProvider;
    use crate::internal::transport::AuthenticatedRpcTransport;
    use anp::group_e2ee::operations::{
        AbortCommitOutput, DecryptInput, DecryptOutput, EncryptInput, EncryptOutput,
        GenerateKeyPackageInput, GroupKeyPackageOutput, ProcessNoticeInput, ProcessNoticeOutput,
        ProcessWelcomeInput, ProcessWelcomeOutput, RecoverMemberInput, StatusInput, StatusOutput,
        UpdateMemberInput,
    };
    use serde_json::{json, Value};
    use std::cell::RefCell;
    use std::fs;
    use std::path::PathBuf;
    use std::rc::Rc;

    #[test]
    fn lifecycle_create_prepares_delivers_finalizes_and_persists_summary() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let provider = RecordingMlsProvider::new();
        let result = GroupE2eeLifecycleRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                responses: vec![(
                    "group.e2ee.create".to_owned(),
                    json!({
                        "accepted": true,
                        "group_did": "did:example:groups:e2ee",
                        "group_state_version": "state-0",
                        "epoch": "0"
                    }),
                )],
            },
            provider.clone(),
        )
        .create_secure_group(GroupE2eeCreateInput {
            group: crate::ids::GroupRef::parse("did:example:groups:e2ee").unwrap(),
            credentials: Some(fixture.credentials()),
            service_did: Some(crate::ids::Did::parse("did:example:service").unwrap()),
        })
        .unwrap();

        assert!(result.warnings.is_empty());
        assert_eq!(
            provider.created.borrow().as_slice(),
            ["did:example:groups:e2ee"]
        );
        assert_eq!(provider.finalized.borrow().as_slice(), ["pc-create"]);
        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].method, "group.e2ee.create");
        assert_eq!(
            calls[0].params["meta"]["target"],
            json!({"kind":"service","did":"did:example:service"})
        );
        assert_eq!(
            calls[0].params["body"]["group_did"],
            "did:example:groups:e2ee"
        );
        assert_eq!(calls[0].params["body"]["epoch"], "0");
        assert!(calls[0].params["body"].get("commit_b64u").is_none());
        assert!(
            stored_group_metadata(&fixture, &client, "did:example:groups:e2ee")
                .to_string()
                .contains("group-e2ee")
        );
    }

    #[test]
    fn lifecycle_add_gets_key_package_prepares_commit_and_finalizes() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let provider = RecordingMlsProvider::new();
        let result = GroupE2eeLifecycleRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                responses: vec![
                    (
                        "group.e2ee.get_key_package".to_owned(),
                        json!({"group_key_package": key_package_json("did:example:bob")}),
                    ),
                    (
                        "group.e2ee.add".to_owned(),
                        json!({
                            "accepted": true,
                            "group_did": "did:example:groups:e2ee",
                            "group_state_version": "state-2",
                            "epoch": "2"
                        }),
                    ),
                ],
            },
            provider.clone(),
        )
        .add_secure_member(GroupE2eeMemberMutationInput {
            group: crate::ids::GroupRef::parse("did:example:groups:e2ee").unwrap(),
            member: crate::ids::Did::parse("did:example:bob").unwrap(),
            reason_text: None,
            leave_request_id: None,
            credentials: Some(fixture.credentials()),
            service_did: Some(crate::ids::Did::parse("did:example:service").unwrap()),
        })
        .unwrap();

        assert!(result.warnings.is_empty());
        assert_eq!(
            provider.added.borrow().as_slice(),
            ["did:example:groups:e2ee:did:example:bob"]
        );
        assert_eq!(provider.finalized.borrow().as_slice(), ["pc-add"]);
        let calls = calls.borrow();
        assert_eq!(
            calls
                .iter()
                .map(|call| call.method.as_str())
                .collect::<Vec<_>>(),
            vec!["group.e2ee.get_key_package", "group.e2ee.add"]
        );
        assert_eq!(calls[0].params["body"]["target_did"], "did:example:bob");
        assert_eq!(calls[1].params["body"]["member_did"], "did:example:bob");
        assert_eq!(
            calls[1].params["body"]["subject_key_package_id"],
            "kp-did:example:bob"
        );
        assert_eq!(calls[1].params["body"]["commit_b64u"], "commit-add");
        assert_eq!(calls[1].params["body"]["welcome_b64u"], "welcome-add");
    }

    #[test]
    fn lifecycle_public_delivery_redacts_mls_artifacts_from_service_response() {
        let delivery = public_lifecycle_delivery(
            "secure_group_remove_member",
            "did:example:groups:e2ee",
            Some("did:example:bob"),
            Some("removed"),
            &json!({
                "accepted": true,
                "group_did": "did:example:groups:e2ee",
                "member_did": "did:example:bob",
                "subject_status": "removed",
                "epoch": "3",
                "group_state_ref": {
                    "group_state_version": "state-3",
                    "crypto_group_id_b64u": "secret-crypto-group"
                },
                "commit_b64u": "secret-commit",
                "welcome_b64u": "secret-welcome",
                "ratchet_tree_b64u": "secret-ratchet",
                "group_key_package": key_package_json("did:example:bob"),
                "e2ee_notice": {
                    "notice_type": "commit-delivery",
                    "commit_b64u": "secret-notice-commit"
                }
            }),
        );

        assert_eq!(delivery["action"], "secure_group_remove_member");
        assert_eq!(delivery["secure"], true);
        assert_eq!(delivery["group_did"], "did:example:groups:e2ee");
        assert_eq!(delivery["member_did"], "did:example:bob");
        assert_eq!(delivery["subject_status"], "removed");
        assert_eq!(delivery["group_state"]["epoch"], "3");
        assert_eq!(delivery["group_state"]["group_state_version"], "state-3");
        let encoded = delivery.to_string();
        for secret in [
            "secret-commit",
            "secret-welcome",
            "secret-ratchet",
            "secret-notice-commit",
            "secret-crypto-group",
            "mls_key_package_b64u",
            "group_key_package",
            "e2ee_notice",
        ] {
            assert!(!encoded.contains(secret), "{encoded}");
        }
    }

    #[test]
    fn lifecycle_remove_aborts_pending_commit_on_deterministic_service_rejection() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let provider = RecordingMlsProvider::new();
        let err = GroupE2eeLifecycleRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                responses: vec![(
                    "group.e2ee.remove".to_owned(),
                    json!({"error": {"code": "invalid_argument", "message": "bad remove"}}),
                )],
            },
            provider.clone(),
        )
        .remove_secure_member(GroupE2eeMemberMutationInput {
            group: crate::ids::GroupRef::parse("did:example:groups:e2ee").unwrap(),
            member: crate::ids::Did::parse("did:example:bob").unwrap(),
            reason_text: Some("cleanup".to_owned()),
            leave_request_id: Some("leave-1".to_owned()),
            credentials: Some(fixture.credentials()),
            service_did: None,
        })
        .unwrap_err();

        assert!(err.to_string().contains("was aborted"));
        assert_eq!(provider.removed.borrow().as_slice(), ["did:example:bob"]);
        assert_eq!(provider.aborted.borrow().as_slice(), ["pc-remove"]);
        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].method, "group.e2ee.remove");
        assert_eq!(calls[0].params["body"]["reason_text"], "cleanup");
        assert_eq!(calls[0].params["body"]["leave_request_id"], "leave-1");
    }

    #[test]
    fn lifecycle_remove_returns_redacted_public_delivery() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let provider = RecordingMlsProvider::new();
        let result = GroupE2eeLifecycleRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                responses: vec![(
                    "group.e2ee.remove".to_owned(),
                    json!({
                        "accepted": true,
                        "group_did": "did:example:groups:e2ee",
                        "member_did": "did:example:bob",
                        "subject_status": "removed",
                        "epoch": "3",
                        "group_state_ref": {
                            "group_state_version": "state-3",
                            "crypto_group_id_b64u": "secret-crypto-group"
                        },
                        "commit_b64u": "secret-service-commit",
                        "e2ee_notice": {
                            "notice_type": "commit-delivery",
                            "commit_b64u": "secret-notice-commit"
                        }
                    }),
                )],
            },
            provider,
        )
        .remove_secure_member(GroupE2eeMemberMutationInput {
            group: crate::ids::GroupRef::parse("did:example:groups:e2ee").unwrap(),
            member: crate::ids::Did::parse("did:example:bob").unwrap(),
            reason_text: None,
            leave_request_id: None,
            credentials: Some(fixture.credentials()),
            service_did: None,
        })
        .unwrap();

        assert_eq!(result.delivery["action"], "secure_group_remove_member");
        assert_eq!(result.delivery["accepted"], true);
        assert_eq!(result.delivery["member_did"], "did:example:bob");
        assert_eq!(result.delivery["group_state"]["epoch"], "3");
        assert_eq!(
            result.delivery["group_state"]["group_state_version"],
            "state-3"
        );
        let encoded = result.delivery.to_string();
        assert!(!encoded.contains("secret-service-commit"), "{encoded}");
        assert!(!encoded.contains("secret-notice-commit"), "{encoded}");
        assert!(!encoded.contains("secret-crypto-group"), "{encoded}");
        assert!(!encoded.contains("e2ee_notice"), "{encoded}");

        let calls = calls.borrow();
        assert_eq!(calls[0].method, "group.e2ee.remove");
        assert_eq!(calls[0].params["body"]["commit_b64u"], "commit-remove");
    }

    #[test]
    fn lifecycle_leave_request_uses_high_level_request_without_local_finalize() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let provider = RecordingMlsProvider::new();
        let result = GroupE2eeLifecycleRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                responses: vec![(
                    "group.e2ee.leave_request".to_owned(),
                    json!({"accepted": true, "leave_request_id": "leave-request-1"}),
                )],
            },
            provider.clone(),
        )
        .leave_secure_group(GroupE2eeLeaveInput {
            group: crate::ids::GroupRef::parse("did:example:groups:e2ee").unwrap(),
            reason_text: Some("bye".to_owned()),
            owner_leave_commit: false,
            credentials: Some(fixture.credentials()),
        })
        .unwrap();

        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.contains("owner must process")));
        assert!(provider.finalized.borrow().is_empty());
        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].method, "group.e2ee.leave_request");
        assert_eq!(calls[0].params["body"]["subject_status"], "leave_requested");
        assert_eq!(calls[0].params["body"]["reason_text"], "bye");
    }

    #[test]
    fn lifecycle_leave_request_returns_redacted_public_delivery() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let provider = RecordingMlsProvider::new();
        let result = GroupE2eeLifecycleRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                responses: vec![(
                    "group.e2ee.leave_request".to_owned(),
                    json!({
                        "accepted": true,
                        "leave_request_id": "leave-request-1",
                        "e2ee_notice": {
                            "notice_type": "leave-request",
                            "welcome_b64u": "secret-welcome"
                        }
                    }),
                )],
            },
            provider,
        )
        .leave_secure_group(GroupE2eeLeaveInput {
            group: crate::ids::GroupRef::parse("did:example:groups:e2ee").unwrap(),
            reason_text: Some("bye".to_owned()),
            owner_leave_commit: false,
            credentials: Some(fixture.credentials()),
        })
        .unwrap();

        assert_eq!(result.delivery["action"], "secure_group_leave_request");
        assert_eq!(result.delivery["accepted"], true);
        assert_eq!(result.delivery["leave_request_id"], "leave-request-1");
        assert_eq!(result.delivery["subject_status"], "leave_requested");
        let encoded = result.delivery.to_string();
        assert!(!encoded.contains("secret-welcome"), "{encoded}");
        assert!(!encoded.contains("e2ee_notice"), "{encoded}");

        let calls = calls.borrow();
        assert_eq!(calls[0].method, "group.e2ee.leave_request");
    }

    #[test]
    fn service_availability_preflight_returns_disabled_gate_error() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut transport = RecordingTransport {
            calls: Rc::clone(&calls),
            responses: vec![(
                "group.e2ee.head".to_owned(),
                json!({"error": {"code": "1405", "message": "group E2EE contract-test APIs are disabled"}}),
            )],
        };

        let err = ensure_group_e2ee_service_available(
            &client,
            &ReadySessionProvider,
            &mut transport,
            GroupE2eeServiceAvailabilityInput {
                credentials: Some(fixture.credentials()),
                service_did: Some(crate::ids::Did::parse("did:example:service").unwrap()),
                check_key_package: true,
            },
        )
        .unwrap_err();

        assert!(is_group_e2ee_service_disabled(&err));
        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].method, "group.e2ee.head");
    }

    #[test]
    fn service_availability_preflight_checks_key_package_gate_when_requested() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut transport = RecordingTransport {
            calls: Rc::clone(&calls),
            responses: vec![
                (
                    "group.e2ee.head".to_owned(),
                    json!({"error": {"code": "1404", "message": "group E2EE crypto head not found"}}),
                ),
                (
                    "group.e2ee.get_key_package".to_owned(),
                    json!({"error": {"code": "1405", "message": "group E2EE P6 APIs are disabled"}}),
                ),
            ],
        };

        let err = ensure_group_e2ee_service_available(
            &client,
            &ReadySessionProvider,
            &mut transport,
            GroupE2eeServiceAvailabilityInput {
                credentials: Some(fixture.credentials()),
                service_did: Some(crate::ids::Did::parse("did:example:service").unwrap()),
                check_key_package: true,
            },
        )
        .unwrap_err();

        assert!(is_group_e2ee_service_disabled(&err));
        let calls = calls.borrow();
        assert_eq!(
            calls
                .iter()
                .map(|call| call.method.as_str())
                .collect::<Vec<_>>(),
            vec!["group.e2ee.head", "group.e2ee.get_key_package"]
        );
    }

    #[test]
    fn service_availability_preflight_ignores_non_gate_errors() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut transport = RecordingTransport {
            calls: Rc::clone(&calls),
            responses: vec![
                (
                    "group.e2ee.head".to_owned(),
                    json!({"error": {"code": "1404", "message": "group E2EE crypto head not found"}}),
                ),
                (
                    "group.e2ee.get_key_package".to_owned(),
                    json!({"error": {"code": "1403", "message": "group.e2ee.get_key_package purpose=normal requires active owner role"}}),
                ),
            ],
        };

        ensure_group_e2ee_service_available(
            &client,
            &ReadySessionProvider,
            &mut transport,
            GroupE2eeServiceAvailabilityInput {
                credentials: Some(fixture.credentials()),
                service_did: Some(crate::ids::Did::parse("did:example:service").unwrap()),
                check_key_package: true,
            },
        )
        .unwrap();

        let calls = calls.borrow();
        assert_eq!(
            calls
                .iter()
                .map(|call| call.method.as_str())
                .collect::<Vec<_>>(),
            vec!["group.e2ee.head", "group.e2ee.get_key_package"]
        );
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
            unreachable!("group E2EE lifecycle should not refresh through the session provider")
        }

        fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
            unreachable!("group E2EE lifecycle should not read auth status")
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

    #[derive(Clone)]
    struct RecordingMlsProvider {
        created: Rc<RefCell<Vec<String>>>,
        added: Rc<RefCell<Vec<String>>>,
        removed: Rc<RefCell<Vec<String>>>,
        finalized: Rc<RefCell<Vec<String>>>,
        aborted: Rc<RefCell<Vec<String>>>,
    }

    impl RecordingMlsProvider {
        fn new() -> Self {
            Self {
                created: Rc::new(RefCell::new(Vec::new())),
                added: Rc::new(RefCell::new(Vec::new())),
                removed: Rc::new(RefCell::new(Vec::new())),
                finalized: Rc::new(RefCell::new(Vec::new())),
                aborted: Rc::new(RefCell::new(Vec::new())),
            }
        }
    }

    impl GroupMlsProvider for RecordingMlsProvider {
        fn create_group_prepare(
            &self,
            input: CreateGroupInput,
        ) -> crate::ImResult<anp::group_e2ee::operations::PreparedMlsCommitOutput> {
            self.created.borrow_mut().push(input.group_did.clone());
            Ok(prepared(
                "pc-create",
                input.operation_id,
                "0",
                "0",
                "active",
            ))
        }

        fn add_member_prepare(
            &self,
            input: AddMemberInput,
        ) -> crate::ImResult<anp::group_e2ee::operations::PreparedMlsCommitOutput> {
            self.added
                .borrow_mut()
                .push(format!("{}:{}", input.group_did, input.member_did));
            let mut output = prepared("pc-add", input.operation_id, "1", "2", "active");
            output.subject_did = input.member_did;
            output.member_did = Some(output.subject_did.clone());
            output.commit_b64u = "commit-add".to_owned();
            output.welcome_b64u = Some("welcome-add".to_owned());
            output.ratchet_tree_b64u = Some("ratchet-tree-add".to_owned());
            Ok(output)
        }

        fn remove_member_prepare(
            &self,
            input: RemoveMemberInput,
        ) -> crate::ImResult<anp::group_e2ee::operations::PreparedMlsCommitOutput> {
            self.removed.borrow_mut().push(input.member_did.clone());
            let mut output = prepared("pc-remove", input.operation_id, "2", "3", "removed");
            output.subject_did = input.member_did;
            output.commit_b64u = "commit-remove".to_owned();
            Ok(output)
        }

        fn leave_prepare(
            &self,
            input: LeaveGroupInput,
        ) -> crate::ImResult<anp::group_e2ee::operations::PreparedMlsCommitOutput> {
            let mut output = prepared("pc-leave", input.operation_id, "3", "4", "left");
            output.subject_did = input.actor_did;
            output.commit_b64u = "commit-leave".to_owned();
            Ok(output)
        }

        fn finalize_commit(
            &self,
            input: FinalizeCommitInput,
        ) -> crate::ImResult<anp::group_e2ee::operations::FinalizeCommitOutput> {
            self.finalized
                .borrow_mut()
                .push(input.pending_commit_id.clone());
            Ok(anp::group_e2ee::operations::FinalizeCommitOutput {
                pending_commit_id: input.pending_commit_id,
                operation_id: "op-finalized".to_owned(),
                group_did: "did:example:groups:e2ee".to_owned(),
                crypto_group_id_b64u: "crypto".to_owned(),
                status: "finalized".to_owned(),
                from_epoch: "0".to_owned(),
                epoch: "2".to_owned(),
                local_epoch: "2".to_owned(),
                subject_did: "did:example:bob".to_owned(),
                subject_status: "active".to_owned(),
                epoch_authenticator: Some("auth".to_owned()),
            })
        }

        fn abort_commit(&self, input: AbortCommitInput) -> crate::ImResult<AbortCommitOutput> {
            self.aborted
                .borrow_mut()
                .push(input.pending_commit_id.clone());
            Ok(AbortCommitOutput {
                pending_commit_id: input.pending_commit_id,
                operation_id: "op-abort".to_owned(),
                group_did: "did:example:groups:e2ee".to_owned(),
                crypto_group_id_b64u: "crypto".to_owned(),
                status: "aborted".to_owned(),
                local_epoch: "2".to_owned(),
                subject_did: "did:example:bob".to_owned(),
                subject_status: "removed".to_owned(),
            })
        }

        fn status(&self, _input: StatusInput) -> crate::ImResult<StatusOutput> {
            Err(crate::ImError::LocalStateUnavailable {
                detail: "test has no local MLS state".to_owned(),
            })
        }

        fn generate_key_package(
            &self,
            _input: GenerateKeyPackageInput,
        ) -> crate::ImResult<GroupKeyPackageOutput> {
            unreachable!("lifecycle should lease member key packages from service")
        }

        fn update_member_prepare(
            &self,
            _input: UpdateMemberInput,
        ) -> crate::ImResult<anp::group_e2ee::operations::PreparedMlsCommitOutput> {
            unreachable!("lifecycle should not update members")
        }

        fn recover_member_prepare(
            &self,
            _input: RecoverMemberInput,
        ) -> crate::ImResult<anp::group_e2ee::operations::PreparedMlsCommitOutput> {
            unreachable!("lifecycle should not recover members")
        }

        fn process_welcome(
            &self,
            _input: ProcessWelcomeInput,
        ) -> crate::ImResult<ProcessWelcomeOutput> {
            unreachable!("lifecycle should not process welcomes")
        }

        fn process_notice(
            &self,
            _input: ProcessNoticeInput,
        ) -> crate::ImResult<ProcessNoticeOutput> {
            unreachable!("lifecycle should not process notices")
        }

        fn encrypt(&self, _input: EncryptInput) -> crate::ImResult<EncryptOutput> {
            unreachable!("lifecycle should not encrypt")
        }

        fn decrypt(&self, _input: DecryptInput) -> crate::ImResult<DecryptOutput> {
            unreachable!("lifecycle should not decrypt")
        }
    }

    fn prepared(
        pending_commit_id: &str,
        operation_id: String,
        from_epoch: &str,
        epoch: &str,
        subject_status: &str,
    ) -> anp::group_e2ee::operations::PreparedMlsCommitOutput {
        anp::group_e2ee::operations::PreparedMlsCommitOutput {
            pending_commit_id: pending_commit_id.to_owned(),
            operation_id,
            status: "pending".to_owned(),
            actor_did: "did:example:alice".to_owned(),
            subject_did: "did:example:alice".to_owned(),
            subject_status: subject_status.to_owned(),
            group_did: "did:example:groups:e2ee".to_owned(),
            crypto_group_id_b64u: "crypto".to_owned(),
            from_epoch: from_epoch.to_owned(),
            epoch: epoch.to_owned(),
            to_epoch: epoch.to_owned(),
            local_epoch: from_epoch.to_owned(),
            commit_b64u: "commit".to_owned(),
            welcome_b64u: None,
            ratchet_tree_b64u: None,
            group_info_b64u: Some("group-info".to_owned()),
            epoch_authenticator: Some("auth".to_owned()),
            epoch_authenticator_b64u: None,
            suite: "MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519".to_owned(),
            member_did: None,
        }
    }

    fn key_package_json(owner: &str) -> Value {
        json!({
            "key_package_id": format!("kp-{owner}"),
            "owner_did": owner,
            "device_id": "default",
            "suite": "MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519",
            "mls_key_package_b64u": "a2V5LXBhY2thZ2U",
            "did_wba_binding": {"did": owner},
            "expires_at": "2026-05-25T00:00:00Z",
            "non_cryptographic": true
        })
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
            let local = root.join("local");
            fs::create_dir_all(&local).unwrap();
            let connection = rusqlite::Connection::open(local.join("im.sqlite")).unwrap();
            crate::internal::local_state::schema::ensure_schema(&connection).unwrap();
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
                    anp_service_did: Some(crate::ids::Did::parse("did:example:service").unwrap()),
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
                    challenge: Some("group-e2ee-lifecycle-test".to_owned()),
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

    fn unique_temp_root() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "im-core-group-e2ee-lifecycle-{}-{nanos}",
            std::process::id()
        ))
    }
}
