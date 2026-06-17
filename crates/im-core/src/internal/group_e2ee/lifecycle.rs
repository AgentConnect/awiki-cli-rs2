use anp::group_e2ee::operations::{
    AbortCommitInput, AddMemberInput, CreateGroupInput, FinalizeCommitInput, LeaveGroupInput,
    RecoverMemberInput, RemoveMemberInput, UpdateMemberInput,
};
use anp::group_e2ee::{GroupKeyPackage, GroupStateRef};
use serde_json::{Map, Value};

use crate::internal::auth::session::{AsyncSessionProvider, SessionProvider};
use crate::internal::message_runtime::group::{
    load_credentials, load_credentials_async, GroupTextCredentials,
};
use crate::internal::transport::{AsyncAuthenticatedRpcTransport, AuthenticatedRpcTransport};

use super::provider::GroupMlsProvider;
use super::state_ref::{
    group_state_ref_from_service_head, local_group_state_ref, local_group_state_ref_async,
};
use super::summary::{
    persist_group_e2ee_summary, persist_group_e2ee_summary_async, GroupE2eeSummaryUpdate,
};
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
    pub(crate) group_state_ref: Option<GroupStateRef>,
}

#[derive(Debug, Clone)]
pub(crate) struct GroupE2eeMemberMutationInput {
    pub(crate) group: crate::ids::GroupRef,
    pub(crate) member: crate::ids::Did,
    pub(crate) reason_text: Option<String>,
    pub(crate) leave_request_id: Option<String>,
    pub(crate) credentials: Option<GroupTextCredentials>,
    pub(crate) service_did: Option<crate::ids::Did>,
    pub(crate) group_state_ref: Option<GroupStateRef>,
}

#[derive(Debug, Clone)]
pub(crate) struct GroupE2eeKeyReplacementInput {
    pub(crate) group: crate::ids::GroupRef,
    pub(crate) member: crate::ids::Did,
    pub(crate) device_id: String,
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
        None,
        None,
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

pub(crate) async fn ensure_group_e2ee_service_available_async<P, T>(
    client: &crate::core::ImClient,
    session_provider: &P,
    transport: &mut T,
    input: GroupE2eeServiceAvailabilityInput,
) -> crate::ImResult<()>
where
    P: AsyncSessionProvider,
    T: AsyncAuthenticatedRpcTransport,
{
    session_provider
        .ensure_session(crate::auth::AuthScope::GroupMessaging)
        .await?;
    let credentials = match input.credentials {
        Some(credentials) => credentials,
        None => load_credentials_async(client).await?,
    };
    let preflight_group_did = group_e2ee_availability_group_did(client);
    let head_params = super::wire::build_group_e2ee_head_rpc_params(
        &credentials,
        client.did().as_str(),
        &preflight_group_did,
    )?;
    match transport
        .authenticated_rpc(
            crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT,
            "group.e2ee.head",
            head_params,
        )
        .await
    {
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
        None,
        None,
    )?;
    match transport
        .authenticated_rpc(
            crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT,
            "group.e2ee.get_key_package",
            key_package_params,
        )
        .await
    {
        Ok(_) => Ok(()),
        Err(err) if is_group_e2ee_service_disabled(&err) => Err(err),
        Err(_) => Ok(()),
    }
}

pub(crate) async fn leave_secure_group_request_async<P, T>(
    client: &crate::core::ImClient,
    session_provider: &P,
    transport: &mut T,
    input: GroupE2eeLeaveInput,
) -> crate::ImResult<GroupE2eeLifecycleResult>
where
    P: AsyncSessionProvider,
    T: AsyncAuthenticatedRpcTransport,
{
    if input.owner_leave_commit {
        return Err(crate::ImError::unsupported(
            "group-e2ee-owner-leave-commit-async",
        ));
    }
    session_provider
        .ensure_session(crate::auth::AuthScope::GroupMessaging)
        .await?;
    let credentials = match input.credentials {
        Some(credentials) => credentials,
        None => load_credentials_async(client).await?,
    };
    let group_did = require_group(input.group.as_str())?.to_owned();
    let params = super::wire::build_group_e2ee_leave_request_rpc_params(
        &credentials,
        client.did().as_str(),
        &group_did,
        input.reason_text.as_deref(),
    )?;
    let delivery = transport
        .authenticated_rpc(
            crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT,
            "group.e2ee.leave_request",
            params,
        )
        .await?;
    Ok(GroupE2eeLifecycleResult {
        delivery: public_lifecycle_delivery(
            "secure_group_leave_request",
            &group_did,
            Some(client.did().as_str()),
            Some("leave_requested"),
            &delivery,
        ),
        warnings: vec![
            "group E2EE leave request created; the group owner must process it before the MLS epoch advances"
                .to_owned(),
        ],
    })
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
        let service_did = input
            .service_did
            .map(Ok)
            .unwrap_or_else(|| group_e2ee_service_did(self.client))?;
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
        let group_state_ref = input
            .group_state_ref
            .or_else(|| local_group_state_ref(self.client, &group_did));
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
                GroupE2eeSummaryUpdate {
                    group_did: &group_did,
                    epoch: summary_epoch(&finalized.epoch, Some(prepared.epoch.as_str())),
                    group_state_version: group_state_version(&delivery, group_state_ref.as_ref())
                        .as_deref(),
                    crypto_group_id_b64u: Some(finalized.crypto_group_id_b64u.as_str()),
                    epoch_authenticator: finalized.epoch_authenticator.as_deref(),
                    suite: Some(prepared.suite.as_str()),
                    operation_id: Some(finalized.operation_id.as_str()),
                    membership_status: "active",
                },
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
        let service_did = input
            .service_did
            .map(Ok)
            .unwrap_or_else(|| group_e2ee_service_did(self.client))?;
        let group_did = require_group(input.group.as_str())?.to_owned();
        let member_did = require_did(input.member.as_str(), "member")?.to_owned();
        let key_package = self.lookup_member_key_package(
            &credentials,
            service_did.as_str(),
            &group_did,
            &member_did,
            "normal",
            None,
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
        let group_state_ref = input
            .group_state_ref
            .or_else(|| local_group_state_ref(self.client, &group_did));
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
            Err(err) => {
                return service_rejected_prepared(&self.mls_provider, &prepared, &group_did, err)
            }
        };
        let mut warnings = Vec::new();
        match finalize_prepared(&self.mls_provider, &prepared, &group_did, &delivery) {
            Ok(finalized) => persist_group_e2ee_summary(
                self.client,
                GroupE2eeSummaryUpdate {
                    group_did: &group_did,
                    epoch: summary_epoch(&finalized.epoch, Some(prepared.epoch.as_str())),
                    group_state_version: group_state_version(&delivery, group_state_ref.as_ref())
                        .as_deref(),
                    crypto_group_id_b64u: Some(finalized.crypto_group_id_b64u.as_str()),
                    epoch_authenticator: finalized.epoch_authenticator.as_deref(),
                    suite: Some(prepared.suite.as_str()),
                    operation_id: Some(finalized.operation_id.as_str()),
                    membership_status: "active",
                },
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
        let group_state_ref = match input.group_state_ref.clone() {
            Some(reference) => Some(reference),
            None => self.resolved_group_state_ref(&credentials, &input.group, &group_did)?,
        };
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
            Err(err) => {
                return service_rejected_prepared(&self.mls_provider, &prepared, &group_did, err)
            }
        };
        let mut warnings = Vec::new();
        match finalize_prepared(&self.mls_provider, &prepared, &group_did, &delivery) {
            Ok(finalized) => persist_group_e2ee_summary(
                self.client,
                GroupE2eeSummaryUpdate {
                    group_did: &group_did,
                    epoch: summary_epoch(&finalized.epoch, Some(prepared.epoch.as_str())),
                    group_state_version: group_state_version(&delivery, group_state_ref.as_ref())
                        .as_deref(),
                    crypto_group_id_b64u: Some(finalized.crypto_group_id_b64u.as_str()),
                    epoch_authenticator: finalized.epoch_authenticator.as_deref(),
                    suite: Some(prepared.suite.as_str()),
                    operation_id: Some(finalized.operation_id.as_str()),
                    membership_status: "active",
                },
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
            GroupE2eeSummaryUpdate {
                group_did: &group_did,
                epoch: Some(prepared.epoch.as_str()),
                group_state_version: group_state_version(&delivery, group_state_ref.as_ref())
                    .as_deref(),
                crypto_group_id_b64u: Some(prepared.crypto_group_id_b64u.as_str()),
                epoch_authenticator: prepared
                    .epoch_authenticator
                    .as_deref()
                    .or(prepared.epoch_authenticator_b64u.as_deref()),
                suite: Some(prepared.suite.as_str()),
                operation_id: Some(prepared.operation_id.as_str()),
                membership_status: "left",
            },
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

    pub(crate) fn update_member_key(
        mut self,
        input: GroupE2eeKeyReplacementInput,
    ) -> crate::ImResult<GroupE2eeLifecycleResult> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::GroupMessaging)?;
        let credentials = self.credentials(input.credentials)?;
        let service_did = input
            .service_did
            .map(Ok)
            .unwrap_or_else(|| group_e2ee_service_did(self.client))?;
        let group_did = require_group(input.group.as_str())?.to_owned();
        let member_did = require_did(input.member.as_str(), "member")?.to_owned();
        let device_id = normalize_device_id(&input.device_id);
        let key_package = self.lookup_member_key_package(
            &credentials,
            service_did.as_str(),
            &group_did,
            &member_did,
            "update",
            Some(&device_id),
        )?;
        let group_state_ref =
            self.resolved_group_state_ref(&credentials, &input.group, &group_did)?;
        let operation_id = format!(
            "op-{}",
            crate::internal::wire::common::generate_operation_id()
        );
        let prepared = self.mls_provider.update_member_prepare(UpdateMemberInput {
            actor_did: self.client.did().as_str().to_owned(),
            device_id: device_id_for_client(self.client),
            group_did: group_did.clone(),
            member_did: member_did.clone(),
            target_device_id: device_id.clone(),
            group_key_package: key_package.clone(),
            group_state_ref: group_state_ref.clone(),
            update_key_package_id: Some(key_package.key_package_id.clone()),
            operation_id: operation_id.clone(),
            request_id: format!("group-e2ee-update-{operation_id}"),
            pending_commit_id: Some(format!("pc-{operation_id}")),
        })?;
        let params = super::wire::build_group_e2ee_update_rpc_params(
            &credentials,
            self.client.did().as_str(),
            &group_did,
            &member_did,
            &device_id,
            &prepared,
            &key_package,
            group_state_ref.as_ref(),
        )?;
        let delivery = match self.transport.authenticated_rpc(
            crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT,
            "group.e2ee.update",
            params,
        ) {
            Ok(delivery) => delivery,
            Err(err) => {
                return service_rejected_prepared(&self.mls_provider, &prepared, &group_did, err)
            }
        };
        let mut warnings = Vec::new();
        match finalize_prepared(&self.mls_provider, &prepared, &group_did, &delivery) {
            Ok(finalized) => persist_group_e2ee_summary(
                self.client,
                GroupE2eeSummaryUpdate {
                    group_did: &group_did,
                    epoch: summary_epoch(&finalized.epoch, Some(prepared.epoch.as_str())),
                    group_state_version: group_state_version(&delivery, group_state_ref.as_ref())
                        .as_deref(),
                    crypto_group_id_b64u: Some(finalized.crypto_group_id_b64u.as_str()),
                    epoch_authenticator: finalized.epoch_authenticator.as_deref(),
                    suite: Some(prepared.suite.as_str()),
                    operation_id: Some(finalized.operation_id.as_str()),
                    membership_status: "active",
                },
            ),
            Err(err) => warnings.push(format!(
                "group E2EE update was accepted by service but local finalize failed: {err}"
            )),
        }
        Ok(GroupE2eeLifecycleResult {
            delivery: public_lifecycle_delivery(
                "secure_group_update_key",
                &group_did,
                Some(&member_did),
                Some("active"),
                &delivery,
            ),
            warnings,
        })
    }

    pub(crate) fn recover_member(
        mut self,
        input: GroupE2eeKeyReplacementInput,
    ) -> crate::ImResult<GroupE2eeLifecycleResult> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::GroupMessaging)?;
        let credentials = self.credentials(input.credentials)?;
        let service_did = self.service_did(input.service_did)?;
        let group_did = require_group(input.group.as_str())?.to_owned();
        let member_did = require_did(input.member.as_str(), "member")?.to_owned();
        let device_id = normalize_device_id(&input.device_id);
        let key_package = self.lookup_member_key_package(
            &credentials,
            service_did.as_str(),
            &group_did,
            &member_did,
            "recovery",
            Some(&device_id),
        )?;
        let group_state_ref =
            self.resolved_group_state_ref(&credentials, &input.group, &group_did)?;
        let operation_id = format!(
            "op-{}",
            crate::internal::wire::common::generate_operation_id()
        );
        let prepared = self
            .mls_provider
            .recover_member_prepare(RecoverMemberInput {
                actor_did: self.client.did().as_str().to_owned(),
                device_id: device_id_for_client(self.client),
                group_did: group_did.clone(),
                member_did: member_did.clone(),
                target_device_id: device_id.clone(),
                group_key_package: key_package.clone(),
                group_state_ref: group_state_ref.clone(),
                operation_id: operation_id.clone(),
                request_id: format!("group-e2ee-recover-{operation_id}"),
                pending_commit_id: Some(format!("pc-{operation_id}")),
            })?;
        let params = super::wire::build_group_e2ee_recover_member_rpc_params(
            &credentials,
            self.client.did().as_str(),
            &group_did,
            &member_did,
            &device_id,
            &prepared,
            &key_package,
            group_state_ref.as_ref(),
        )?;
        let delivery = match self.transport.authenticated_rpc(
            crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT,
            "group.e2ee.recover_member",
            params,
        ) {
            Ok(delivery) => delivery,
            Err(err) => return self.service_rejected_prepared(&prepared, &group_did, err),
        };
        let mut warnings = Vec::new();
        match self.finalize_prepared(&prepared, &group_did, &delivery) {
            Ok(finalized) => persist_group_e2ee_summary(
                self.client,
                GroupE2eeSummaryUpdate {
                    group_did: &group_did,
                    epoch: summary_epoch(&finalized.epoch, Some(prepared.epoch.as_str())),
                    group_state_version: group_state_version(&delivery, group_state_ref.as_ref())
                        .as_deref(),
                    crypto_group_id_b64u: Some(finalized.crypto_group_id_b64u.as_str()),
                    epoch_authenticator: finalized.epoch_authenticator.as_deref(),
                    suite: Some(prepared.suite.as_str()),
                    operation_id: Some(finalized.operation_id.as_str()),
                    membership_status: "active",
                },
            ),
            Err(err) => warnings.push(format!(
                "group E2EE recover-member was accepted by service but local finalize failed: {err}"
            )),
        }
        Ok(GroupE2eeLifecycleResult {
            delivery: public_lifecycle_delivery(
                "secure_group_recover_member",
                &group_did,
                Some(&member_did),
                Some("active"),
                &delivery,
            ),
            warnings,
        })
    }

    pub(crate) fn process_leave_request(
        mut self,
        input: GroupE2eeMemberMutationInput,
    ) -> crate::ImResult<GroupE2eeLifecycleResult> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::GroupMessaging)?;
        let credentials = self.credentials(input.credentials)?;
        let group_did = require_group(input.group.as_str())?.to_owned();
        let member_did = require_did(input.member.as_str(), "member")?.to_owned();
        let leave_request_id = input
            .leave_request_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                crate::ImError::invalid_input(
                    Some("leave_request_id".to_owned()),
                    "group E2EE process leave request requires leave_request_id",
                )
            })?
            .to_owned();
        let params = super::wire::build_group_e2ee_process_leave_request_rpc_params(
            &credentials,
            self.client.did().as_str(),
            &group_did,
            &leave_request_id,
        )?;
        let processing = self.transport.authenticated_rpc(
            crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT,
            "group.e2ee.process_leave_request",
            params,
        )?;
        let final_result = self.remove_secure_member(GroupE2eeMemberMutationInput {
            group: input.group,
            member: crate::ids::Did::parse(&member_did)?,
            reason_text: input.reason_text,
            leave_request_id: Some(leave_request_id),
            credentials: Some(credentials),
            service_did: input.service_did,
            group_state_ref: input.group_state_ref,
        })?;
        Ok(GroupE2eeLifecycleResult {
            delivery: merge_process_leave_delivery(final_result.delivery, &processing),
            warnings: final_result.warnings,
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
        purpose: &str,
        device_id: Option<&str>,
    ) -> crate::ImResult<GroupKeyPackage> {
        let params = super::wire::build_group_e2ee_get_key_package_rpc_params(
            credentials,
            self.client.did().as_str(),
            service_did,
            group_did,
            member_did,
            Some(purpose),
            device_id,
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
        match super::state_ref::resolve_group_state_ref_local_first(
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
        finalize_prepared(&self.mls_provider, prepared, group_did, delivery)
    }

    fn service_rejected_prepared(
        &self,
        prepared: &anp::group_e2ee::operations::PreparedMlsCommitOutput,
        group_did: &str,
        err: crate::ImError,
    ) -> crate::ImResult<GroupE2eeLifecycleResult> {
        service_rejected_prepared(&self.mls_provider, prepared, group_did, err)
    }
}

fn finalize_prepared<M>(
    mls_provider: &M,
    prepared: &anp::group_e2ee::operations::PreparedMlsCommitOutput,
    group_did: &str,
    delivery: &Value,
) -> crate::ImResult<anp::group_e2ee::operations::FinalizeCommitOutput>
where
    M: GroupMlsProvider,
{
    if !service_delivery_accepts_commit(prepared, delivery) {
        return Err(crate::ImError::Service {
            status_code: None,
            code: Some("group_e2ee_not_accepted".to_owned()),
            message: "group E2EE service response did not accept the prepared commit".to_owned(),
        });
    }
    let finalized = mls_provider.finalize_commit(FinalizeCommitInput {
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

fn service_rejected_prepared<M>(
    mls_provider: &M,
    prepared: &anp::group_e2ee::operations::PreparedMlsCommitOutput,
    group_did: &str,
    err: crate::ImError,
) -> crate::ImResult<GroupE2eeLifecycleResult>
where
    M: GroupMlsProvider,
{
    if should_abort_pending_commit(&err) {
        match mls_provider.abort_commit(AbortCommitInput {
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

async fn run_lifecycle_mls_blocking<F, T>(label: &'static str, operation: F) -> crate::ImResult<T>
where
    F: FnOnce() -> crate::ImResult<T> + Send + 'static,
    T: Send + 'static,
{
    crate::internal::runtime::worker::run_blocking(operation)
        .await
        .map_err(|err| crate::ImError::Internal {
            message: format!("group E2EE lifecycle MLS {label} worker failed: {err}"),
        })?
}

async fn finalize_prepared_async<M>(
    mls_provider: M,
    prepared: anp::group_e2ee::operations::PreparedMlsCommitOutput,
    group_did: String,
    delivery: Value,
) -> crate::ImResult<anp::group_e2ee::operations::FinalizeCommitOutput>
where
    M: GroupMlsProvider + Send + 'static,
{
    run_lifecycle_mls_blocking("finalize", move || {
        finalize_prepared(&mls_provider, &prepared, &group_did, &delivery)
    })
    .await
}

async fn service_rejected_prepared_async<M>(
    mls_provider: M,
    prepared: anp::group_e2ee::operations::PreparedMlsCommitOutput,
    group_did: String,
    err: crate::ImError,
) -> crate::ImResult<GroupE2eeLifecycleResult>
where
    M: GroupMlsProvider + Send + 'static,
{
    run_lifecycle_mls_blocking("abort", move || {
        service_rejected_prepared(&mls_provider, &prepared, &group_did, err)
    })
    .await
}

impl<P, T, M> GroupE2eeLifecycleRuntime<'_, P, T, M>
where
    P: AsyncSessionProvider,
    T: AsyncAuthenticatedRpcTransport,
    M: GroupMlsProvider + Clone + Send + 'static,
{
    pub(crate) async fn leave_secure_group_async(
        mut self,
        input: GroupE2eeLeaveInput,
    ) -> crate::ImResult<GroupE2eeLifecycleResult> {
        if !input.owner_leave_commit {
            return leave_secure_group_request_async(
                self.client,
                &self.session_provider,
                &mut self.transport,
                input,
            )
            .await;
        }

        Err(crate::ImError::unsupported(
            "group-e2ee-owner-leave-commit-async",
        ))
    }

    pub(crate) async fn create_secure_group_async(
        mut self,
        input: GroupE2eeCreateInput,
    ) -> crate::ImResult<GroupE2eeLifecycleResult> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::GroupMessaging)
            .await?;
        let credentials = match input.credentials {
            Some(credentials) => credentials,
            None => load_credentials_async(self.client).await?,
        };
        let service_did = input
            .service_did
            .map(Ok)
            .unwrap_or_else(|| group_e2ee_service_did(self.client))?;
        let group_did = require_group(input.group.as_str())?.to_owned();
        let operation_id = format!(
            "op-{}",
            crate::internal::wire::common::generate_operation_id()
        );
        let create_input = CreateGroupInput {
            creator_did: self.client.did().as_str().to_owned(),
            device_id: device_id_for_client(self.client),
            group_did: group_did.clone(),
            operation_id: operation_id.clone(),
            request_id: format!("group-e2ee-create-{operation_id}"),
            pending_commit_id: Some(format!("pc-{operation_id}")),
        };
        let mls_provider = self.mls_provider.clone();
        let prepared = run_lifecycle_mls_blocking("create prepare", move || {
            mls_provider.create_group_prepare(create_input)
        })
        .await?;
        let group_state_ref = match input.group_state_ref {
            Some(reference) => Some(reference),
            None => local_group_state_ref_async(self.client, &group_did).await,
        };
        let params = super::wire::build_group_e2ee_create_rpc_params(
            &credentials,
            self.client.did().as_str(),
            service_did.as_str(),
            &group_did,
            &prepared,
            group_state_ref.as_ref(),
        )?;
        let delivery = match self
            .transport
            .authenticated_rpc(
                crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT,
                "group.e2ee.create",
                params,
            )
            .await
        {
            Ok(delivery) => delivery,
            Err(err) => {
                return service_rejected_prepared_async(
                    self.mls_provider.clone(),
                    prepared,
                    group_did,
                    err,
                )
                .await
            }
        };
        let mut warnings = Vec::new();
        match finalize_prepared_async(
            self.mls_provider.clone(),
            prepared.clone(),
            group_did.clone(),
            delivery.clone(),
        )
        .await
        {
            Ok(finalized) => {
                if let Err(err) = persist_group_e2ee_summary_async(
                    self.client,
                    GroupE2eeSummaryUpdate {
                        group_did: &group_did,
                        epoch: summary_epoch(&finalized.epoch, Some(prepared.epoch.as_str())),
                        group_state_version: group_state_version(
                            &delivery,
                            group_state_ref.as_ref(),
                        )
                        .as_deref(),
                        crypto_group_id_b64u: Some(finalized.crypto_group_id_b64u.as_str()),
                        epoch_authenticator: finalized.epoch_authenticator.as_deref(),
                        suite: Some(prepared.suite.as_str()),
                        operation_id: Some(finalized.operation_id.as_str()),
                        membership_status: "active",
                    },
                )
                .await
                {
                    warnings.push(format!(
                        "group E2EE create finalized locally but failed to persist local summary: {err}"
                    ));
                }
            }
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

    pub(crate) async fn add_secure_member_async(
        mut self,
        input: GroupE2eeMemberMutationInput,
    ) -> crate::ImResult<GroupE2eeLifecycleResult> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::GroupMessaging)
            .await?;
        let credentials = match input.credentials {
            Some(credentials) => credentials,
            None => load_credentials_async(self.client).await?,
        };
        let service_did = input
            .service_did
            .map(Ok)
            .unwrap_or_else(|| group_e2ee_service_did(self.client))?;
        let group_did = require_group(input.group.as_str())?.to_owned();
        let member_did = require_did(input.member.as_str(), "member")?.to_owned();
        let key_package = self
            .lookup_member_key_package_async(
                &credentials,
                service_did.as_str(),
                &group_did,
                &member_did,
                "normal",
                None,
            )
            .await?;
        let operation_id = format!(
            "op-{}",
            crate::internal::wire::common::generate_operation_id()
        );
        let add_input = AddMemberInput {
            actor_did: self.client.did().as_str().to_owned(),
            device_id: device_id_for_client(self.client),
            group_did: group_did.clone(),
            member_did: member_did.clone(),
            group_key_package: key_package.clone(),
            operation_id: operation_id.clone(),
            request_id: format!("group-e2ee-add-{operation_id}"),
            pending_commit_id: Some(format!("pc-{operation_id}")),
        };
        let mls_provider = self.mls_provider.clone();
        let prepared = run_lifecycle_mls_blocking("add prepare", move || {
            mls_provider.add_member_prepare(add_input)
        })
        .await?;
        let group_state_ref = match input.group_state_ref {
            Some(reference) => Some(reference),
            None => local_group_state_ref_async(self.client, &group_did).await,
        };
        let params = super::wire::build_group_e2ee_add_rpc_params(
            &credentials,
            self.client.did().as_str(),
            &group_did,
            &member_did,
            &prepared,
            &key_package,
            group_state_ref.as_ref(),
        )?;
        let delivery = match self
            .transport
            .authenticated_rpc(
                crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT,
                "group.e2ee.add",
                params,
            )
            .await
        {
            Ok(delivery) => delivery,
            Err(err) => {
                return service_rejected_prepared_async(
                    self.mls_provider.clone(),
                    prepared,
                    group_did,
                    err,
                )
                .await
            }
        };
        let mut warnings = Vec::new();
        match finalize_prepared_async(
            self.mls_provider.clone(),
            prepared.clone(),
            group_did.clone(),
            delivery.clone(),
        )
        .await
        {
            Ok(finalized) => {
                if let Err(err) = persist_group_e2ee_summary_async(
                    self.client,
                    GroupE2eeSummaryUpdate {
                        group_did: &group_did,
                        epoch: summary_epoch(&finalized.epoch, Some(prepared.epoch.as_str())),
                        group_state_version: group_state_version(
                            &delivery,
                            group_state_ref.as_ref(),
                        )
                        .as_deref(),
                        crypto_group_id_b64u: Some(finalized.crypto_group_id_b64u.as_str()),
                        epoch_authenticator: finalized.epoch_authenticator.as_deref(),
                        suite: Some(prepared.suite.as_str()),
                        operation_id: Some(finalized.operation_id.as_str()),
                        membership_status: "active",
                    },
                )
                .await
                {
                    warnings.push(format!(
                        "group E2EE add finalized locally but failed to persist local summary: {err}"
                    ));
                }
            }
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

    pub(crate) async fn remove_secure_member_async(
        mut self,
        input: GroupE2eeMemberMutationInput,
    ) -> crate::ImResult<GroupE2eeLifecycleResult> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::GroupMessaging)
            .await?;
        let credentials = match input.credentials {
            Some(credentials) => credentials,
            None => load_credentials_async(self.client).await?,
        };
        let group_did = require_group(input.group.as_str())?.to_owned();
        let member_did = require_did(input.member.as_str(), "member")?.to_owned();
        let group_state_ref = match input.group_state_ref.clone() {
            Some(reference) => Some(reference),
            None => local_group_state_ref_async(self.client, &group_did).await,
        };
        let operation_id = format!(
            "op-{}",
            crate::internal::wire::common::generate_operation_id()
        );
        let remove_input = RemoveMemberInput {
            actor_did: self.client.did().as_str().to_owned(),
            device_id: device_id_for_client(self.client),
            group_did: group_did.clone(),
            member_did: member_did.clone(),
            group_state_ref: group_state_ref.clone(),
            operation_id: operation_id.clone(),
            request_id: format!("group-e2ee-remove-{operation_id}"),
            pending_commit_id: Some(format!("pc-{operation_id}")),
        };
        let mls_provider = self.mls_provider.clone();
        let prepared = run_lifecycle_mls_blocking("remove prepare", move || {
            mls_provider.remove_member_prepare(remove_input)
        })
        .await?;
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
        let delivery = match self
            .transport
            .authenticated_rpc(
                crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT,
                "group.e2ee.remove",
                params,
            )
            .await
        {
            Ok(delivery) => delivery,
            Err(err) => {
                return service_rejected_prepared_async(
                    self.mls_provider.clone(),
                    prepared,
                    group_did,
                    err,
                )
                .await
            }
        };
        let mut warnings = Vec::new();
        match finalize_prepared_async(
            self.mls_provider.clone(),
            prepared.clone(),
            group_did.clone(),
            delivery.clone(),
        )
        .await
        {
            Ok(finalized) => {
                if let Err(err) = persist_group_e2ee_summary_async(
                    self.client,
                    GroupE2eeSummaryUpdate {
                        group_did: &group_did,
                        epoch: summary_epoch(&finalized.epoch, Some(prepared.epoch.as_str())),
                        group_state_version: group_state_version(
                            &delivery,
                            group_state_ref.as_ref(),
                        )
                        .as_deref(),
                        crypto_group_id_b64u: Some(finalized.crypto_group_id_b64u.as_str()),
                        epoch_authenticator: finalized.epoch_authenticator.as_deref(),
                        suite: Some(prepared.suite.as_str()),
                        operation_id: Some(finalized.operation_id.as_str()),
                        membership_status: "active",
                    },
                )
                .await
                {
                    warnings.push(format!(
                        "group E2EE remove finalized locally but failed to persist local summary: {err}"
                    ));
                }
            }
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

    pub(crate) async fn update_member_key_async(
        mut self,
        input: GroupE2eeKeyReplacementInput,
    ) -> crate::ImResult<GroupE2eeLifecycleResult> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::GroupMessaging)
            .await?;
        let credentials = match input.credentials {
            Some(credentials) => credentials,
            None => load_credentials_async(self.client).await?,
        };
        let service_did = input
            .service_did
            .map(Ok)
            .unwrap_or_else(|| group_e2ee_service_did(self.client))?;
        let group_did = require_group(input.group.as_str())?.to_owned();
        let member_did = require_did(input.member.as_str(), "member")?.to_owned();
        let device_id = normalize_device_id(&input.device_id);
        let key_package = self
            .lookup_member_key_package_async(
                &credentials,
                service_did.as_str(),
                &group_did,
                &member_did,
                "update",
                Some(&device_id),
            )
            .await?;
        let group_state_ref = self
            .resolved_group_state_ref_async(&credentials, &input.group, &group_did)
            .await?;
        let operation_id = format!(
            "op-{}",
            crate::internal::wire::common::generate_operation_id()
        );
        let update_input = UpdateMemberInput {
            actor_did: self.client.did().as_str().to_owned(),
            device_id: device_id_for_client(self.client),
            group_did: group_did.clone(),
            member_did: member_did.clone(),
            target_device_id: device_id.clone(),
            group_key_package: key_package.clone(),
            group_state_ref: group_state_ref.clone(),
            update_key_package_id: Some(key_package.key_package_id.clone()),
            operation_id: operation_id.clone(),
            request_id: format!("group-e2ee-update-{operation_id}"),
            pending_commit_id: Some(format!("pc-{operation_id}")),
        };
        let mls_provider = self.mls_provider.clone();
        let prepared = run_lifecycle_mls_blocking("update prepare", move || {
            mls_provider.update_member_prepare(update_input)
        })
        .await?;
        let params = super::wire::build_group_e2ee_update_rpc_params(
            &credentials,
            self.client.did().as_str(),
            &group_did,
            &member_did,
            &device_id,
            &prepared,
            &key_package,
            group_state_ref.as_ref(),
        )?;
        let delivery = match self
            .transport
            .authenticated_rpc(
                crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT,
                "group.e2ee.update",
                params,
            )
            .await
        {
            Ok(delivery) => delivery,
            Err(err) => {
                return service_rejected_prepared_async(
                    self.mls_provider.clone(),
                    prepared,
                    group_did,
                    err,
                )
                .await
            }
        };
        let mut warnings = Vec::new();
        match finalize_prepared_async(
            self.mls_provider.clone(),
            prepared.clone(),
            group_did.clone(),
            delivery.clone(),
        )
        .await
        {
            Ok(finalized) => {
                if let Err(err) = persist_group_e2ee_summary_async(
                    self.client,
                    GroupE2eeSummaryUpdate {
                        group_did: &group_did,
                        epoch: summary_epoch(&finalized.epoch, Some(prepared.epoch.as_str())),
                        group_state_version: group_state_version(
                            &delivery,
                            group_state_ref.as_ref(),
                        )
                        .as_deref(),
                        crypto_group_id_b64u: Some(finalized.crypto_group_id_b64u.as_str()),
                        epoch_authenticator: finalized.epoch_authenticator.as_deref(),
                        suite: Some(prepared.suite.as_str()),
                        operation_id: Some(finalized.operation_id.as_str()),
                        membership_status: "active",
                    },
                )
                .await
                {
                    warnings.push(format!(
                        "group E2EE update finalized locally but failed to persist local summary: {err}"
                    ));
                }
            }
            Err(err) => warnings.push(format!(
                "group E2EE update was accepted by service but local finalize failed: {err}"
            )),
        }
        Ok(GroupE2eeLifecycleResult {
            delivery: public_lifecycle_delivery(
                "secure_group_update_key",
                &group_did,
                Some(&member_did),
                Some("active"),
                &delivery,
            ),
            warnings,
        })
    }

    pub(crate) async fn recover_member_async(
        mut self,
        input: GroupE2eeKeyReplacementInput,
    ) -> crate::ImResult<GroupE2eeLifecycleResult> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::GroupMessaging)
            .await?;
        let credentials = match input.credentials {
            Some(credentials) => credentials,
            None => load_credentials_async(self.client).await?,
        };
        let service_did = input
            .service_did
            .map(Ok)
            .unwrap_or_else(|| group_e2ee_service_did(self.client))?;
        let group_did = require_group(input.group.as_str())?.to_owned();
        let member_did = require_did(input.member.as_str(), "member")?.to_owned();
        let device_id = normalize_device_id(&input.device_id);
        let key_package = self
            .lookup_member_key_package_async(
                &credentials,
                service_did.as_str(),
                &group_did,
                &member_did,
                "recovery",
                Some(&device_id),
            )
            .await?;
        let group_state_ref = self
            .resolved_group_state_ref_async(&credentials, &input.group, &group_did)
            .await?;
        let operation_id = format!(
            "op-{}",
            crate::internal::wire::common::generate_operation_id()
        );
        let recover_input = RecoverMemberInput {
            actor_did: self.client.did().as_str().to_owned(),
            device_id: device_id_for_client(self.client),
            group_did: group_did.clone(),
            member_did: member_did.clone(),
            target_device_id: device_id.clone(),
            group_key_package: key_package.clone(),
            group_state_ref: group_state_ref.clone(),
            operation_id: operation_id.clone(),
            request_id: format!("group-e2ee-recover-{operation_id}"),
            pending_commit_id: Some(format!("pc-{operation_id}")),
        };
        let mls_provider = self.mls_provider.clone();
        let prepared = run_lifecycle_mls_blocking("recover prepare", move || {
            mls_provider.recover_member_prepare(recover_input)
        })
        .await?;
        let params = super::wire::build_group_e2ee_recover_member_rpc_params(
            &credentials,
            self.client.did().as_str(),
            &group_did,
            &member_did,
            &device_id,
            &prepared,
            &key_package,
            group_state_ref.as_ref(),
        )?;
        let delivery = match self
            .transport
            .authenticated_rpc(
                crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT,
                "group.e2ee.recover_member",
                params,
            )
            .await
        {
            Ok(delivery) => delivery,
            Err(err) => {
                return service_rejected_prepared_async(
                    self.mls_provider.clone(),
                    prepared,
                    group_did,
                    err,
                )
                .await
            }
        };
        let mut warnings = Vec::new();
        match finalize_prepared_async(
            self.mls_provider.clone(),
            prepared.clone(),
            group_did.clone(),
            delivery.clone(),
        )
        .await
        {
            Ok(finalized) => {
                if let Err(err) = persist_group_e2ee_summary_async(
                    self.client,
                    GroupE2eeSummaryUpdate {
                        group_did: &group_did,
                        epoch: summary_epoch(&finalized.epoch, Some(prepared.epoch.as_str())),
                        group_state_version: group_state_version(
                            &delivery,
                            group_state_ref.as_ref(),
                        )
                        .as_deref(),
                        crypto_group_id_b64u: Some(finalized.crypto_group_id_b64u.as_str()),
                        epoch_authenticator: finalized.epoch_authenticator.as_deref(),
                        suite: Some(prepared.suite.as_str()),
                        operation_id: Some(finalized.operation_id.as_str()),
                        membership_status: "active",
                    },
                )
                .await
                {
                    warnings.push(format!(
                        "group E2EE recover-member finalized locally but failed to persist local summary: {err}"
                    ));
                }
            }
            Err(err) => warnings.push(format!(
                "group E2EE recover-member was accepted by service but local finalize failed: {err}"
            )),
        }
        Ok(GroupE2eeLifecycleResult {
            delivery: public_lifecycle_delivery(
                "secure_group_recover_member",
                &group_did,
                Some(&member_did),
                Some("active"),
                &delivery,
            ),
            warnings,
        })
    }

    pub(crate) async fn process_leave_request_async(
        mut self,
        input: GroupE2eeMemberMutationInput,
    ) -> crate::ImResult<GroupE2eeLifecycleResult> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::GroupMessaging)
            .await?;
        let GroupE2eeMemberMutationInput {
            group,
            member,
            reason_text,
            leave_request_id,
            credentials,
            service_did,
            group_state_ref,
        } = input;
        let credentials = match credentials {
            Some(credentials) => credentials,
            None => load_credentials_async(self.client).await?,
        };
        let group_did = require_group(group.as_str())?.to_owned();
        let member_did = require_did(member.as_str(), "member")?.to_owned();
        let leave_request_id = leave_request_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                crate::ImError::invalid_input(
                    Some("leave_request_id".to_owned()),
                    "group E2EE process leave request requires leave_request_id",
                )
            })?
            .to_owned();
        let params = super::wire::build_group_e2ee_process_leave_request_rpc_params(
            &credentials,
            self.client.did().as_str(),
            &group_did,
            &leave_request_id,
        )?;
        let processing = self
            .transport
            .authenticated_rpc(
                crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT,
                "group.e2ee.process_leave_request",
                params,
            )
            .await?;
        let final_result = self
            .remove_secure_member_async(GroupE2eeMemberMutationInput {
                group,
                member: crate::ids::Did::parse(&member_did)?,
                reason_text,
                leave_request_id: Some(leave_request_id),
                credentials: Some(credentials),
                service_did,
                group_state_ref,
            })
            .await?;
        Ok(GroupE2eeLifecycleResult {
            delivery: merge_process_leave_delivery(final_result.delivery, &processing),
            warnings: final_result.warnings,
        })
    }

    async fn resolved_group_state_ref_async(
        &mut self,
        credentials: &GroupTextCredentials,
        group: &crate::ids::GroupRef,
        group_did: &str,
    ) -> crate::ImResult<Option<GroupStateRef>>
    where
        M: Clone + Send + 'static,
    {
        if let Some(reference) = local_group_state_ref_async(self.client, group_did).await {
            return Ok(Some(reference));
        }
        match super::state_ref::resolve_group_state_ref_local_first_async(
            self.client,
            &self.session_provider,
            &mut self.transport,
            &self.mls_provider,
            super::state_ref::ResolveGroupStateRef {
                group: group.clone(),
                credentials: Some(credentials.clone()),
            },
        )
        .await
        {
            Ok(result) => Ok(Some(result.group_state_ref)),
            Err(crate::ImError::LocalStateUnavailable { .. }) => Ok(None),
            Err(err) => Err(err),
        }
    }

    async fn lookup_member_key_package_async(
        &mut self,
        credentials: &GroupTextCredentials,
        service_did: &str,
        group_did: &str,
        member_did: &str,
        purpose: &str,
        device_id: Option<&str>,
    ) -> crate::ImResult<GroupKeyPackage> {
        let params = super::wire::build_group_e2ee_get_key_package_rpc_params(
            credentials,
            self.client.did().as_str(),
            service_did,
            group_did,
            member_did,
            Some(purpose),
            device_id,
        )?;
        let raw = self
            .transport
            .authenticated_rpc(
                crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT,
                "group.e2ee.get_key_package",
                params,
            )
            .await?;
        group_key_package_from_value(&raw)
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

fn merge_process_leave_delivery(mut delivery: Value, processing: &Value) -> Value {
    let Value::Object(ref mut output) = delivery else {
        return delivery;
    };
    output.insert(
        "action".to_owned(),
        Value::String("secure_group_process_leave_request".to_owned()),
    );
    let mut process = Map::new();
    for key in [
        "accepted",
        "final_acceptance",
        "group_did",
        "leave_request_id",
        "pending_leave_request_count",
        "next_step",
    ] {
        if let Some(value) = processing.get(key) {
            process.insert(key.to_owned(), value.clone());
        }
    }
    if let Some(leave_request) = processing.get("leave_request") {
        process.insert("leave_request".to_owned(), leave_request.clone());
    }
    if !process.is_empty() {
        output.insert("process_leave_request".to_owned(), Value::Object(process));
    }
    delivery
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

fn insert_string(map: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        map.insert(key.to_owned(), Value::String(value.to_owned()));
    }
}

fn normalize_device_id(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        DEFAULT_GROUP_MLS_DEVICE_ID.to_owned()
    } else {
        value.to_owned()
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
mod tests;
