//! Production construction and public lifecycle adapters for P6 v2.
//!
//! The public Group API remains device-agnostic. This module binds each P6
//! operation to the current authenticated protocol device and never accepts a
//! caller-selected sibling or the legacy `default` device.

use anp::authentication::PROFILE_GROUP_E2EE_V2;
use anp::group_e2ee::operations::v2::{V2CreateGroupInput, V2GenerateKeyPackageInput};
use anp::group_e2ee::{
    V2GroupStateRef, V2ServiceMetadata, V2Target, GROUP_E2EE_PROFILE_V2,
    GROUP_E2EE_SECURITY_PROFILE_V2, GROUP_E2EE_TRANSPORT_PROFILE_V2,
};
use anp::PrivateKeyMaterial;
use serde_json::Value;
use time::{format_description::well_known::Rfc3339, Duration, OffsetDateTime};

use crate::internal::proof::origin::OriginProofIdentity;

use super::v2_product::{GroupE2eeV2Product, RpcGroupE2eeV2Host};

const KEY_PACKAGE_TTL: Duration = Duration::days(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CurrentV2Device {
    pub(crate) device_id: String,
    pub(crate) signing_key_id: String,
}

pub(crate) type ProductionV2Product<'a> =
    GroupE2eeV2Product<RpcGroupE2eeV2Host<crate::internal::transport::CoreHttpTransport<'a>>>;

pub(crate) struct ProductionV2Context<'a> {
    pub(crate) product: ProductionV2Product<'a>,
    pub(crate) device: CurrentV2Device,
    pub(crate) did_document: Value,
    device_signing_private_key: PrivateKeyMaterial,
}

/// Builds the sole production P6 v2 product context for the current device.
///
/// The exact protocol device comes from authenticated local identity state,
/// while its P6 authorization comes from the embedded P2 `deviceManifest`.
/// Neither a public request nor a wire response may select another local
/// device.
pub(crate) fn production_context(
    client: &crate::core::ImClient,
) -> crate::ImResult<ProductionV2Context<'_>> {
    let device = current_v2_device(client)?;
    let did_document = client.runtime().key_provider.did_document()?;
    validate_current_device_document(client.did().as_str(), &device, &did_document)?;

    let signing_private_pem = client
        .runtime()
        .key_provider
        .device_request_signing_private_pem()?;
    let device_signing_private_key = PrivateKeyMaterial::from_pem(&signing_private_pem)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let proof_identity = OriginProofIdentity {
        identity_name: client.current_identity().id.as_str().to_owned(),
        did_document: Some(did_document.clone()),
        key1_private_pem: signing_private_pem,
        verification_method: Some(device.signing_key_id.clone()),
    };
    let runtime = super::v2_runtime::runtime_for_client(client)?;
    let host = RpcGroupE2eeV2Host::new(
        crate::internal::transport::CoreHttpTransport::new(client),
        proof_identity,
    );
    Ok(ProductionV2Context {
        product: GroupE2eeV2Product::new(runtime, host),
        device,
        did_document,
        device_signing_private_key,
    })
}

pub(crate) fn publish_current_key_package(
    client: &crate::core::ImClient,
    request: crate::groups::GroupKeyPackagePublishRequest,
) -> crate::ImResult<crate::groups::GroupKeyPackagePublishResult> {
    let mut context = production_context(client)?;
    require_current_device_selection(request.device_id.as_deref(), &context.device.device_id)?;
    let service_did = service_did(client)?;
    let operation_id = operation_id("publish-key-package");
    let now = OffsetDateTime::now_utc();
    let now_text = format_time(now)?;
    let expires_at = format_time(now + KEY_PACKAGE_TTL)?;
    let key_package_id = request
        .key_package_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            format!(
                "kp-{}",
                crate::internal::wire::common::generate_operation_id()
            )
        });
    let prepared = context.product.prepare_current_key_package(
        service_meta(
            client,
            &context.device,
            service_did.as_str(),
            &operation_id,
            GROUP_E2EE_TRANSPORT_PROFILE_V2,
            &now_text,
        ),
        V2GenerateKeyPackageInput {
            owner_did: client.did().as_str().to_owned(),
            owner_device_id: context.device.device_id.clone(),
            verification_method: context.device.signing_key_id.clone(),
            key_package_id: key_package_id.clone(),
            issued_at: now_text.clone(),
            expires_at,
            now: now_text,
            draft_extension_negotiated: true,
            request_id: format!("{operation_id}-local"),
        },
        &context.did_document,
        &context.device_signing_private_key,
    )?;
    let accepted = context.product.publish_current_key_package(&prepared)?;
    let raw_response =
        serde_json::to_value(&accepted).map_err(|error| crate::ImError::Serialization {
            detail: format!("serialize P6 v2 KeyPackage publish result: {error}"),
        })?;
    Ok(crate::groups::GroupKeyPackagePublishResult {
        owner_did: client.did().clone(),
        device_id: context.device.device_id,
        key_package_id,
        purpose: request.purpose,
        group: request.group,
        raw_response,
        warnings: Vec::new(),
    })
}

/// Creates the P6 v2 MLS state after P4 has created the business group.
///
/// The creator KeyPackage is published first, then the exact typed P6 create
/// result is accepted before local state is finalized. Any uncertain Host
/// response leaves the SDK WAL prepared and returns the error to the caller.
pub(crate) fn initialize_created_group(
    client: &crate::core::ImClient,
    group_state_ref: V2GroupStateRef,
) -> crate::ImResult<()> {
    let mut context = production_context(client)?;
    let service_did = service_did(client)?;
    let now = OffsetDateTime::now_utc();
    let now_text = format_time(now)?;
    let expires_at = format_time(now + KEY_PACKAGE_TTL)?;
    let publish_operation = operation_id("create-key-package");
    let key_package_id = format!(
        "kp-{}",
        crate::internal::wire::common::generate_operation_id()
    );
    let prepared_package = context.product.prepare_current_key_package(
        service_meta(
            client,
            &context.device,
            service_did.as_str(),
            &publish_operation,
            GROUP_E2EE_TRANSPORT_PROFILE_V2,
            &now_text,
        ),
        V2GenerateKeyPackageInput {
            owner_did: client.did().as_str().to_owned(),
            owner_device_id: context.device.device_id.clone(),
            verification_method: context.device.signing_key_id.clone(),
            key_package_id,
            issued_at: now_text.clone(),
            expires_at,
            now: now_text.clone(),
            draft_extension_negotiated: true,
            request_id: format!("{publish_operation}-local"),
        },
        &context.did_document,
        &context.device_signing_private_key,
    )?;
    context
        .product
        .publish_current_key_package(&prepared_package)?;

    let create_operation = operation_id("create");
    let submission = context.product.prepare_create(V2CreateGroupInput {
        meta: service_meta(
            client,
            &context.device,
            service_did.as_str(),
            &create_operation,
            GROUP_E2EE_SECURITY_PROFILE_V2,
            &now_text,
        ),
        group_state_ref,
        creator_key_package: prepared_package.body.group_key_package,
        creator_did_document: context.did_document,
        now: now_text,
        draft_extension_negotiated: true,
        pending_commit_id: format!("pending-{create_operation}"),
        request_id: format!("{create_operation}-prepare"),
    })?;
    context
        .product
        .submit_create(&submission, format!("{create_operation}-finalize"))?;
    Ok(())
}

pub(crate) fn v2_group_state_ref(reference: anp::group_e2ee::GroupStateRef) -> V2GroupStateRef {
    V2GroupStateRef {
        group_did: reference.group_did,
        group_state_version: reference.group_state_version,
        policy_hash: reference.policy_hash,
        roster_hash: None,
    }
}

pub(crate) fn required_created_group_state_ref(
    group_did: &str,
    reference: Option<anp::group_e2ee::GroupStateRef>,
) -> crate::ImResult<V2GroupStateRef> {
    let reference = reference.ok_or_else(|| crate::ImError::LocalStateUnavailable {
        detail: format!("P4 group.create response for {group_did} omitted group_state_ref"),
    })?;
    if reference.group_did != group_did {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(v2_group_state_ref(reference))
}

fn current_v2_device(client: &crate::core::ImClient) -> crate::ImResult<CurrentV2Device> {
    let alias = client
        .current_identity()
        .local_alias
        .as_deref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let index = crate::internal::identity_store::IdentityStore::new(
        &client.core_inner().sdk_paths().identities,
    )
    .load_index()?;
    let state = index
        .credentials
        .get(alias)
        .and_then(|entry| entry.device_state.as_ref())
        .filter(|state| {
            state.mode == crate::internal::identity_device_state::IdentityDeviceMode::VNext
        })
        .ok_or(crate::ImError::PermissionDenied)?;
    let authorization = state
        .authorization
        .as_ref()
        .filter(|authorization| {
            authorization.status
                == crate::internal::identity_device_state::DeviceAuthorizationStatus::Active
        })
        .ok_or(crate::ImError::PermissionDenied)?;
    Ok(CurrentV2Device {
        device_id: authorization.protocol_device_id.as_str().to_owned(),
        signing_key_id: authorization.signing_key_id.clone(),
    })
}

fn validate_current_device_document(
    did: &str,
    device: &CurrentV2Device,
    document: &Value,
) -> crate::ImResult<()> {
    if document.get("id").and_then(Value::as_str) != Some(did)
        || !anp::authentication::validate_did_document_binding(document, true)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let manifest_device = anp::authentication::find_eligible_device(
        document,
        &device.device_id,
        PROFILE_GROUP_E2EE_V2,
    )
    .map_err(|_| crate::ImError::PermissionDenied)?
    .ok_or(crate::ImError::PermissionDenied)?;
    if manifest_device.signing_key_id != device.signing_key_id {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn require_current_device_selection(
    requested: Option<&str>,
    current_device_id: &str,
) -> crate::ImResult<()> {
    let Some(requested) = requested.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    if requested == current_device_id {
        return Ok(());
    }
    Err(crate::ImError::invalid_input(
        Some("device_id".to_owned()),
        "P6 v2 KeyPackage publish only accepts the current protocol device",
    ))
}

fn service_did(client: &crate::core::ImClient) -> crate::ImResult<crate::ids::Did> {
    client
        .core_inner()
        .sdk_config()
        .anp_service_did
        .clone()
        .ok_or_else(|| {
            crate::ImError::invalid_input(
                Some("anp_service_did".to_owned()),
                "P6 v2 lifecycle requires ImCoreConfig.anp_service_did",
            )
        })
}

fn service_meta(
    client: &crate::core::ImClient,
    device: &CurrentV2Device,
    service_did: &str,
    operation_id: &str,
    security_profile: &str,
    created_at: &str,
) -> V2ServiceMetadata {
    V2ServiceMetadata {
        anp_version: Some("2.0".to_owned()),
        profile: GROUP_E2EE_PROFILE_V2.to_owned(),
        security_profile: security_profile.to_owned(),
        sender_did: client.did().as_str().to_owned(),
        sender_device_id: device.device_id.clone(),
        target: V2Target {
            kind: "service".to_owned(),
            did: service_did.to_owned(),
        },
        operation_id: operation_id.to_owned(),
        created_at: Some(created_at.to_owned()),
    }
}

fn operation_id(kind: &str) -> String {
    format!(
        "p6-v2-{kind}-{}",
        crate::internal::wire::common::generate_operation_id()
    )
}

fn format_time(value: OffsetDateTime) -> crate::ImResult<String> {
    value
        .replace_nanosecond(0)
        .unwrap_or(OffsetDateTime::UNIX_EPOCH)
        .format(&Rfc3339)
        .map_err(|error| crate::ImError::Serialization {
            detail: format!("format P6 v2 timestamp: {error}"),
        })
}

#[cfg(test)]
#[path = "v2_lifecycle_tests.rs"]
mod tests;
