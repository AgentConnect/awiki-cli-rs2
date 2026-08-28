//! Internal product orchestration for the ANP P6 v2 device-scoped runtime.
//!
//! This module deliberately stays below the public Core/Dart surface. It owns
//! the local prepare/submit/finalize boundary and a typed Host seam, while the
//! SDK remains the only owner of OpenMLS private state.

use anp::group_e2ee::operations::v2::{
    V2AcceptKeyPackagePublishInput, V2AddMemberInput, V2DecryptInput, V2DecryptOutput,
    V2DidTransitionController, V2EncryptInput, V2FinalizeInput, V2FinalizeOutput,
    V2GenerateKeyPackageInput, V2InspectLocalGroupInput, V2InspectLocalGroupOutput,
    V2KeyPackagePublishStatus, V2ListLocalGroupMemberEndpointsOutput,
    V2PrepareKeyPackagePublishInput, V2PreparedAdd, V2PreparedCreate, V2PreparedKeyPackagePublish,
    V2PreparedRemove, V2ProcessCommitOutput, V2ProcessNoticeInput, V2ProcessNoticeOutput,
    V2ProcessWelcomeInput, V2ReconcilePendingInput, V2ReconciledPendingCommit,
    V2RecoverTransitionWelcomeInput, V2RemoveMemberInput,
};
use anp::group_e2ee::{
    get_key_package_request_v2, group_add_request_v2, group_create_request_v2,
    group_incoming_notification_v2, group_remove_request_v2, group_send_request_v2,
    parse_get_key_package_result_v2, parse_group_create_result_v2,
    parse_group_membership_result_v2, parse_group_send_result_v2,
    parse_publish_key_package_result_v2, publish_key_package_request_v2, V2DeliveredOriginAuth,
    V2GetKeyPackageBody, V2GetKeyPackageResult, V2GroupAddBody, V2GroupCipherObject,
    V2GroupCreateBody, V2GroupCreateResult, V2GroupIncomingBody, V2GroupIncomingMetadata,
    V2GroupMembershipResult, V2GroupRemoveBody, V2GroupSendMetadata, V2GroupSendResult,
    V2GroupStateRef, V2OriginAuth, V2PublishKeyPackageBody, V2PublishKeyPackageResult,
    V2ServiceMetadata, V2Target, GROUP_E2EE_SECURITY_PROFILE_V2, METHOD_GROUP_ADD_V2,
    METHOD_GROUP_CREATE_V2, METHOD_GROUP_REMOVE_V2, METHOD_GROUP_SEND_V2,
};
use anp::proof::{ImProofError, Rfc9421OriginProofError, Rfc9421OriginProofVerificationOptions};
use serde::Serialize;
use serde_json::Value;

use crate::internal::proof::origin::OriginProofIdentity;
use crate::internal::transport::{AsyncAuthenticatedRpcTransport, AuthenticatedRpcTransport};
use crate::internal::wire::direct::DirectPayload;

use super::v2_application::{V2ApplicationProjection, V2ProductApplication};
use super::v2_runtime::GroupE2eeV2Runtime;

const MESSAGE_RPC_ENDPOINT: &str = "/im/rpc";

/// Typed seam between local P6 v2 state and the Group Host.
///
/// There is intentionally no default or degraded implementation. Production
/// callers must provide an authenticated transport adapter; test doubles live
/// only in the test module.
pub(crate) trait GroupE2eeV2Host {
    fn publish_key_package(
        &mut self,
        meta: V2ServiceMetadata,
        body: V2PublishKeyPackageBody,
    ) -> crate::ImResult<V2PublishKeyPackageResult>;

    fn get_key_package(
        &mut self,
        meta: V2ServiceMetadata,
        body: V2GetKeyPackageBody,
    ) -> crate::ImResult<V2GetKeyPackageResult>;

    fn create_group(
        &mut self,
        meta: V2ServiceMetadata,
        body: V2GroupCreateBody,
    ) -> crate::ImResult<V2GroupCreateResult>;

    fn add_member(
        &mut self,
        meta: anp::group_e2ee::V2GroupControlMetadata,
        body: V2GroupAddBody,
    ) -> crate::ImResult<V2GroupMembershipResult>;

    fn remove_member(
        &mut self,
        meta: anp::group_e2ee::V2GroupControlMetadata,
        body: V2GroupRemoveBody,
    ) -> crate::ImResult<V2GroupMembershipResult>;

    fn send_application(
        &mut self,
        meta: V2GroupSendMetadata,
        body: V2GroupCipherObject,
        client_context: Option<Value>,
    ) -> crate::ImResult<V2GroupSendResult>;
}

/// Production adapter for the existing authenticated JSON-RPC transport.
pub(crate) struct RpcGroupE2eeV2Host<T> {
    transport: T,
    proof_identity: OriginProofIdentity,
    p6_client_instance_id: Option<String>,
}

impl<T> RpcGroupE2eeV2Host<T> {
    pub(crate) fn new(transport: T, proof_identity: OriginProofIdentity) -> Self {
        Self {
            transport,
            proof_identity,
            p6_client_instance_id: None,
        }
    }

    pub(crate) fn with_p6_client_instance_id(
        mut self,
        client_instance_id: impl Into<String>,
    ) -> Self {
        self.p6_client_instance_id = Some(client_instance_id.into());
        self
    }
}

impl<T> RpcGroupE2eeV2Host<T>
where
    T: AuthenticatedRpcTransport,
{
    fn origin_auth<M: Serialize, B: Serialize>(
        &self,
        method: &str,
        meta: &M,
        body: &B,
    ) -> crate::ImResult<V2OriginAuth> {
        let payload = DirectPayload {
            method: method.to_owned(),
            meta: to_value(meta, "P6 v2 request metadata")?,
            body: to_value(body, "P6 v2 request body")?,
        };
        let proof =
            crate::internal::proof::origin::build_origin_proof(&self.proof_identity, &payload)?;
        serde_json::from_value(crate::internal::proof::origin::origin_auth_value(&proof)).map_err(
            |err| crate::ImError::Serialization {
                detail: format!("encode P6 v2 origin auth: {err}"),
            },
        )
    }

    fn execute<R>(
        &mut self,
        request: Value,
        parse_result: fn(&Value) -> Result<R, anp::group_e2ee::GroupE2eeV2Error>,
    ) -> crate::ImResult<R> {
        let method = request
            .get("method")
            .and_then(Value::as_str)
            .ok_or_else(|| serialization_error("P6 v2 SDK request is missing method"))?
            .to_owned();
        let mut params = request
            .get("params")
            .cloned()
            .filter(Value::is_object)
            .ok_or_else(|| serialization_error("P6 v2 SDK request is missing params"))?;
        if let Some(client_instance_id) = self.p6_client_instance_id.as_deref() {
            let params = params.as_object_mut().expect("validated P6 params object");
            let client = params
                .entry("client".to_owned())
                .or_insert_with(|| Value::Object(Default::default()))
                .as_object_mut()
                .ok_or_else(|| serialization_error("P6 client context must be an object"))?;
            client.insert(
                "p6_delivery".to_owned(),
                serde_json::json!({
                    "profile": "p6.delivery_context.v1",
                    "client_instance_id": client_instance_id,
                }),
            );
        }
        let raw = self
            .transport
            .authenticated_rpc(MESSAGE_RPC_ENDPOINT, &method, params)?;
        parse_result(&raw).map_err(map_v2_wire_error)
    }
}

impl<T> GroupE2eeV2Host for RpcGroupE2eeV2Host<T>
where
    T: AuthenticatedRpcTransport,
{
    fn publish_key_package(
        &mut self,
        meta: V2ServiceMetadata,
        body: V2PublishKeyPackageBody,
    ) -> crate::ImResult<V2PublishKeyPackageResult> {
        let request = publish_key_package_request_v2(meta, body).map_err(map_v2_wire_error)?;
        self.execute(request, parse_publish_key_package_result_v2)
    }

    fn get_key_package(
        &mut self,
        meta: V2ServiceMetadata,
        body: V2GetKeyPackageBody,
    ) -> crate::ImResult<V2GetKeyPackageResult> {
        let request = get_key_package_request_v2(meta, body).map_err(map_v2_wire_error)?;
        self.execute(request, parse_get_key_package_result_v2)
    }

    fn create_group(
        &mut self,
        meta: V2ServiceMetadata,
        body: V2GroupCreateBody,
    ) -> crate::ImResult<V2GroupCreateResult> {
        let auth = self.origin_auth(METHOD_GROUP_CREATE_V2, &meta, &body)?;
        let request = group_create_request_v2(meta, body, auth).map_err(map_v2_wire_error)?;
        self.execute(request, parse_group_create_result_v2)
    }

    fn add_member(
        &mut self,
        meta: anp::group_e2ee::V2GroupControlMetadata,
        body: V2GroupAddBody,
    ) -> crate::ImResult<V2GroupMembershipResult> {
        let auth = self.origin_auth(METHOD_GROUP_ADD_V2, &meta, &body)?;
        let request = group_add_request_v2(meta, body, auth).map_err(map_v2_wire_error)?;
        self.execute(request, parse_group_membership_result_v2)
    }

    fn remove_member(
        &mut self,
        meta: anp::group_e2ee::V2GroupControlMetadata,
        body: V2GroupRemoveBody,
    ) -> crate::ImResult<V2GroupMembershipResult> {
        let auth = self.origin_auth(METHOD_GROUP_REMOVE_V2, &meta, &body)?;
        let request = group_remove_request_v2(meta, body, auth).map_err(map_v2_wire_error)?;
        self.execute(request, parse_group_membership_result_v2)
    }

    fn send_application(
        &mut self,
        meta: V2GroupSendMetadata,
        body: V2GroupCipherObject,
        client_context: Option<Value>,
    ) -> crate::ImResult<V2GroupSendResult> {
        let auth = self.origin_auth(METHOD_GROUP_SEND_V2, &meta, &body)?;
        let mut request = group_send_request_v2(meta, body, auth).map_err(map_v2_wire_error)?;
        if let Some(client_context) = client_context {
            request
                .get_mut("params")
                .and_then(Value::as_object_mut)
                .ok_or_else(|| serialization_error("P6 v2 SDK request params must be an object"))?
                .insert("client".to_owned(), client_context);
        }
        self.execute(request, parse_group_send_result_v2)
    }
}

impl<T> RpcGroupE2eeV2Host<T>
where
    T: AsyncAuthenticatedRpcTransport,
{
    async fn origin_auth_async<M: Serialize, B: Serialize>(
        &self,
        method: &str,
        meta: &M,
        body: &B,
    ) -> crate::ImResult<V2OriginAuth> {
        let payload = DirectPayload {
            method: method.to_owned(),
            meta: to_value(meta, "P6 v2 request metadata")?,
            body: to_value(body, "P6 v2 request body")?,
        };
        let proof = crate::internal::proof::origin::build_origin_proof_async(
            &self.proof_identity,
            &payload,
        )
        .await?;
        serde_json::from_value(crate::internal::proof::origin::origin_auth_value(&proof)).map_err(
            |err| crate::ImError::Serialization {
                detail: format!("encode P6 v2 origin auth: {err}"),
            },
        )
    }

    async fn execute_async<R>(
        &mut self,
        request: Value,
        parse_result: fn(&Value) -> Result<R, anp::group_e2ee::GroupE2eeV2Error>,
    ) -> crate::ImResult<R> {
        let method = request
            .get("method")
            .and_then(Value::as_str)
            .ok_or_else(|| serialization_error("P6 v2 SDK request is missing method"))?
            .to_owned();
        let mut params = request
            .get("params")
            .cloned()
            .filter(Value::is_object)
            .ok_or_else(|| serialization_error("P6 v2 SDK request is missing params"))?;
        if let Some(client_instance_id) = self.p6_client_instance_id.as_deref() {
            let params = params.as_object_mut().expect("validated P6 params object");
            let client = params
                .entry("client".to_owned())
                .or_insert_with(|| Value::Object(Default::default()))
                .as_object_mut()
                .ok_or_else(|| serialization_error("P6 client context must be an object"))?;
            client.insert(
                "p6_delivery".to_owned(),
                serde_json::json!({
                    "profile": "p6.delivery_context.v1",
                    "client_instance_id": client_instance_id,
                }),
            );
        }
        let raw = self
            .transport
            .authenticated_rpc(MESSAGE_RPC_ENDPOINT, &method, params)
            .await?;
        parse_result(&raw).map_err(map_v2_wire_error)
    }

    async fn get_key_package_async(
        &mut self,
        meta: V2ServiceMetadata,
        body: V2GetKeyPackageBody,
    ) -> crate::ImResult<V2GetKeyPackageResult> {
        let request = get_key_package_request_v2(meta, body).map_err(map_v2_wire_error)?;
        self.execute_async(request, parse_get_key_package_result_v2)
            .await
    }

    async fn create_group_async(
        &mut self,
        meta: V2ServiceMetadata,
        body: V2GroupCreateBody,
    ) -> crate::ImResult<V2GroupCreateResult> {
        let auth = self
            .origin_auth_async(METHOD_GROUP_CREATE_V2, &meta, &body)
            .await?;
        let request = group_create_request_v2(meta, body, auth).map_err(map_v2_wire_error)?;
        self.execute_async(request, parse_group_create_result_v2)
            .await
    }

    async fn add_member_async(
        &mut self,
        meta: anp::group_e2ee::V2GroupControlMetadata,
        body: V2GroupAddBody,
    ) -> crate::ImResult<V2GroupMembershipResult> {
        let auth = self
            .origin_auth_async(METHOD_GROUP_ADD_V2, &meta, &body)
            .await?;
        let request = group_add_request_v2(meta, body, auth).map_err(map_v2_wire_error)?;
        self.execute_async(request, parse_group_membership_result_v2)
            .await
    }

    async fn remove_member_async(
        &mut self,
        meta: anp::group_e2ee::V2GroupControlMetadata,
        body: V2GroupRemoveBody,
    ) -> crate::ImResult<V2GroupMembershipResult> {
        let auth = self
            .origin_auth_async(METHOD_GROUP_REMOVE_V2, &meta, &body)
            .await?;
        let request = group_remove_request_v2(meta, body, auth).map_err(map_v2_wire_error)?;
        self.execute_async(request, parse_group_membership_result_v2)
            .await
    }

    async fn send_application_async(
        &mut self,
        meta: V2GroupSendMetadata,
        body: V2GroupCipherObject,
        client_context: Option<Value>,
    ) -> crate::ImResult<V2GroupSendResult> {
        let auth = self
            .origin_auth_async(METHOD_GROUP_SEND_V2, &meta, &body)
            .await?;
        let mut request = group_send_request_v2(meta, body, auth).map_err(map_v2_wire_error)?;
        if let Some(client_context) = client_context {
            request
                .get_mut("params")
                .and_then(Value::as_object_mut)
                .ok_or_else(|| serialization_error("P6 v2 SDK request params must be an object"))?
                .insert("client".to_owned(), client_context);
        }
        self.execute_async(request, parse_group_send_result_v2)
            .await
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct V2PreparedCreateSubmission {
    pub(crate) meta: V2ServiceMetadata,
    pub(crate) prepared: V2PreparedCreate,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct V2PreparedAddSubmission {
    pub(crate) meta: anp::group_e2ee::V2GroupControlMetadata,
    pub(crate) prepared: V2PreparedAdd,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct V2PreparedRemoveSubmission {
    pub(crate) meta: anp::group_e2ee::V2GroupControlMetadata,
    pub(crate) prepared: V2PreparedRemove,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct V2Committed<R> {
    pub(crate) accepted: R,
    pub(crate) finalized: V2FinalizeOutput,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct V2PreparedApplicationSend {
    pub(crate) meta: V2GroupSendMetadata,
    pub(crate) cipher: V2GroupCipherObject,
    /// AWiki-local, non-secret delivery metadata outside the MLS ciphertext.
    pub(crate) client_context: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct V2PreparedProductApplicationSend {
    pub(crate) encrypted: V2PreparedApplicationSend,
    /// Secret-free text/JSON/attachment projection for local history and UI.
    pub(crate) projection: V2ApplicationProjection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum V2ControlDisposition {
    /// The notice was consumed by the MLS state machine and must not be
    /// projected into the ordinary conversation timeline.
    ConsumedControl(V2ProcessNoticeOutput),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct V2ReconcileEntry {
    pub(crate) pending: V2ReconciledPendingCommit,
    pub(crate) host_recheck_required: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct V2ReconcileResult {
    pub(crate) entries: Vec<V2ReconcileEntry>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct V2IncomingApplicationInput {
    pub(crate) recipient_did: String,
    pub(crate) recipient_device_id: String,
    pub(crate) meta: V2GroupIncomingMetadata,
    pub(crate) body: V2GroupIncomingBody,
    pub(crate) auth: V2DeliveredOriginAuth,
    pub(crate) sender_did_document: Value,
    pub(crate) now: String,
    pub(crate) draft_extension_negotiated: bool,
    pub(crate) request_id: String,
}

pub(crate) struct GroupE2eeV2Product<H> {
    runtime: GroupE2eeV2Runtime,
    host: H,
}

impl<H> GroupE2eeV2Product<H>
where
    H: GroupE2eeV2Host,
{
    pub(crate) fn new(runtime: GroupE2eeV2Runtime, host: H) -> Self {
        Self { runtime, host }
    }

    pub(crate) fn prepare_current_key_package(
        &self,
        meta: V2ServiceMetadata,
        input: V2GenerateKeyPackageInput,
        did_document: &Value,
        identity_signer: &dyn crate::internal::key_provider::IdentitySigner,
    ) -> crate::ImResult<V2PreparedKeyPackagePublish> {
        self.ensure_current_device(&input.owner_did, &input.owner_device_id)?;
        self.ensure_current_device(&meta.sender_did, &meta.sender_device_id)?;
        let prepared = self.runtime.prepare_or_resume_key_package_publish(
            V2PrepareKeyPackagePublishInput {
                meta,
                owner_did: input.owner_did,
                owner_device_id: input.owner_device_id,
                verification_method: input.verification_method,
                key_package_id: input.key_package_id,
                issued_at: input.issued_at,
                expires_at: input.expires_at,
                now: input.now,
                draft_extension_negotiated: input.draft_extension_negotiated,
                request_id: input.request_id,
            },
            did_document,
            identity_signer,
        )?;
        publish_key_package_request_v2(prepared.meta.clone(), prepared.body.clone())
            .map_err(map_v2_wire_error)?;
        Ok(prepared)
    }

    pub(crate) async fn prepare_current_key_package_async(
        &self,
        meta: V2ServiceMetadata,
        input: V2GenerateKeyPackageInput,
        did_document: &Value,
        identity_signer: &dyn crate::internal::key_provider::IdentitySigner,
    ) -> crate::ImResult<V2PreparedKeyPackagePublish> {
        self.ensure_current_device(&input.owner_did, &input.owner_device_id)?;
        self.ensure_current_device(&meta.sender_did, &meta.sender_device_id)?;
        let prepared = self
            .runtime
            .prepare_or_resume_key_package_publish_async(
                V2PrepareKeyPackagePublishInput {
                    meta,
                    owner_did: input.owner_did,
                    owner_device_id: input.owner_device_id,
                    verification_method: input.verification_method,
                    key_package_id: input.key_package_id,
                    issued_at: input.issued_at,
                    expires_at: input.expires_at,
                    now: input.now,
                    draft_extension_negotiated: input.draft_extension_negotiated,
                    request_id: input.request_id,
                },
                did_document,
                identity_signer,
            )
            .await?;
        publish_key_package_request_v2(prepared.meta.clone(), prepared.body.clone())
            .map_err(map_v2_wire_error)?;
        Ok(prepared)
    }

    pub(crate) fn publish_current_key_package(
        &mut self,
        prepared: &V2PreparedKeyPackagePublish,
    ) -> crate::ImResult<V2PublishKeyPackageResult> {
        self.ensure_current_device(
            &prepared.body.group_key_package.owner_did,
            &prepared.body.group_key_package.owner_device_id,
        )?;
        if prepared.status == V2KeyPackagePublishStatus::Accepted {
            return prepared.accepted_result.clone().ok_or_else(|| {
                crate::ImError::LocalStateUnavailable {
                    detail: "accepted P6 v2 KeyPackage publish omitted its typed Host result"
                        .to_owned(),
                }
            });
        }
        let result = self
            .host
            .publish_key_package(prepared.meta.clone(), prepared.body.clone())?;
        result.validate().map_err(map_v2_wire_error)?;
        if result.owner_did != prepared.body.group_key_package.owner_did
            || result.owner_device_id != prepared.body.group_key_package.owner_device_id
            || result.key_package_id != prepared.body.group_key_package.key_package_id
        {
            return Err(host_typed_result_mismatch("KeyPackage publish result"));
        }
        let accepted = self
            .runtime
            .accept_key_package_publish(V2AcceptKeyPackagePublishInput {
                owner_did: prepared.body.group_key_package.owner_did.clone(),
                owner_device_id: prepared.body.group_key_package.owner_device_id.clone(),
                operation_id: prepared.meta.operation_id.clone(),
                result,
                request_id: format!("{}-accept", prepared.meta.operation_id),
            })?;
        accepted
            .accepted_result
            .ok_or_else(|| crate::ImError::LocalStateUnavailable {
                detail: "accepted P6 v2 KeyPackage publish was not persisted".to_owned(),
            })
    }

    /// Async production seam for Join-triggered P6 publication. Local
    /// prepare/resume and acceptance remain synchronous SQLite operations, but
    /// the authenticated Host call never blocks the async executor.
    pub(crate) async fn publish_current_key_package_async<T>(
        &self,
        transport: &mut T,
        prepared: &V2PreparedKeyPackagePublish,
    ) -> crate::ImResult<V2PublishKeyPackageResult>
    where
        T: AsyncAuthenticatedRpcTransport,
    {
        self.ensure_current_device(
            &prepared.body.group_key_package.owner_did,
            &prepared.body.group_key_package.owner_device_id,
        )?;
        if prepared.status == V2KeyPackagePublishStatus::Accepted {
            return prepared.accepted_result.clone().ok_or_else(|| {
                crate::ImError::LocalStateUnavailable {
                    detail: "accepted P6 v2 KeyPackage publish omitted its typed Host result"
                        .to_owned(),
                }
            });
        }
        let request = publish_key_package_request_v2(prepared.meta.clone(), prepared.body.clone())
            .map_err(map_v2_wire_error)?;
        let method = request
            .get("method")
            .and_then(Value::as_str)
            .ok_or_else(|| serialization_error("P6 v2 SDK request is missing method"))?
            .to_owned();
        let params = request
            .get("params")
            .cloned()
            .filter(Value::is_object)
            .ok_or_else(|| serialization_error("P6 v2 SDK request is missing params"))?;
        let raw = transport
            .authenticated_rpc(MESSAGE_RPC_ENDPOINT, &method, params)
            .await?;
        let result = parse_publish_key_package_result_v2(&raw).map_err(map_v2_wire_error)?;
        result.validate().map_err(map_v2_wire_error)?;
        if result.owner_did != prepared.body.group_key_package.owner_did
            || result.owner_device_id != prepared.body.group_key_package.owner_device_id
            || result.key_package_id != prepared.body.group_key_package.key_package_id
        {
            return Err(host_typed_result_mismatch("KeyPackage publish result"));
        }
        let accepted = self
            .runtime
            .accept_key_package_publish(V2AcceptKeyPackagePublishInput {
                owner_did: prepared.body.group_key_package.owner_did.clone(),
                owner_device_id: prepared.body.group_key_package.owner_device_id.clone(),
                operation_id: prepared.meta.operation_id.clone(),
                result,
                request_id: format!("{}-accept", prepared.meta.operation_id),
            })?;
        accepted
            .accepted_result
            .ok_or_else(|| crate::ImError::LocalStateUnavailable {
                detail: "accepted P6 v2 KeyPackage publish was not persisted".to_owned(),
            })
    }

    pub(crate) fn get_target_key_package(
        &mut self,
        meta: V2ServiceMetadata,
        body: V2GetKeyPackageBody,
    ) -> crate::ImResult<V2GetKeyPackageResult> {
        self.ensure_current_device(&meta.sender_did, &meta.sender_device_id)?;
        get_key_package_request_v2(meta.clone(), body.clone()).map_err(map_v2_wire_error)?;
        let result = self.host.get_key_package(meta, body.clone())?;
        result.validate().map_err(map_v2_wire_error)?;
        if result.target_did != body.target_did || result.target_device_id != body.target_device_id
        {
            return Err(host_typed_result_mismatch("KeyPackage lookup result"));
        }
        Ok(result)
    }

    pub(crate) fn list_local_group_member_endpoints(
        &self,
        input: V2InspectLocalGroupInput,
    ) -> crate::ImResult<V2ListLocalGroupMemberEndpointsOutput> {
        self.ensure_current_device(&input.owner_did, &input.owner_device_id)?;
        self.runtime.list_local_group_member_endpoints(input)
    }

    pub(crate) fn inspect_local_group(
        &self,
        input: V2InspectLocalGroupInput,
    ) -> crate::ImResult<V2InspectLocalGroupOutput> {
        self.ensure_current_device(&input.owner_did, &input.owner_device_id)?;
        self.runtime.inspect_local_group(input)
    }

    pub(crate) fn prepare_create(
        &self,
        input: anp::group_e2ee::operations::v2::V2CreateGroupInput,
    ) -> crate::ImResult<V2PreparedCreateSubmission> {
        self.ensure_current_device(&input.meta.sender_did, &input.meta.sender_device_id)?;
        let meta = input.meta.clone();
        let prepared = self.runtime.create_group_prepare(input)?;
        Ok(V2PreparedCreateSubmission { meta, prepared })
    }

    pub(crate) fn submit_create(
        &mut self,
        submission: &V2PreparedCreateSubmission,
        request_id: impl Into<String>,
    ) -> crate::ImResult<V2Committed<V2GroupCreateResult>> {
        let result = self
            .host
            .create_group(submission.meta.clone(), submission.prepared.body.clone())?;
        result.validate().map_err(map_v2_wire_error)?;
        let body = &submission.prepared.body;
        if result.group_did != body.group_did
            || result.group_state_ref != body.group_state_ref
            || result.crypto_group_id_b64u != body.crypto_group_id_b64u
            || result.epoch != body.epoch
        {
            return Err(host_typed_result_mismatch("group create result"));
        }
        let finalized = self.runtime.finalize_commit(V2FinalizeInput {
            pending_commit_id: submission.prepared.pending_commit_id.clone(),
            request_id: request_id.into(),
        })?;
        Ok(V2Committed {
            accepted: result,
            finalized,
        })
    }

    pub(crate) fn prepare_add(
        &self,
        input: V2AddMemberInput,
    ) -> crate::ImResult<V2PreparedAddSubmission> {
        self.ensure_current_device(&input.meta.sender_did, &input.meta.sender_device_id)?;
        let meta = input.meta.clone();
        let prepared = self.runtime.add_member_prepare(input)?;
        Ok(V2PreparedAddSubmission { meta, prepared })
    }

    pub(crate) fn prepare_transition_add(
        &self,
        input: V2AddMemberInput,
        controller: V2DidTransitionController,
    ) -> crate::ImResult<V2PreparedAddSubmission> {
        let meta = input.meta.clone();
        let prepared = self
            .runtime
            .transition_add_member_prepare(input, controller)?;
        Ok(V2PreparedAddSubmission { meta, prepared })
    }

    pub(crate) fn process_transition_welcome(
        &self,
        input: V2ProcessWelcomeInput,
    ) -> crate::ImResult<V2ProcessCommitOutput> {
        self.ensure_current_device(&input.recipient_did, &input.recipient_device_id)?;
        self.runtime.process_welcome(input)
    }

    pub(crate) fn recover_finalized_transition_welcome(
        &self,
        input: V2RecoverTransitionWelcomeInput,
    ) -> crate::ImResult<Option<V2GroupAddBody>> {
        self.runtime.recover_finalized_transition_welcome(input)
    }

    pub(crate) fn submit_add(
        &mut self,
        submission: &V2PreparedAddSubmission,
        request_id: impl Into<String>,
    ) -> crate::ImResult<V2Committed<V2GroupMembershipResult>> {
        let result = self
            .host
            .add_member(submission.meta.clone(), submission.prepared.body.clone())?;
        verify_membership_result(&result, &submission.prepared.body)?;
        let finalized = self.runtime.finalize_commit(V2FinalizeInput {
            pending_commit_id: submission.prepared.pending_commit_id.clone(),
            request_id: request_id.into(),
        })?;
        Ok(V2Committed {
            accepted: result,
            finalized,
        })
    }

    pub(crate) fn prepare_remove(
        &self,
        input: V2RemoveMemberInput,
    ) -> crate::ImResult<V2PreparedRemoveSubmission> {
        self.ensure_current_device(&input.meta.sender_did, &input.meta.sender_device_id)?;
        let meta = input.meta.clone();
        let prepared = self.runtime.remove_member_prepare(input)?;
        Ok(V2PreparedRemoveSubmission { meta, prepared })
    }

    pub(crate) fn submit_remove(
        &mut self,
        submission: &V2PreparedRemoveSubmission,
        request_id: impl Into<String>,
    ) -> crate::ImResult<V2Committed<V2GroupMembershipResult>> {
        let result = self
            .host
            .remove_member(submission.meta.clone(), submission.prepared.body.clone())?;
        verify_remove_result(&result, &submission.prepared.body)?;
        let finalized = self.runtime.finalize_commit(V2FinalizeInput {
            pending_commit_id: submission.prepared.pending_commit_id.clone(),
            request_id: request_id.into(),
        })?;
        Ok(V2Committed {
            accepted: result,
            finalized,
        })
    }

    /// Abort is explicit and must only be called after a deterministic Host
    /// rejection. An ambiguous transport error deliberately leaves `prepared`.
    pub(crate) fn abort_pending(
        &self,
        pending_commit_id: impl Into<String>,
        request_id: impl Into<String>,
    ) -> crate::ImResult<V2FinalizeOutput> {
        self.runtime.abort_commit(V2FinalizeInput {
            pending_commit_id: pending_commit_id.into(),
            request_id: request_id.into(),
        })
    }

    pub(crate) fn reconcile_pending(
        &self,
        request_id: impl Into<String>,
    ) -> crate::ImResult<V2ReconcileResult> {
        let output = self.runtime.reconcile_pending(V2ReconcilePendingInput {
            request_id: request_id.into(),
        })?;
        Ok(V2ReconcileResult {
            entries: output
                .pending_commits
                .into_iter()
                .map(|pending| V2ReconcileEntry {
                    host_recheck_required: pending.status == "prepared",
                    pending,
                })
                .collect(),
        })
    }

    pub(crate) fn consume_notice(
        &self,
        input: V2ProcessNoticeInput,
    ) -> crate::ImResult<V2ControlDisposition> {
        self.ensure_current_device(&input.recipient_did, &input.recipient_device_id)?;
        self.runtime
            .process_notice(input)
            .map(V2ControlDisposition::ConsumedControl)
    }

    pub(crate) fn prepare_application_send(
        &self,
        input: V2EncryptInput,
    ) -> crate::ImResult<V2PreparedApplicationSend> {
        self.ensure_current_device(&input.meta.sender_did, &input.meta.sender_device_id)?;
        let meta = input.meta.clone();
        let cipher = self.runtime.encrypt(input)?;
        Ok(V2PreparedApplicationSend {
            meta,
            cipher,
            client_context: None,
        })
    }

    /// Encrypts one logical application body into exactly one MLS ciphertext.
    ///
    /// For attachments, `application` contains the full manifest only until
    /// this call returns. The returned projection has the object key/nonce
    /// removed and is safe for ordinary local message projection.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare_product_application_send(
        &self,
        meta: V2GroupSendMetadata,
        group_state_ref: V2GroupStateRef,
        application: V2ProductApplication,
        sender_did_document: Value,
        now: String,
        draft_extension_negotiated: bool,
        request_id: String,
    ) -> crate::ImResult<V2PreparedProductApplicationSend> {
        let projection = application.projection().clone();
        let client_context = application.client_context().cloned();
        let mut encrypted = self.prepare_application_send(V2EncryptInput {
            meta,
            group_state_ref,
            application_plaintext: application.into_plaintext(),
            sender_did_document,
            now,
            draft_extension_negotiated,
            request_id,
        })?;
        encrypted.client_context = client_context;
        Ok(V2PreparedProductApplicationSend {
            encrypted,
            projection,
        })
    }

    pub(crate) fn submit_product_application_send(
        &mut self,
        prepared: &V2PreparedProductApplicationSend,
    ) -> crate::ImResult<V2GroupSendResult> {
        self.submit_application_send(&prepared.encrypted)
    }

    pub(crate) fn submit_application_send(
        &mut self,
        prepared: &V2PreparedApplicationSend,
    ) -> crate::ImResult<V2GroupSendResult> {
        let result = match self.host.send_application(
            prepared.meta.clone(),
            prepared.cipher.clone(),
            prepared.client_context.clone(),
        ) {
            Ok(result) => result,
            Err(error) => {
                let signal = match &error {
                    crate::ImError::Service {
                        code: Some(code), ..
                    } if code == "group.not_member" => {
                        Some(anp::group_e2ee::operations::v2::V2TerminalSignal::GroupNotMember)
                    }
                    crate::ImError::Service {
                        code: Some(code), ..
                    } if code == "group.e2ee.leaf_not_current" => {
                        Some(anp::group_e2ee::operations::v2::V2TerminalSignal::LeafNotCurrent)
                    }
                    _ => None,
                };
                if let Some(signal) = signal {
                    let scope = self.runtime.owner_scope()?;
                    self.runtime.mark_terminal_intent(
                        anp::group_e2ee::operations::v2::V2MarkTerminalIntentInput {
                            owner_did: scope.owner_did,
                            owner_device_id: scope.device_id,
                            group_did: prepared.cipher.group_state_ref.group_did.clone(),
                            signal,
                            request_id: format!(
                                "p6-v2-send-terminal-{}",
                                crate::internal::wire::common::generate_operation_id()
                            ),
                        },
                    )?;
                }
                return Err(error);
            }
        };
        result.validate().map_err(map_v2_wire_error)?;
        if result.group_did != prepared.cipher.group_state_ref.group_did
            || result.message_id != prepared.meta.message_id
            || result.operation_id != prepared.meta.operation_id
            || result.group_state_version != prepared.cipher.group_state_ref.group_state_version
            || result.epoch != prepared.cipher.epoch
        {
            return Err(host_typed_result_mismatch("group send result"));
        }
        Ok(result)
    }

    pub(crate) fn decrypt_incoming_application(
        &self,
        input: V2IncomingApplicationInput,
    ) -> crate::ImResult<V2DecryptOutput> {
        self.ensure_current_device(&input.recipient_did, &input.recipient_device_id)?;
        group_incoming_notification_v2(input.meta.clone(), input.body.clone(), input.auth.clone())
            .map_err(map_v2_wire_error)?;
        if input.meta.target.did != input.recipient_did
            || input.meta.recipient_device_id != input.recipient_device_id
        {
            return Err(crate::ImError::invalid_input(
                None,
                "P6 v2 group.incoming does not target the current device",
            ));
        }
        let parsed_signature =
            anp::proof::parse_im_signature_input(&input.auth.origin_proof.signature_input)
                .map_err(|_| crate::ImError::PermissionDenied)?;
        if let Some(device) = anp::authentication::find_eligible_device(
            &input.sender_did_document,
            &input.meta.sender_device_id,
            anp::authentication::PROFILE_GROUP_E2EE_V2,
        )
        .map_err(|_| crate::ImError::PermissionDenied)?
        {
            if parsed_signature.keyid != device.signing_key_id {
                return Err(crate::ImError::PermissionDenied);
            }
        }
        match anp::group_e2ee::verify_group_incoming_origin_proof_v2(
            &input.meta,
            &input.body,
            &input.auth,
            Rfc9421OriginProofVerificationOptions {
                did_document: Some(input.sender_did_document.clone()),
                verification_method: None,
                expected_signer_did: Some(input.meta.sender_did.clone()),
            },
        ) {
            Ok(_) => {}
            Err(anp::group_e2ee::GroupE2eeV2Error::OriginProof(
                Rfc9421OriginProofError::ImProof(
                    ImProofError::VerificationMethodNotFound | ImProofError::VerificationFailed,
                ),
            )) => {
                // P2 is not a historical key log. A missing key or a
                // signature-only failure can be a legitimate in-place
                // rotation; MLS authentication below remains mandatory.
            }
            Err(error) => return Err(map_v2_wire_error(error)),
        }
        let context = input.auth.origin_context;
        let contextual_anp_version = context
            .extra_meta
            .get("anp_version")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or(input.meta.anp_version);
        let originating_meta = V2GroupSendMetadata {
            anp_version: contextual_anp_version,
            profile: input.meta.profile,
            security_profile: GROUP_E2EE_SECURITY_PROFILE_V2.to_owned(),
            sender_did: input.meta.sender_did,
            sender_device_id: input.meta.sender_device_id,
            target: V2Target {
                kind: "group".to_owned(),
                did: input.body.group_did,
            },
            operation_id: input.meta.operation_id,
            message_id: input.meta.message_id,
            content_type: input.meta.content_type,
            created_at: context.created_at,
        };
        self.runtime.decrypt(V2DecryptInput {
            recipient_did: input.recipient_did,
            recipient_device_id: input.recipient_device_id,
            originating_meta,
            group_cipher_object: input.body.group_cipher_object,
            sender_did_document: input.sender_did_document,
            now: input.now,
            draft_extension_negotiated: input.draft_extension_negotiated,
            request_id: input.request_id,
        })
    }

    fn ensure_current_device(&self, did: &str, device_id: &str) -> crate::ImResult<()> {
        let scope = self.runtime.owner_scope()?;
        if scope.owner_did != did || scope.device_id != device_id {
            return Err(crate::ImError::invalid_input(
                None,
                format!(
                    "P6 v2 operation targets {did}/{device_id}, but local OwnerScope is {}/{}",
                    scope.owner_did, scope.device_id
                ),
            ));
        }
        Ok(())
    }
}

impl<T> GroupE2eeV2Product<RpcGroupE2eeV2Host<T>>
where
    T: AsyncAuthenticatedRpcTransport + AuthenticatedRpcTransport,
{
    pub(crate) async fn submit_create_async(
        &mut self,
        submission: &V2PreparedCreateSubmission,
        request_id: impl Into<String>,
    ) -> crate::ImResult<V2Committed<V2GroupCreateResult>> {
        let result = self
            .host
            .create_group_async(submission.meta.clone(), submission.prepared.body.clone())
            .await?;
        result.validate().map_err(map_v2_wire_error)?;
        let body = &submission.prepared.body;
        if result.group_did != body.group_did
            || result.group_state_ref != body.group_state_ref
            || result.crypto_group_id_b64u != body.crypto_group_id_b64u
            || result.epoch != body.epoch
        {
            return Err(host_typed_result_mismatch("group create result"));
        }
        let finalized = self.runtime.finalize_commit(V2FinalizeInput {
            pending_commit_id: submission.prepared.pending_commit_id.clone(),
            request_id: request_id.into(),
        })?;
        Ok(V2Committed {
            accepted: result,
            finalized,
        })
    }

    pub(crate) async fn get_target_key_package_async(
        &mut self,
        meta: V2ServiceMetadata,
        body: V2GetKeyPackageBody,
    ) -> crate::ImResult<V2GetKeyPackageResult> {
        self.ensure_current_device(&meta.sender_did, &meta.sender_device_id)?;
        get_key_package_request_v2(meta.clone(), body.clone()).map_err(map_v2_wire_error)?;
        let result = self.host.get_key_package_async(meta, body.clone()).await?;
        result.validate().map_err(map_v2_wire_error)?;
        if result.target_did != body.target_did || result.target_device_id != body.target_device_id
        {
            return Err(host_typed_result_mismatch("KeyPackage lookup result"));
        }
        Ok(result)
    }

    pub(crate) async fn submit_add_async(
        &mut self,
        submission: &V2PreparedAddSubmission,
        request_id: impl Into<String>,
    ) -> crate::ImResult<V2Committed<V2GroupMembershipResult>> {
        let result = self
            .host
            .add_member_async(submission.meta.clone(), submission.prepared.body.clone())
            .await?;
        verify_membership_result(&result, &submission.prepared.body)?;
        let finalized = self.runtime.finalize_commit(V2FinalizeInput {
            pending_commit_id: submission.prepared.pending_commit_id.clone(),
            request_id: request_id.into(),
        })?;
        Ok(V2Committed {
            accepted: result,
            finalized,
        })
    }

    pub(crate) async fn submit_remove_async(
        &mut self,
        submission: &V2PreparedRemoveSubmission,
        request_id: impl Into<String>,
    ) -> crate::ImResult<V2Committed<V2GroupMembershipResult>> {
        let result = self
            .host
            .remove_member_async(submission.meta.clone(), submission.prepared.body.clone())
            .await?;
        verify_remove_result(&result, &submission.prepared.body)?;
        let finalized = self.runtime.finalize_commit(V2FinalizeInput {
            pending_commit_id: submission.prepared.pending_commit_id.clone(),
            request_id: request_id.into(),
        })?;
        Ok(V2Committed {
            accepted: result,
            finalized,
        })
    }

    pub(crate) async fn submit_product_application_send_async(
        &mut self,
        prepared: &V2PreparedProductApplicationSend,
    ) -> crate::ImResult<V2GroupSendResult> {
        self.submit_application_send_async(&prepared.encrypted)
            .await
    }

    pub(crate) async fn submit_application_send_async(
        &mut self,
        prepared: &V2PreparedApplicationSend,
    ) -> crate::ImResult<V2GroupSendResult> {
        let result = match self
            .host
            .send_application_async(
                prepared.meta.clone(),
                prepared.cipher.clone(),
                prepared.client_context.clone(),
            )
            .await
        {
            Ok(result) => result,
            Err(error) => {
                let signal = match &error {
                    crate::ImError::Service {
                        code: Some(code), ..
                    } if code == "group.not_member" => {
                        Some(anp::group_e2ee::operations::v2::V2TerminalSignal::GroupNotMember)
                    }
                    crate::ImError::Service {
                        code: Some(code), ..
                    } if code == "group.e2ee.leaf_not_current" => {
                        Some(anp::group_e2ee::operations::v2::V2TerminalSignal::LeafNotCurrent)
                    }
                    _ => None,
                };
                if let Some(signal) = signal {
                    let scope = self.runtime.owner_scope()?;
                    self.runtime.mark_terminal_intent(
                        anp::group_e2ee::operations::v2::V2MarkTerminalIntentInput {
                            owner_did: scope.owner_did,
                            owner_device_id: scope.device_id,
                            group_did: prepared.cipher.group_state_ref.group_did.clone(),
                            signal,
                            request_id: format!(
                                "p6-v2-send-terminal-{}",
                                crate::internal::wire::common::generate_operation_id()
                            ),
                        },
                    )?;
                }
                return Err(error);
            }
        };
        result.validate().map_err(map_v2_wire_error)?;
        if result.group_did != prepared.cipher.group_state_ref.group_did
            || result.message_id != prepared.meta.message_id
            || result.operation_id != prepared.meta.operation_id
            || result.group_state_version != prepared.cipher.group_state_ref.group_state_version
            || result.epoch != prepared.cipher.epoch
        {
            return Err(host_typed_result_mismatch("group send result"));
        }
        Ok(result)
    }
}

fn verify_membership_result(
    result: &V2GroupMembershipResult,
    body: &V2GroupAddBody,
) -> crate::ImResult<()> {
    result.validate().map_err(map_v2_wire_error)?;
    if result.group_did != body.group_state_ref.group_did
        || result.member_did != body.member_did
        || result.member_device_id != body.member_device_id
        || result.group_state_ref != body.group_state_ref
        || result.crypto_group_id_b64u != body.crypto_group_id_b64u
        || result.epoch != body.epoch
    {
        return Err(host_typed_result_mismatch("group add result"));
    }
    Ok(())
}

fn verify_remove_result(
    result: &V2GroupMembershipResult,
    body: &V2GroupRemoveBody,
) -> crate::ImResult<()> {
    result.validate().map_err(map_v2_wire_error)?;
    if result.group_did != body.group_state_ref.group_did
        || result.member_did != body.member_did
        || result.member_device_id != body.member_device_id
        || result.group_state_ref != body.group_state_ref
        || result.crypto_group_id_b64u != body.crypto_group_id_b64u
        || result.epoch != body.epoch
    {
        return Err(host_typed_result_mismatch("group remove result"));
    }
    Ok(())
}

fn to_value<T: Serialize>(value: &T, context: &str) -> crate::ImResult<Value> {
    serde_json::to_value(value).map_err(|err| crate::ImError::Serialization {
        detail: format!("serialize {context}: {err}"),
    })
}

fn map_v2_wire_error(error: anp::group_e2ee::GroupE2eeV2Error) -> crate::ImError {
    crate::ImError::invalid_input(None, format!("invalid P6 v2 wire object: {error}"))
}

fn serialization_error(message: impl Into<String>) -> crate::ImError {
    crate::ImError::Serialization {
        detail: message.into(),
    }
}

fn host_typed_result_mismatch(context: &str) -> crate::ImError {
    crate::ImError::Internal {
        message: format!("P6 v2 Host typed result field mismatch: {context}"),
    }
}

#[cfg(test)]
#[path = "v2_product_tests.rs"]
mod tests;
