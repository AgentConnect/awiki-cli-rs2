//! Production construction and public lifecycle adapters for P6 v2.
//!
//! The public Group API remains device-agnostic. This module binds each P6
//! operation to the authoritative active vNext device authorization and never
//! accepts a caller-selected sibling or the legacy `default` device. The same
//! persisted projection scopes the P6 cryptographic runtime after a restart.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use anp::authentication::{DeviceManifest, PROFILE_GROUP_E2EE_V2};
use anp::group_e2ee::operations::v2::{
    V2AddMemberInput, V2CreateGroupInput, V2GenerateKeyPackageInput, V2InspectLocalGroupInput,
    V2LocalGroupMemberEndpoint, V2LocalGroupReadiness, V2PreparedAdd, V2PreparedRemove,
    V2ReconciledPendingCommit, V2RemoveMemberInput,
};
use anp::group_e2ee::{
    V2GetKeyPackageBody, V2GroupAddBody, V2GroupControlMetadata, V2GroupRemoveBody,
    V2GroupStateRef, V2ServiceMetadata, V2Target, GROUP_E2EE_MTI_SUITE_V2, GROUP_E2EE_PROFILE_V2,
    GROUP_E2EE_SECURITY_PROFILE_V2, GROUP_E2EE_TRANSPORT_PROFILE_V2,
};
use serde_json::Value;
use time::{format_description::well_known::Rfc3339, Duration, OffsetDateTime};

use crate::internal::proof::origin::OriginProofIdentity;

use super::v2_product::{
    GroupE2eeV2Product, RpcGroupE2eeV2Host, V2PreparedAddSubmission, V2PreparedRemoveSubmission,
};

const KEY_PACKAGE_TTL: Duration = Duration::days(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CurrentV2Device {
    pub(crate) device_id: String,
    pub(crate) signing_key_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct P4MemberTransition {
    pub(crate) group_state_ref: V2GroupStateRef,
    pub(crate) member_did: String,
}

pub(crate) type ProductionV2Product<'a> =
    GroupE2eeV2Product<RpcGroupE2eeV2Host<crate::internal::transport::CoreHttpTransport<'a>>>;

pub(crate) struct ProductionV2Context<'a> {
    pub(crate) product: ProductionV2Product<'a>,
    pub(crate) device: CurrentV2Device,
    pub(crate) did_document: Value,
    identity_signer: Arc<dyn crate::internal::key_provider::IdentitySigner>,
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
    // P6 authorization is derived from the currently published P2 document,
    // not from the possibly stale local bootstrap copy. This also makes a
    // remotely revoked controller fail closed before it can prepare a commit.
    let did_document = resolve_member_document_fresh(client, client.did().as_str())?;
    build_production_context(client, device, did_document)
}

async fn production_context_async(
    client: &crate::core::ImClient,
) -> crate::ImResult<ProductionV2Context<'_>> {
    let device = current_v2_device(client)?;
    let did_document = resolve_member_document_fresh_async(client, client.did().as_str()).await?;
    build_production_context(client, device, did_document)
}

fn build_production_context<'a>(
    client: &'a crate::core::ImClient,
    device: CurrentV2Device,
    did_document: Value,
) -> crate::ImResult<ProductionV2Context<'a>> {
    validate_current_device_document(client.did().as_str(), &device, &did_document)?;

    let signer = client
        .runtime()
        .key_provider
        .async_session()
        .map(crate::internal::proof::origin::OriginProofSigner::Provider)
        .unwrap_or_else(|| {
            crate::internal::proof::origin::OriginProofSigner::Identity(Arc::clone(
                &client.runtime().key_provider,
            ))
        });
    let proof_identity = OriginProofIdentity {
        identity_name: client.current_identity().id.as_str().to_owned(),
        did_document: Some(did_document.clone()),
        signer,
        verification_method: Some(device.signing_key_id.clone()),
    };
    let runtime = super::v2_runtime::runtime_for_client(client)?;
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    let p6_client_instance_id =
        crate::internal::local_state::sync_v2::load_or_create_sync_client_instance_id(
            &connection,
            client.current_identity().id.as_str(),
        )?;
    let host = RpcGroupE2eeV2Host::new(
        crate::internal::transport::CoreHttpTransport::new(client),
        proof_identity,
    )
    .with_p6_client_instance_id(p6_client_instance_id);
    Ok(ProductionV2Context {
        product: GroupE2eeV2Product::new(runtime, host),
        device,
        did_document,
        identity_signer: Arc::clone(&client.runtime().key_provider),
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
        context.identity_signer.as_ref(),
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

pub(crate) async fn publish_current_key_package_async(
    client: &crate::core::ImClient,
    request: crate::groups::GroupKeyPackagePublishRequest,
) -> crate::ImResult<crate::groups::GroupKeyPackagePublishResult> {
    let context = production_context_async(client).await?;
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
    let prepared = context
        .product
        .prepare_current_key_package_async(
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
            context.identity_signer.as_ref(),
        )
        .await?;
    let mut transport = crate::internal::transport::CoreHttpTransport::new(client);
    let accepted = context
        .product
        .publish_current_key_package_async(&mut transport, &prepared)
        .await?;
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

/// Publishes a stable P6 KeyPackage family for the selected current device.
/// The caller supplies deterministic family base IDs; the SDK WAL may return a
/// fresh attempt-specific `meta` and body after an unaccepted attempt expires.
/// Only that returned wire attempt is sent to the Host, while an accepted
/// family remains terminal and has no network side effect on later retries.
pub(crate) async fn publish_stable_key_package(
    client: &crate::core::ImClient,
    expected_device_id: &str,
    base_operation_id: &str,
    base_key_package_id: &str,
) -> crate::ImResult<()> {
    let context = production_context(client)?;
    require_current_device_selection(Some(expected_device_id), &context.device.device_id)?;
    let service_did = service_did(client)?;
    let now = OffsetDateTime::now_utc();
    let now_text = format_time(now)?;
    let expires_at = format_time(now + KEY_PACKAGE_TTL)?;
    let mut meta = service_meta(
        client,
        &context.device,
        service_did.as_str(),
        base_operation_id,
        GROUP_E2EE_TRANSPORT_PROFILE_V2,
        &now_text,
    );
    // A volatile wire timestamp would change the stable WAL digest after a
    // restart. P6 permits this field to be omitted.
    meta.created_at = None;
    let prepared = context
        .product
        .prepare_current_key_package_async(
            meta,
            V2GenerateKeyPackageInput {
                owner_did: client.did().as_str().to_owned(),
                owner_device_id: context.device.device_id.clone(),
                verification_method: context.device.signing_key_id.clone(),
                key_package_id: base_key_package_id.to_owned(),
                issued_at: now_text.clone(),
                expires_at,
                now: now_text,
                draft_extension_negotiated: true,
                request_id: format!("{base_operation_id}-local"),
            },
            &context.did_document,
            context.identity_signer.as_ref(),
        )
        .await?;
    let mut transport = crate::internal::transport::CoreHttpTransport::new(client);
    context
        .product
        .publish_current_key_package_async(&mut transport, &prepared)
        .await?;
    Ok(())
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
        context.identity_signer.as_ref(),
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

pub(crate) async fn initialize_created_group_async(
    client: &crate::core::ImClient,
    group_state_ref: V2GroupStateRef,
) -> crate::ImResult<()> {
    let mut context = production_context_async(client).await?;
    let service_did = service_did(client)?;
    let now = OffsetDateTime::now_utc();
    let now_text = format_time(now)?;
    let expires_at = format_time(now + KEY_PACKAGE_TTL)?;
    let publish_operation = operation_id("create-key-package");
    let key_package_id = format!(
        "kp-{}",
        crate::internal::wire::common::generate_operation_id()
    );
    let prepared_package = context
        .product
        .prepare_current_key_package_async(
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
            context.identity_signer.as_ref(),
        )
        .await?;
    let mut transport = crate::internal::transport::CoreHttpTransport::new(client);
    context
        .product
        .publish_current_key_package_async(&mut transport, &prepared_package)
        .await?;

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
        .submit_create_async(&submission, format!("{create_operation}-finalize"))
        .await?;
    Ok(())
}

/// Verifies that the selected current device owns an active local leaf before
/// a public P4 membership mutation is attempted.
pub(crate) fn preflight_current_controller(
    client: &crate::core::ImClient,
    group_did: &str,
) -> crate::ImResult<()> {
    let context = production_context(client)?;
    let endpoints = context
        .product
        .list_local_group_member_endpoints(endpoint_inventory_input(
            client,
            &context.device,
            group_did,
            "preflight",
        ))?;
    require_current_controller_endpoint(
        &endpoints.member_endpoints,
        client.did().as_str(),
        &context.device.device_id,
    )
}

pub(crate) async fn preflight_current_controller_async(
    client: &crate::core::ImClient,
    group_did: &str,
) -> crate::ImResult<()> {
    let context = production_context_async(client).await?;
    let endpoints = context
        .product
        .list_local_group_member_endpoints(endpoint_inventory_input(
            client,
            &context.device,
            group_did,
            "preflight",
        ))?;
    require_current_controller_endpoint(
        &endpoints.member_endpoints,
        client.did().as_str(),
        &context.device.device_id,
    )
}

/// Adds one leaf for every currently eligible target device that is not yet
/// present in the selected group's accepted local MLS tree.
///
/// P4 has already made `member_did` active and supplied `group_state_ref`.
/// Each target device is then fetched and committed independently so the
/// Group Host can route its Welcome to that exact endpoint.
pub(crate) fn add_active_member_devices(
    client: &crate::core::ImClient,
    group_state_ref: V2GroupStateRef,
    member_did: &str,
) -> crate::ImResult<usize> {
    require_exact_group(&group_state_ref)?;
    let group_did = group_state_ref.group_did.clone();
    let mut next_state_ref = Some(group_state_ref);
    let mut changed = 0usize;
    for _ in 0..256 {
        let member_document = resolve_member_document_fresh(client, member_did)?;
        let manifest = validated_p6_manifest(member_did, &member_document)?;
        let desired = eligible_device_ids(&manifest);
        if desired.is_empty() {
            // A new P4 member is not converged until at least one of its
            // current devices can become a P6 Leaf. Whole-roster repair uses
            // the same Manifest parser but deliberately permits an empty set
            // so it can remove Leaves after the last eligible device is lost.
            return Err(crate::ImError::PermissionDenied);
        }
        let mut context = production_context(client)?;
        if resume_pending_membership_commits(client, &mut context, &group_did)? > 0 {
            // A replayed/finalized SDK WAL entry is itself a completed P6
            // step. Do not reuse the caller's pre-recovery P4 reference for
            // the next device; reload the authoritative state below.
            next_state_ref = None;
        }
        let endpoints =
            context
                .product
                .list_local_group_member_endpoints(endpoint_inventory_input(
                    client,
                    &context.device,
                    &group_did,
                    "add-reconcile",
                ))?;
        require_current_controller_endpoint(
            &endpoints.member_endpoints,
            client.did().as_str(),
            &context.device.device_id,
        )?;
        let desired = desired
            .into_iter()
            .map(|device_id| (member_did.to_owned(), device_id))
            .collect::<BTreeSet<_>>();
        let observed = endpoint_set_for_member(member_did, &endpoints.member_endpoints);
        let (extra, missing) = roster_delta(&desired, &observed);
        let extra = extra.into_iter().next().map(|(_, device_id)| device_id);
        let missing = missing.into_iter().next().map(|(_, device_id)| device_id);
        if extra.is_none() && missing.is_none() {
            return Ok(changed);
        }
        let state_ref = match next_state_ref.take() {
            Some(reference) => reference,
            None => fresh_group_state_ref(client, &group_did)?,
        };
        if let Some(device_id) = extra {
            submit_exact_remove(
                client,
                &mut context,
                state_ref,
                member_did,
                &device_id,
                member_document,
            )?;
        } else if let Some(device_id) = missing {
            submit_exact_add(
                client,
                &mut context,
                state_ref,
                member_did,
                &device_id,
                member_document,
            )?;
        }
        changed = changed.saturating_add(1);
    }
    Err(crate::ImError::LocalStateUnavailable {
        detail: "P6 v2 target device roster did not stabilize".to_owned(),
    })
}

pub(crate) async fn add_active_member_devices_async(
    client: &crate::core::ImClient,
    group_state_ref: V2GroupStateRef,
    member_did: &str,
) -> crate::ImResult<usize> {
    require_exact_group(&group_state_ref)?;
    let group_did = group_state_ref.group_did.clone();
    let mut next_state_ref = Some(group_state_ref);
    let mut changed = 0usize;
    for _ in 0..256 {
        let member_document = resolve_member_document_fresh_async(client, member_did).await?;
        let manifest = validated_p6_manifest(member_did, &member_document)?;
        let desired = eligible_device_ids(&manifest);
        if desired.is_empty() {
            return Err(crate::ImError::PermissionDenied);
        }
        let mut context = production_context_async(client).await?;
        if resume_pending_membership_commits_async(client, &mut context, &group_did).await? > 0 {
            next_state_ref = None;
        }
        let endpoints =
            context
                .product
                .list_local_group_member_endpoints(endpoint_inventory_input(
                    client,
                    &context.device,
                    &group_did,
                    "add-reconcile",
                ))?;
        require_current_controller_endpoint(
            &endpoints.member_endpoints,
            client.did().as_str(),
            &context.device.device_id,
        )?;
        let desired = desired
            .into_iter()
            .map(|device_id| (member_did.to_owned(), device_id))
            .collect::<BTreeSet<_>>();
        let observed = endpoint_set_for_member(member_did, &endpoints.member_endpoints);
        let (extra, missing) = roster_delta(&desired, &observed);
        let extra = extra.into_iter().next().map(|(_, device_id)| device_id);
        let missing = missing.into_iter().next().map(|(_, device_id)| device_id);
        if extra.is_none() && missing.is_none() {
            return Ok(changed);
        }
        let state_ref = match next_state_ref.take() {
            Some(reference) => reference,
            None => fresh_group_state_ref_async(client, &group_did).await?,
        };
        if let Some(device_id) = extra {
            submit_exact_remove_async(
                client,
                &mut context,
                state_ref,
                member_did,
                &device_id,
                member_document,
            )
            .await?;
        } else if let Some(device_id) = missing {
            submit_exact_add_async(
                client,
                &mut context,
                state_ref,
                member_did,
                &device_id,
                member_document,
            )
            .await?;
        }
        changed = changed.saturating_add(1);
    }
    Err(crate::ImError::LocalStateUnavailable {
        detail: "P6 v2 target device roster did not stabilize".to_owned(),
    })
}

/// Removes every accepted local leaf for the P4 member that has just become
/// removed. The target list comes from the authenticated MLS tree rather than
/// the current Manifest because Manifest loss is itself a valid Remove trigger.
pub(crate) fn remove_inactive_member_devices(
    client: &crate::core::ImClient,
    group_state_ref: V2GroupStateRef,
    member_did: &str,
) -> crate::ImResult<usize> {
    require_exact_group(&group_state_ref)?;
    let member_document = resolve_member_document(client, member_did)?;
    validate_member_document_id(member_did, &member_document)?;
    let group_did = group_state_ref.group_did.clone();
    let mut next_state_ref = Some(group_state_ref);
    let mut removed = 0usize;
    for _ in 0..256 {
        let mut context = production_context(client)?;
        if resume_pending_membership_commits(client, &mut context, &group_did)? > 0 {
            next_state_ref = None;
        }
        let endpoints =
            context
                .product
                .list_local_group_member_endpoints(endpoint_inventory_input(
                    client,
                    &context.device,
                    &group_did,
                    "remove-reconcile",
                ))?;
        require_current_controller_endpoint(
            &endpoints.member_endpoints,
            client.did().as_str(),
            &context.device.device_id,
        )?;
        let Some(target_device_id) = member_device_ids(member_did, &endpoints.member_endpoints)
            .into_iter()
            .next()
        else {
            return Ok(removed);
        };
        let state_ref = match next_state_ref.take() {
            Some(reference) => reference,
            None => fresh_group_state_ref(client, &group_did)?,
        };
        submit_exact_remove(
            client,
            &mut context,
            state_ref,
            member_did,
            &target_device_id,
            member_document.clone(),
        )?;
        removed = removed.saturating_add(1);
    }
    Err(crate::ImError::LocalStateUnavailable {
        detail: "P6 v2 removed-member endpoint set did not converge".to_owned(),
    })
}

pub(crate) async fn remove_inactive_member_devices_async(
    client: &crate::core::ImClient,
    group_state_ref: V2GroupStateRef,
    member_did: &str,
) -> crate::ImResult<usize> {
    require_exact_group(&group_state_ref)?;
    let member_document = resolve_member_document_fresh_async(client, member_did).await?;
    validate_member_document_id(member_did, &member_document)?;
    let group_did = group_state_ref.group_did.clone();
    let mut next_state_ref = Some(group_state_ref);
    let mut removed = 0usize;
    for _ in 0..256 {
        let mut context = production_context_async(client).await?;
        if resume_pending_membership_commits_async(client, &mut context, &group_did).await? > 0 {
            next_state_ref = None;
        }
        let endpoints =
            context
                .product
                .list_local_group_member_endpoints(endpoint_inventory_input(
                    client,
                    &context.device,
                    &group_did,
                    "remove-reconcile",
                ))?;
        require_current_controller_endpoint(
            &endpoints.member_endpoints,
            client.did().as_str(),
            &context.device.device_id,
        )?;
        let Some(target_device_id) = member_device_ids(member_did, &endpoints.member_endpoints)
            .into_iter()
            .next()
        else {
            return Ok(removed);
        };
        let state_ref = match next_state_ref.take() {
            Some(reference) => reference,
            None => fresh_group_state_ref_async(client, &group_did).await?,
        };
        submit_exact_remove_async(
            client,
            &mut context,
            state_ref,
            member_did,
            &target_device_id,
            member_document.clone(),
        )
        .await?;
        removed = removed.saturating_add(1);
    }
    Err(crate::ImError::LocalStateUnavailable {
        detail: "P6 v2 removed-member endpoint set did not converge".to_owned(),
    })
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct V2RosterReconcileSummary {
    pub(crate) added_devices: usize,
    pub(crate) removed_devices: usize,
    pub(crate) repaired_wal_entries: usize,
    pub(crate) remaining_devices: usize,
}

/// Reconciles one selected group's MLS endpoints to the authoritative P4
/// active-member set and each member's fresh P2 device Manifest. Progress is
/// derived from the accepted SDK tree after every commit; the SDK WAL remains
/// the only crash-recovery state machine.
pub(crate) fn reconcile_group_device_roster(
    client: &crate::core::ImClient,
    group: crate::ids::GroupRef,
) -> crate::ImResult<V2RosterReconcileSummary> {
    if !current_actor_is_active_owner(client, group.clone())? {
        return Ok(V2RosterReconcileSummary::default());
    }
    let group_did = group.as_str().to_owned();
    let mut summary = V2RosterReconcileSummary::default();
    for _ in 0..256 {
        let mut context = production_context(client)?;
        summary.repaired_wal_entries =
            summary
                .repaired_wal_entries
                .saturating_add(resume_pending_membership_commits(
                    client,
                    &mut context,
                    &group_did,
                )?);
        let inspected = context
            .product
            .inspect_local_group(endpoint_inventory_input(
                client,
                &context.device,
                &group_did,
                "group-roster-reconcile-readiness",
            ))?;
        // A same-DID sibling inherits the P4 business role but does not own
        // this historical group's MLS state until it processes a Welcome.
        // Repair must remain a secret-free no-op instead of synthesizing or
        // copying a controller Leaf from another device.
        if !local_group_can_reconcile_roster(&inspected.readiness) {
            return Ok(summary);
        }
        // The fresh P4 reference and desired member/device set are read after
        // WAL repair, so each subsequent P6 step starts from the latest
        // authoritative business state.
        let (group_state_ref, desired) = fresh_desired_group_roster(client, group.clone())?;
        let endpoints =
            context
                .product
                .list_local_group_member_endpoints(endpoint_inventory_input(
                    client,
                    &context.device,
                    &group_did,
                    "group-roster-reconcile",
                ))?;
        require_current_controller_endpoint(
            &endpoints.member_endpoints,
            client.did().as_str(),
            &context.device.device_id,
        )?;

        let observed = endpoints
            .member_endpoints
            .iter()
            .map(|endpoint| {
                (
                    endpoint.member_did.clone(),
                    endpoint.member_device_id.clone(),
                )
            })
            .collect::<BTreeSet<_>>();
        let desired_endpoints = desired
            .iter()
            .flat_map(|(member_did, member)| {
                member
                    .device_ids
                    .iter()
                    .map(|device_id| (member_did.clone(), device_id.clone()))
            })
            .collect::<BTreeSet<_>>();
        let (extra, missing) = roster_delta(&desired_endpoints, &observed);
        summary.remaining_devices = extra.len().saturating_add(missing.len());
        let extra = extra.into_iter().next();
        let missing = missing.into_iter().next();
        if extra.is_none() && missing.is_none() {
            return Ok(summary);
        }

        if let Some((member_did, member_device_id)) = extra {
            let document = match desired.get(&member_did) {
                Some(member) => member.document.clone(),
                None => resolve_member_document(client, &member_did)?,
            };
            validate_member_document_id(&member_did, &document)?;
            submit_exact_remove(
                client,
                &mut context,
                group_state_ref,
                &member_did,
                &member_device_id,
                document,
            )?;
            summary.removed_devices = summary.removed_devices.saturating_add(1);
        } else if let Some((member_did, member_device_id)) = missing {
            let member = desired
                .get(&member_did)
                .ok_or(crate::ImError::PermissionDenied)?;
            submit_exact_add(
                client,
                &mut context,
                group_state_ref,
                &member_did,
                &member_device_id,
                member.document.clone(),
            )?;
            summary.added_devices = summary.added_devices.saturating_add(1);
        }
    }
    Err(crate::ImError::LocalStateUnavailable {
        detail: "P6 v2 group device roster did not stabilize".to_owned(),
    })
}

pub(crate) async fn reconcile_group_device_roster_async(
    client: &crate::core::ImClient,
    group: crate::ids::GroupRef,
) -> crate::ImResult<V2RosterReconcileSummary> {
    if !current_actor_is_active_owner_async(client, group.clone()).await? {
        return Ok(V2RosterReconcileSummary::default());
    }
    let group_did = group.as_str().to_owned();
    let mut summary = V2RosterReconcileSummary::default();
    for _ in 0..256 {
        let mut context = production_context_async(client).await?;
        summary.repaired_wal_entries = summary.repaired_wal_entries.saturating_add(
            resume_pending_membership_commits_async(client, &mut context, &group_did).await?,
        );
        let inspected = context
            .product
            .inspect_local_group(endpoint_inventory_input(
                client,
                &context.device,
                &group_did,
                "group-roster-reconcile-readiness",
            ))?;
        if !local_group_can_reconcile_roster(&inspected.readiness) {
            return Ok(summary);
        }
        let (group_state_ref, desired) =
            fresh_desired_group_roster_async(client, group.clone()).await?;
        let endpoints =
            context
                .product
                .list_local_group_member_endpoints(endpoint_inventory_input(
                    client,
                    &context.device,
                    &group_did,
                    "group-roster-reconcile",
                ))?;
        require_current_controller_endpoint(
            &endpoints.member_endpoints,
            client.did().as_str(),
            &context.device.device_id,
        )?;

        let observed = endpoints
            .member_endpoints
            .iter()
            .map(|endpoint| {
                (
                    endpoint.member_did.clone(),
                    endpoint.member_device_id.clone(),
                )
            })
            .collect::<BTreeSet<_>>();
        let desired_endpoints = desired
            .iter()
            .flat_map(|(member_did, member)| {
                member
                    .device_ids
                    .iter()
                    .map(|device_id| (member_did.clone(), device_id.clone()))
            })
            .collect::<BTreeSet<_>>();
        let (extra, missing) = roster_delta(&desired_endpoints, &observed);
        summary.remaining_devices = extra.len().saturating_add(missing.len());
        let extra = extra.into_iter().next();
        let missing = missing.into_iter().next();
        if extra.is_none() && missing.is_none() {
            return Ok(summary);
        }

        if let Some((member_did, member_device_id)) = extra {
            let document = match desired.get(&member_did) {
                Some(member) => member.document.clone(),
                None => resolve_member_document_fresh_async(client, &member_did).await?,
            };
            validate_member_document_id(&member_did, &document)?;
            submit_exact_remove_async(
                client,
                &mut context,
                group_state_ref,
                &member_did,
                &member_device_id,
                document,
            )
            .await?;
            summary.removed_devices = summary.removed_devices.saturating_add(1);
        } else if let Some((member_did, member_device_id)) = missing {
            let member = desired
                .get(&member_did)
                .ok_or(crate::ImError::PermissionDenied)?;
            submit_exact_add_async(
                client,
                &mut context,
                group_state_ref,
                &member_did,
                &member_device_id,
                member.document.clone(),
            )
            .await?;
            summary.added_devices = summary.added_devices.saturating_add(1);
        }
    }
    Err(crate::ImError::LocalStateUnavailable {
        detail: "P6 v2 group device roster did not stabilize".to_owned(),
    })
}

fn current_actor_is_active_owner(
    client: &crate::core::ImClient,
    group: crate::ids::GroupRef,
) -> crate::ImResult<bool> {
    let group_did = group.as_str().to_owned();
    let authoritative = crate::internal::group_runtime::read::GroupReadRuntime::new(
        client,
        crate::internal::auth::session::FileSessionProvider::new(client),
        crate::internal::transport::CoreHttpTransport::new(client),
    )
    .get_with_policy(group)?;
    if !crate::groups::authoritative_group_e2ee_classification(&group_did, &authoritative)? {
        return Ok(false);
    }
    Ok(authoritative.group.as_ref().is_some_and(|snapshot| {
        snapshot.did.as_str() == group_did
            && snapshot.membership_status.as_deref() == Some("active")
            && snapshot.my_role.as_deref() == Some("owner")
    }))
}

async fn current_actor_is_active_owner_async(
    client: &crate::core::ImClient,
    group: crate::ids::GroupRef,
) -> crate::ImResult<bool> {
    let group_did = group.as_str().to_owned();
    let authoritative = crate::internal::group_runtime::read::GroupReadRuntime::new(
        client,
        crate::internal::auth::session::FileSessionProvider::new(client),
        crate::internal::transport::CoreHttpTransport::new(client),
    )
    .get_with_policy_async(group)
    .await?;
    if !crate::groups::authoritative_group_e2ee_classification(&group_did, &authoritative)? {
        return Ok(false);
    }
    Ok(authoritative.group.as_ref().is_some_and(|snapshot| {
        snapshot.did.as_str() == group_did
            && snapshot.membership_status.as_deref() == Some("active")
            && snapshot.my_role.as_deref() == Some("owner")
    }))
}

#[derive(Debug, Clone)]
struct DesiredP6Member {
    document: Value,
    device_ids: Vec<String>,
}

fn fresh_desired_group_roster(
    client: &crate::core::ImClient,
    group: crate::ids::GroupRef,
) -> crate::ImResult<(V2GroupStateRef, BTreeMap<String, DesiredP6Member>)> {
    for attempt in 0..4 {
        match fresh_desired_group_roster_attempt(client, group.clone()) {
            Err(crate::ImError::CursorStale) if attempt < 3 => continue,
            result => return result,
        }
    }
    Err(crate::ImError::CursorStale)
}

async fn fresh_desired_group_roster_async(
    client: &crate::core::ImClient,
    group: crate::ids::GroupRef,
) -> crate::ImResult<(V2GroupStateRef, BTreeMap<String, DesiredP6Member>)> {
    for attempt in 0..4 {
        match fresh_desired_group_roster_attempt_async(client, group.clone()).await {
            Err(crate::ImError::CursorStale) if attempt < 3 => continue,
            result => return result,
        }
    }
    Err(crate::ImError::CursorStale)
}

async fn fresh_desired_group_roster_attempt_async(
    client: &crate::core::ImClient,
    group: crate::ids::GroupRef,
) -> crate::ImResult<(V2GroupStateRef, BTreeMap<String, DesiredP6Member>)> {
    let group_did = group.as_str().to_owned();
    let authoritative = crate::internal::group_runtime::read::GroupReadRuntime::new(
        client,
        crate::internal::auth::session::FileSessionProvider::new(client),
        crate::internal::transport::CoreHttpTransport::new(client),
    )
    .get_with_policy_async(group.clone())
    .await?;
    if !crate::groups::authoritative_group_e2ee_classification(&group_did, &authoritative)? {
        return Err(crate::ImError::PermissionDenied);
    }
    let snapshot = authoritative
        .group
        .as_ref()
        .filter(|snapshot| snapshot.did.as_str() == group_did)
        .ok_or_else(|| crate::ImError::LocalStateUnavailable {
            detail: "authoritative group.get omitted caller membership".to_owned(),
        })?;
    if snapshot.membership_status.as_deref() != Some("active")
        || snapshot.my_role.as_deref() != Some("owner")
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let raw =
        authoritative
            .raw_response()
            .ok_or_else(|| crate::ImError::LocalStateUnavailable {
                detail: "authoritative group.get omitted raw group state".to_owned(),
            })?;
    validate_embedded_group_state_refs(&group_did, raw)?;
    let state_ref = crate::internal::group_e2ee::state_ref::group_state_ref_from_group_response(
        &group_did, raw,
    )
    .map(v2_group_state_ref)
    .ok_or_else(|| crate::ImError::LocalStateUnavailable {
        detail: "authoritative group.get omitted group_state_version".to_owned(),
    })?;
    let max_members = super::member_collector::product_max_members(raw)?;
    let members = super::member_collector::collect_complete_group_members_async(
        client,
        group.clone(),
        Some(&state_ref.group_state_version),
        max_members,
    )
    .await?;
    if members.group_state_version != state_ref.group_state_version {
        return Err(crate::ImError::CursorStale);
    }

    let mut desired = BTreeMap::new();
    for member in members.members {
        if member.status.as_deref().unwrap_or("active") != "active" {
            continue;
        }
        let did = member
            .did
            .ok_or_else(|| crate::ImError::LocalStateUnavailable {
                detail: "active P4 member omitted its current DID".to_owned(),
            })?;
        let document = resolve_member_document_fresh_async(client, did.as_str()).await?;
        let manifest = validated_p6_manifest(did.as_str(), &document)?;
        desired.insert(
            did.as_str().to_owned(),
            DesiredP6Member {
                document,
                device_ids: eligible_device_ids(&manifest),
            },
        );
    }
    let refreshed = crate::internal::group_runtime::read::GroupReadRuntime::new(
        client,
        crate::internal::auth::session::FileSessionProvider::new(client),
        crate::internal::transport::CoreHttpTransport::new(client),
    )
    .get_with_policy_async(group)
    .await?;
    if !group_state_version_matches(
        refreshed.raw_response(),
        state_ref.group_state_version.as_str(),
    ) {
        return Err(crate::ImError::CursorStale);
    }
    Ok((state_ref, desired))
}

fn fresh_desired_group_roster_attempt(
    client: &crate::core::ImClient,
    group: crate::ids::GroupRef,
) -> crate::ImResult<(V2GroupStateRef, BTreeMap<String, DesiredP6Member>)> {
    let group_did = group.as_str().to_owned();
    let authoritative = crate::internal::group_runtime::read::GroupReadRuntime::new(
        client,
        crate::internal::auth::session::FileSessionProvider::new(client),
        crate::internal::transport::CoreHttpTransport::new(client),
    )
    .get_with_policy(group.clone())?;
    if !crate::groups::authoritative_group_e2ee_classification(&group_did, &authoritative)? {
        return Err(crate::ImError::PermissionDenied);
    }
    let snapshot = authoritative
        .group
        .as_ref()
        .filter(|snapshot| snapshot.did.as_str() == group_did)
        .ok_or_else(|| crate::ImError::LocalStateUnavailable {
            detail: "authoritative group.get omitted caller membership".to_owned(),
        })?;
    if snapshot.membership_status.as_deref() != Some("active")
        || snapshot.my_role.as_deref() != Some("owner")
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let raw =
        authoritative
            .raw_response()
            .ok_or_else(|| crate::ImError::LocalStateUnavailable {
                detail: "authoritative group.get omitted raw group state".to_owned(),
            })?;
    validate_embedded_group_state_refs(&group_did, raw)?;
    let state_ref = crate::internal::group_e2ee::state_ref::group_state_ref_from_group_response(
        &group_did, raw,
    )
    .map(v2_group_state_ref)
    .ok_or_else(|| crate::ImError::LocalStateUnavailable {
        detail: "authoritative group.get omitted group_state_version".to_owned(),
    })?;
    let max_members =
        super::member_collector::product_max_members(authoritative.raw_response().unwrap_or(raw))?;
    let members = super::member_collector::collect_complete_group_members(
        client,
        group.clone(),
        Some(&state_ref.group_state_version),
        max_members,
    )?;
    if members.group_state_version != state_ref.group_state_version {
        return Err(crate::ImError::CursorStale);
    }

    let mut desired = BTreeMap::new();
    for member in members.members {
        if member.status.as_deref().unwrap_or("active") != "active" {
            continue;
        }
        let did = member
            .did
            .ok_or_else(|| crate::ImError::LocalStateUnavailable {
                detail: "active P4 member omitted its current DID".to_owned(),
            })?;
        let document = resolve_member_document_fresh(client, did.as_str())?;
        let manifest = validated_p6_manifest(did.as_str(), &document)?;
        desired.insert(
            did.as_str().to_owned(),
            DesiredP6Member {
                document,
                device_ids: eligible_device_ids(&manifest),
            },
        );
    }
    let refreshed = crate::internal::group_runtime::read::GroupReadRuntime::new(
        client,
        crate::internal::auth::session::FileSessionProvider::new(client),
        crate::internal::transport::CoreHttpTransport::new(client),
    )
    .get_with_policy(group)?;
    if !group_state_version_matches(
        refreshed.raw_response(),
        state_ref.group_state_version.as_str(),
    ) {
        return Err(crate::ImError::CursorStale);
    }
    Ok((state_ref, desired))
}

fn group_state_version_matches(raw: Option<&Value>, expected: &str) -> bool {
    raw.and_then(|raw| raw.get("group_state_version"))
        .and_then(Value::as_str)
        == Some(expected)
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

pub(crate) fn required_member_transition(
    group_did: &str,
    expected_member_did: Option<&str>,
    expected_status: &str,
    result: &crate::groups::GroupReadResult,
) -> crate::ImResult<P4MemberTransition> {
    let raw = result
        .raw_response()
        .ok_or_else(|| crate::ImError::LocalStateUnavailable {
            detail: "P4 member transition omitted its authoritative response".to_owned(),
        })?;
    if raw.get("group_did").and_then(Value::as_str) != Some(group_did) {
        return Err(crate::ImError::PermissionDenied);
    }
    let member_did = raw
        .get("member_did")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| value.starts_with("did:"))
        .ok_or(crate::ImError::PermissionDenied)?;
    if crate::ids::Did::parse(member_did).is_err() {
        return Err(crate::ImError::PermissionDenied);
    }
    if expected_member_did.is_some_and(|expected| expected != member_did) {
        return Err(crate::ImError::PermissionDenied);
    }
    if let Some(status) = raw.get("membership_status").and_then(Value::as_str) {
        if status != expected_status {
            return Err(crate::ImError::PermissionDenied);
        }
    } else if expected_status == "active" {
        return Err(crate::ImError::LocalStateUnavailable {
            detail: "P4 group.add response omitted active membership_status".to_owned(),
        });
    }
    validate_embedded_group_state_refs(group_did, raw)?;
    let reference =
        crate::internal::group_e2ee::state_ref::group_state_ref_from_group_response(group_did, raw)
            .ok_or_else(|| crate::ImError::LocalStateUnavailable {
                detail: "P4 member transition omitted group_state_version".to_owned(),
            })?;
    Ok(P4MemberTransition {
        group_state_ref: v2_group_state_ref(reference),
        member_did: member_did.to_owned(),
    })
}

fn resolve_member_document(
    client: &crate::core::ImClient,
    member_did: &str,
) -> crate::ImResult<Value> {
    let mut transport = crate::internal::transport::CoreHttpTransport::new(client);
    super::v2_notice::resolve_did_document(client, &mut transport, member_did)
}

fn resolve_member_document_fresh(
    client: &crate::core::ImClient,
    member_did: &str,
) -> crate::ImResult<Value> {
    let mut transport = crate::internal::transport::CoreHttpTransport::new(client);
    super::v2_notice::resolve_did_document_fresh(client, &mut transport, member_did)
}

async fn resolve_member_document_fresh_async(
    client: &crate::core::ImClient,
    member_did: &str,
) -> crate::ImResult<Value> {
    let mut transport = crate::internal::transport::CoreHttpTransport::new(client);
    super::v2_notice::resolve_did_document_fresh_async(client, &mut transport, member_did).await
}

fn fresh_group_state_ref(
    client: &crate::core::ImClient,
    group_did: &str,
) -> crate::ImResult<V2GroupStateRef> {
    let group = crate::ids::GroupRef::parse(group_did)?;
    let result = crate::internal::group_runtime::read::GroupReadRuntime::new(
        client,
        crate::internal::auth::session::FileSessionProvider::new(client),
        crate::internal::transport::CoreHttpTransport::new(client),
    )
    .get_with_policy(group)?;
    if !crate::groups::authoritative_group_e2ee_classification(group_did, &result)? {
        return Err(crate::ImError::PermissionDenied);
    }
    let raw = result
        .raw_response()
        .ok_or_else(|| crate::ImError::LocalStateUnavailable {
            detail: "authoritative group.get omitted its raw response".to_owned(),
        })?;
    validate_embedded_group_state_refs(group_did, raw)?;
    let reference =
        crate::internal::group_e2ee::state_ref::group_state_ref_from_group_response(group_did, raw)
            .ok_or_else(|| crate::ImError::LocalStateUnavailable {
                detail: "authoritative group.get omitted group_state_version".to_owned(),
            })?;
    Ok(v2_group_state_ref(reference))
}

async fn fresh_group_state_ref_async(
    client: &crate::core::ImClient,
    group_did: &str,
) -> crate::ImResult<V2GroupStateRef> {
    let group = crate::ids::GroupRef::parse(group_did)?;
    let result = crate::internal::group_runtime::read::GroupReadRuntime::new(
        client,
        crate::internal::auth::session::FileSessionProvider::new(client),
        crate::internal::transport::CoreHttpTransport::new(client),
    )
    .get_with_policy_async(group)
    .await?;
    if !crate::groups::authoritative_group_e2ee_classification(group_did, &result)? {
        return Err(crate::ImError::PermissionDenied);
    }
    let raw = result
        .raw_response()
        .ok_or_else(|| crate::ImError::LocalStateUnavailable {
            detail: "authoritative group.get omitted its raw response".to_owned(),
        })?;
    validate_embedded_group_state_refs(group_did, raw)?;
    let reference =
        crate::internal::group_e2ee::state_ref::group_state_ref_from_group_response(group_did, raw)
            .ok_or_else(|| crate::ImError::LocalStateUnavailable {
                detail: "authoritative group.get omitted group_state_version".to_owned(),
            })?;
    Ok(v2_group_state_ref(reference))
}

fn resume_pending_membership_commits(
    client: &crate::core::ImClient,
    context: &mut ProductionV2Context<'_>,
    group_did: &str,
) -> crate::ImResult<usize> {
    let mut repaired = 0usize;
    for _ in 0..256 {
        let reconciled = context
            .product
            .reconcile_pending(format!("{}-wal", operation_id("reconcile")))?;
        repaired = repaired.saturating_add(
            reconciled
                .entries
                .iter()
                .filter(|entry| {
                    entry.pending.group_did == group_did
                        && matches!(entry.pending.status.as_str(), "aborted" | "finalized")
                })
                .count(),
        );
        let Some(entry) = reconciled.entries.into_iter().find(|entry| {
            entry.pending.group_did == group_did && entry.pending.status == "prepared"
        }) else {
            return Ok(repaired);
        };
        let operation_id = entry.pending.operation_id.clone();
        let meta = control_meta(client, &context.device, group_did, &operation_id);
        match prepared_membership_submission(meta, entry.pending)? {
            PreparedMembershipSubmission::Add(submission) => {
                submit_prepared_add(context, &submission, format!("{operation_id}-wal-finalize"))?;
                repaired = repaired.saturating_add(1);
            }
            PreparedMembershipSubmission::Remove(submission) => {
                submit_prepared_remove(
                    context,
                    &submission,
                    format!("{operation_id}-wal-finalize"),
                )?;
                repaired = repaired.saturating_add(1);
            }
        }
    }
    Err(crate::ImError::LocalStateUnavailable {
        detail: "P6 v2 pending membership WAL did not stabilize".to_owned(),
    })
}

async fn resume_pending_membership_commits_async(
    client: &crate::core::ImClient,
    context: &mut ProductionV2Context<'_>,
    group_did: &str,
) -> crate::ImResult<usize> {
    let mut repaired = 0usize;
    for _ in 0..256 {
        let reconciled = context
            .product
            .reconcile_pending(format!("{}-wal", operation_id("reconcile")))?;
        repaired = repaired.saturating_add(
            reconciled
                .entries
                .iter()
                .filter(|entry| {
                    entry.pending.group_did == group_did
                        && matches!(entry.pending.status.as_str(), "aborted" | "finalized")
                })
                .count(),
        );
        let Some(entry) = reconciled.entries.into_iter().find(|entry| {
            entry.pending.group_did == group_did && entry.pending.status == "prepared"
        }) else {
            return Ok(repaired);
        };
        let operation_id = entry.pending.operation_id.clone();
        let meta = control_meta(client, &context.device, group_did, &operation_id);
        match prepared_membership_submission(meta, entry.pending)? {
            PreparedMembershipSubmission::Add(submission) => {
                submit_prepared_add_async(
                    context,
                    &submission,
                    format!("{operation_id}-wal-finalize"),
                )
                .await?;
                repaired = repaired.saturating_add(1);
            }
            PreparedMembershipSubmission::Remove(submission) => {
                submit_prepared_remove_async(
                    context,
                    &submission,
                    format!("{operation_id}-wal-finalize"),
                )
                .await?;
                repaired = repaired.saturating_add(1);
            }
        }
    }
    Err(crate::ImError::LocalStateUnavailable {
        detail: "P6 v2 pending membership WAL did not stabilize".to_owned(),
    })
}

#[derive(Debug, Clone, PartialEq)]
enum PreparedMembershipSubmission {
    Add(V2PreparedAddSubmission),
    Remove(V2PreparedRemoveSubmission),
}

fn prepared_membership_submission(
    meta: V2GroupControlMetadata,
    pending: V2ReconciledPendingCommit,
) -> crate::ImResult<PreparedMembershipSubmission> {
    if pending.status != "prepared"
        || meta.operation_id != pending.operation_id
        || meta.target.did != pending.group_did
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let body = pending
        .prepared_response
        .ok_or_else(|| crate::ImError::LocalStateUnavailable {
            detail: "SDK prepared membership commit omitted its durable body".to_owned(),
        })?;
    if body.get("welcome_b64u").is_some() {
        let body: V2GroupAddBody = serde_json::from_value(body).map_err(|error| {
            crate::ImError::LocalStateUnavailable {
                detail: format!("SDK prepared P6 add body is invalid: {error}"),
            }
        })?;
        body.validate()
            .map_err(|_| crate::ImError::PermissionDenied)?;
        let from_epoch = previous_epoch(&body.epoch)?;
        return Ok(PreparedMembershipSubmission::Add(V2PreparedAddSubmission {
            meta,
            prepared: V2PreparedAdd {
                pending_commit_id: pending.pending_commit_id,
                from_epoch,
                body,
            },
        }));
    }
    if body.get("member_did").is_some() && body.get("member_device_id").is_some() {
        let body: V2GroupRemoveBody = serde_json::from_value(body).map_err(|error| {
            crate::ImError::LocalStateUnavailable {
                detail: format!("SDK prepared P6 remove body is invalid: {error}"),
            }
        })?;
        body.validate()
            .map_err(|_| crate::ImError::PermissionDenied)?;
        let from_epoch = previous_epoch(&body.epoch)?;
        return Ok(PreparedMembershipSubmission::Remove(
            V2PreparedRemoveSubmission {
                meta,
                prepared: V2PreparedRemove {
                    pending_commit_id: pending.pending_commit_id,
                    from_epoch,
                    body,
                },
            },
        ));
    }
    Err(crate::ImError::LocalStateUnavailable {
        detail: "prepared P6 operation requires its dedicated Host recheck".to_owned(),
    })
}

fn submit_exact_add(
    client: &crate::core::ImClient,
    context: &mut ProductionV2Context<'_>,
    group_state_ref: V2GroupStateRef,
    member_did: &str,
    member_device_id: &str,
    member_document: Value,
) -> crate::ImResult<()> {
    let target_service_did = anp::direct_e2ee::message_service_did_from_document(&member_document)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let lookup_now = format_time(OffsetDateTime::now_utc())?;
    let lookup_operation = operation_id("get-key-package");
    let package = context.product.get_target_key_package(
        service_meta(
            client,
            &context.device,
            &target_service_did,
            &lookup_operation,
            GROUP_E2EE_TRANSPORT_PROFILE_V2,
            &lookup_now,
        ),
        V2GetKeyPackageBody {
            target_did: member_did.to_owned(),
            target_device_id: member_device_id.to_owned(),
            preferred_suite: Some(GROUP_E2EE_MTI_SUITE_V2.to_owned()),
            require_fresh: Some(true),
        },
    )?;
    let now_text = format_time(OffsetDateTime::now_utc())?;
    let add_operation = operation_id("add");
    let submission = context.product.prepare_add(V2AddMemberInput {
        meta: control_meta(
            client,
            &context.device,
            &group_state_ref.group_did,
            &add_operation,
        ),
        group_state_ref,
        group_key_package: package.group_key_package,
        member_did_document: member_document,
        now: now_text,
        draft_extension_negotiated: true,
        pending_commit_id: format!("pending-{add_operation}"),
        request_id: format!("{add_operation}-prepare"),
    })?;
    submit_prepared_add(context, &submission, format!("{add_operation}-finalize"))
}

async fn submit_exact_add_async(
    client: &crate::core::ImClient,
    context: &mut ProductionV2Context<'_>,
    group_state_ref: V2GroupStateRef,
    member_did: &str,
    member_device_id: &str,
    member_document: Value,
) -> crate::ImResult<()> {
    let target_service_did = anp::direct_e2ee::message_service_did_from_document(&member_document)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let lookup_now = format_time(OffsetDateTime::now_utc())?;
    let lookup_operation = operation_id("get-key-package");
    let package = context
        .product
        .get_target_key_package_async(
            service_meta(
                client,
                &context.device,
                &target_service_did,
                &lookup_operation,
                GROUP_E2EE_TRANSPORT_PROFILE_V2,
                &lookup_now,
            ),
            V2GetKeyPackageBody {
                target_did: member_did.to_owned(),
                target_device_id: member_device_id.to_owned(),
                preferred_suite: Some(GROUP_E2EE_MTI_SUITE_V2.to_owned()),
                require_fresh: Some(true),
            },
        )
        .await?;
    let now_text = format_time(OffsetDateTime::now_utc())?;
    let add_operation = operation_id("add");
    let submission = context.product.prepare_add(V2AddMemberInput {
        meta: control_meta(
            client,
            &context.device,
            &group_state_ref.group_did,
            &add_operation,
        ),
        group_state_ref,
        group_key_package: package.group_key_package,
        member_did_document: member_document,
        now: now_text,
        draft_extension_negotiated: true,
        pending_commit_id: format!("pending-{add_operation}"),
        request_id: format!("{add_operation}-prepare"),
    })?;
    submit_prepared_add_async(context, &submission, format!("{add_operation}-finalize")).await
}

fn submit_exact_remove(
    client: &crate::core::ImClient,
    context: &mut ProductionV2Context<'_>,
    group_state_ref: V2GroupStateRef,
    member_did: &str,
    member_device_id: &str,
    member_document: Value,
) -> crate::ImResult<()> {
    let now_text = format_time(OffsetDateTime::now_utc())?;
    let remove_operation = operation_id("remove");
    let submission = context.product.prepare_remove(V2RemoveMemberInput {
        meta: control_meta(
            client,
            &context.device,
            &group_state_ref.group_did,
            &remove_operation,
        ),
        group_state_ref,
        member_did: member_did.to_owned(),
        member_device_id: member_device_id.to_owned(),
        member_did_document: member_document,
        now: now_text,
        draft_extension_negotiated: true,
        pending_commit_id: format!("pending-{remove_operation}"),
        request_id: format!("{remove_operation}-prepare"),
    })?;
    let finalize_request_id = format!("{remove_operation}-finalize");
    for retry in 0_u64..=5 {
        match submit_prepared_remove(context, &submission, finalize_request_id.clone()) {
            Err(error) if retry < 5 && p6_remove_trigger_convergence_pending(&error) => {
                std::thread::sleep(std::time::Duration::from_millis((retry + 1) * 200));
            }
            outcome => return outcome,
        }
    }
    unreachable!("bounded P6 Remove retry loop always returns")
}

async fn submit_exact_remove_async(
    client: &crate::core::ImClient,
    context: &mut ProductionV2Context<'_>,
    group_state_ref: V2GroupStateRef,
    member_did: &str,
    member_device_id: &str,
    member_document: Value,
) -> crate::ImResult<()> {
    let now_text = format_time(OffsetDateTime::now_utc())?;
    let remove_operation = operation_id("remove");
    let submission = context.product.prepare_remove(V2RemoveMemberInput {
        meta: control_meta(
            client,
            &context.device,
            &group_state_ref.group_did,
            &remove_operation,
        ),
        group_state_ref,
        member_did: member_did.to_owned(),
        member_device_id: member_device_id.to_owned(),
        member_did_document: member_document,
        now: now_text,
        draft_extension_negotiated: true,
        pending_commit_id: format!("pending-{remove_operation}"),
        request_id: format!("{remove_operation}-prepare"),
    })?;
    let finalize_request_id = format!("{remove_operation}-finalize");
    for retry in 0_u64..=5 {
        match submit_prepared_remove_async(context, &submission, finalize_request_id.clone()).await
        {
            Err(error) if retry < 5 && p6_remove_trigger_convergence_pending(&error) => {
                tokio::time::sleep(std::time::Duration::from_millis((retry + 1) * 200)).await;
            }
            outcome => return outcome,
        }
    }
    unreachable!("bounded P6 Remove retry loop always returns")
}

fn submit_prepared_add(
    context: &mut ProductionV2Context<'_>,
    submission: &V2PreparedAddSubmission,
    finalize_request_id: String,
) -> crate::ImResult<()> {
    let pending_commit_id = submission.prepared.pending_commit_id.clone();
    match context.product.submit_add(submission, finalize_request_id) {
        Ok(_) => Ok(()),
        Err(error) if p6_host_rejection_is_deterministic(&error) => {
            context.product.abort_pending(
                pending_commit_id,
                format!("{}-abort", operation_id("add-rejected")),
            )?;
            Err(error)
        }
        Err(error) => Err(error),
    }
}

async fn submit_prepared_add_async(
    context: &mut ProductionV2Context<'_>,
    submission: &V2PreparedAddSubmission,
    finalize_request_id: String,
) -> crate::ImResult<()> {
    let pending_commit_id = submission.prepared.pending_commit_id.clone();
    match context
        .product
        .submit_add_async(submission, finalize_request_id)
        .await
    {
        Ok(_) => Ok(()),
        Err(error) if p6_host_rejection_is_deterministic(&error) => {
            context.product.abort_pending(
                pending_commit_id,
                format!("{}-abort", operation_id("add-rejected")),
            )?;
            Err(error)
        }
        Err(error) => Err(error),
    }
}

fn submit_prepared_remove(
    context: &mut ProductionV2Context<'_>,
    submission: &V2PreparedRemoveSubmission,
    finalize_request_id: String,
) -> crate::ImResult<()> {
    let pending_commit_id = submission.prepared.pending_commit_id.clone();
    match context
        .product
        .submit_remove(submission, finalize_request_id)
    {
        Ok(_) => Ok(()),
        Err(error) if p6_host_rejection_is_deterministic(&error) => {
            context.product.abort_pending(
                pending_commit_id,
                format!("{}-abort", operation_id("remove-rejected")),
            )?;
            Err(error)
        }
        Err(error) => Err(error),
    }
}

async fn submit_prepared_remove_async(
    context: &mut ProductionV2Context<'_>,
    submission: &V2PreparedRemoveSubmission,
    finalize_request_id: String,
) -> crate::ImResult<()> {
    let pending_commit_id = submission.prepared.pending_commit_id.clone();
    match context
        .product
        .submit_remove_async(submission, finalize_request_id)
        .await
    {
        Ok(_) => Ok(()),
        Err(error) if p6_host_rejection_is_deterministic(&error) => {
            context.product.abort_pending(
                pending_commit_id,
                format!("{}-abort", operation_id("remove-rejected")),
            )?;
            Err(error)
        }
        Err(error) => Err(error),
    }
}

fn p6_host_rejection_is_deterministic(error: &crate::ImError) -> bool {
    matches!(
        error,
        crate::ImError::Service { code: Some(code), .. }
            if matches!(
                code.trim().to_ascii_lowercase().as_str(),
                "anp.invalid_params_shape"
                    | "anp.invalid_target_binding"
                    | "group.state_version_conflict"
                    | "group.e2ee.epoch_conflict"
                    | "group.e2ee.key_package_consumed"
                    | "group.member_conflict"
                    | "group.not_member"
                    | "group.policy_violation"
            )
    )
}

fn p6_remove_trigger_convergence_pending(error: &crate::ImError) -> bool {
    matches!(
        error,
        crate::ImError::Service {
            code: Some(code),
            ..
        } if code.trim().eq_ignore_ascii_case("anp.forbidden")
    )
}

fn previous_epoch(epoch: &str) -> crate::ImResult<String> {
    epoch
        .parse::<u64>()
        .ok()
        .and_then(|value| value.checked_sub(1))
        .map(|value| value.to_string())
        .ok_or_else(|| crate::ImError::LocalStateUnavailable {
            detail: "prepared P6 membership body has invalid epoch".to_owned(),
        })
}

fn validate_embedded_group_state_refs(
    expected_group_did: &str,
    response: &Value,
) -> crate::ImResult<()> {
    let mut versions = BTreeSet::new();
    if let Some(version) = response.get("group_state_version") {
        versions.insert(required_state_value(version)?);
    }
    let mut policy_hashes = BTreeSet::new();
    if let Some(hash) = response.get("policy_hash") {
        policy_hashes.insert(required_state_value(hash)?);
    }
    for pointer in [
        "/group_state_ref",
        "/group/group_state_ref",
        "/group_snapshot/group_state_ref",
    ] {
        let Some(reference) = response.pointer(pointer) else {
            continue;
        };
        if reference.get("group_did").and_then(Value::as_str) != Some(expected_group_did) {
            return Err(crate::ImError::PermissionDenied);
        }
        let version = reference
            .get("group_state_version")
            .ok_or(crate::ImError::PermissionDenied)?;
        versions.insert(required_state_value(version)?);
        if let Some(hash) = reference.get("policy_hash") {
            policy_hashes.insert(required_state_value(hash)?);
        }
    }
    if versions.len() > 1 || policy_hashes.len() > 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn required_state_value(value: &Value) -> crate::ImResult<String> {
    match value {
        Value::String(value) if !value.trim().is_empty() => Ok(value.clone()),
        Value::Number(value) => Ok(value.to_string()),
        _ => Err(crate::ImError::PermissionDenied),
    }
}

fn validated_p6_manifest(member_did: &str, document: &Value) -> crate::ImResult<DeviceManifest> {
    validate_member_document_id(member_did, document)?;
    anp::authentication::validate_device_manifest(document)
        .map_err(|_| crate::ImError::PermissionDenied)?
        .ok_or(crate::ImError::PermissionDenied)
}

fn validate_member_document_id(member_did: &str, document: &Value) -> crate::ImResult<()> {
    if document.get("id").and_then(Value::as_str) != Some(member_did)
        || (member_did.starts_with("did:wba:")
            && !anp::authentication::validate_did_document_binding(document, true))
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn eligible_device_ids(manifest: &DeviceManifest) -> Vec<String> {
    manifest
        .devices
        .iter()
        .filter(|device| {
            device
                .profiles
                .iter()
                .any(|profile| profile == PROFILE_GROUP_E2EE_V2)
        })
        .map(|device| device.device_id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

type V2EndpointKey = (String, String);

fn endpoint_set_for_member(
    member_did: &str,
    endpoints: &[V2LocalGroupMemberEndpoint],
) -> BTreeSet<V2EndpointKey> {
    endpoints
        .iter()
        .filter(|endpoint| endpoint.member_did == member_did)
        .map(|endpoint| {
            (
                endpoint.member_did.clone(),
                endpoint.member_device_id.clone(),
            )
        })
        .collect()
}

fn roster_delta(
    desired: &BTreeSet<V2EndpointKey>,
    observed: &BTreeSet<V2EndpointKey>,
) -> (Vec<V2EndpointKey>, Vec<V2EndpointKey>) {
    (
        observed.difference(desired).cloned().collect(),
        desired.difference(observed).cloned().collect(),
    )
}

fn local_group_can_reconcile_roster(readiness: &V2LocalGroupReadiness) -> bool {
    *readiness == V2LocalGroupReadiness::Active
}

fn member_device_ids(member_did: &str, endpoints: &[V2LocalGroupMemberEndpoint]) -> Vec<String> {
    endpoints
        .iter()
        .filter(|endpoint| endpoint.member_did == member_did)
        .map(|endpoint| endpoint.member_device_id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn require_current_controller_endpoint(
    endpoints: &[V2LocalGroupMemberEndpoint],
    current_did: &str,
    current_device_id: &str,
) -> crate::ImResult<()> {
    if endpoints
        .iter()
        .filter(|endpoint| {
            endpoint.member_did == current_did && endpoint.member_device_id == current_device_id
        })
        .count()
        == 1
    {
        Ok(())
    } else {
        Err(crate::ImError::PermissionDenied)
    }
}

fn endpoint_inventory_input(
    client: &crate::core::ImClient,
    device: &CurrentV2Device,
    group_did: &str,
    kind: &str,
) -> V2InspectLocalGroupInput {
    V2InspectLocalGroupInput {
        owner_did: client.did().as_str().to_owned(),
        owner_device_id: device.device_id.clone(),
        group_did: group_did.to_owned(),
        request_id: format!("{}-inventory", operation_id(kind)),
    }
}

fn require_exact_group(reference: &V2GroupStateRef) -> crate::ImResult<()> {
    if reference.group_did.starts_with("did:") && !reference.group_state_version.is_empty() {
        Ok(())
    } else {
        Err(crate::ImError::PermissionDenied)
    }
}

pub(crate) fn current_v2_device(
    client: &crate::core::ImClient,
) -> crate::ImResult<CurrentV2Device> {
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
        anp_version: None,
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

fn control_meta(
    client: &crate::core::ImClient,
    device: &CurrentV2Device,
    group_did: &str,
    operation_id: &str,
) -> V2GroupControlMetadata {
    V2GroupControlMetadata {
        anp_version: None,
        profile: GROUP_E2EE_PROFILE_V2.to_owned(),
        security_profile: GROUP_E2EE_SECURITY_PROFILE_V2.to_owned(),
        sender_did: client.did().as_str().to_owned(),
        sender_device_id: device.device_id.clone(),
        target: V2Target {
            kind: "group".to_owned(),
            did: group_did.to_owned(),
        },
        operation_id: operation_id.to_owned(),
        // `created_at` is optional in P6 v2 and deliberately omitted for
        // membership commits. The SDK WAL persists the prepared body and
        // operation_id, so omitting a volatile timestamp lets an uncertain
        // Host submission be replayed byte-for-byte after restart.
        created_at: None,
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
