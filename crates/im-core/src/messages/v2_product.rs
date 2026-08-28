//! Public message-service adapters for the P5/P6 v2 product runtimes.
//!
//! The protocol-specific runtimes own encryption and exact-device state. This
//! adapter keeps the public SDK contract at one logical message and persists
//! only the secret-free local projection. Per-invocation delivery counts are
//! AWiki product metadata and never enter the ANP Direct wire object.

use serde_json::Value;

use super::service::ResolvedSendRequest;

#[cfg(feature = "sqlite")]
pub(super) fn direct_enabled_for_client(client: &crate::core::ImClient) -> crate::ImResult<bool> {
    Ok(client.core_inner().direct_e2ee_v2_enabled()
        && crate::internal::secure_direct::v2_product::local_identity_uses_vnext(client)?)
}

#[cfg(not(feature = "sqlite"))]
pub(super) fn direct_enabled_for_client(_client: &crate::core::ImClient) -> crate::ImResult<bool> {
    Ok(false)
}

#[cfg(feature = "sqlite")]
pub(super) async fn send_direct_async(
    client: &crate::core::ImClient,
    mut resolved: ResolvedSendRequest,
) -> crate::ImResult<crate::messages::SendMessageResult> {
    normalize_message_identity(&mut resolved.request)?;
    let target_did = resolved_direct_did(&resolved)?;
    let target_handle = resolved.direct_handle().map(str::to_owned);
    let peer_scope = resolved.peer_scope.clone();
    let conversation_id = peer_scope
        .as_ref()
        .map(crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope);
    let logical_message_id = required_message_id(&resolved.request)?.to_owned();
    let core = client.core_handle();

    let (summary, attachment_projection) = if matches!(
        resolved.request.body,
        crate::messages::MessageBody::Attachment { .. }
    ) {
        let attachment =
            super::service::attachment_request_from_message_request(resolved.request.clone())?;
        let sent = crate::internal::secure_direct::v2_product::send_attachment_for_client(
            &core,
            client,
            true,
            crate::internal::secure_direct::v2_product::V2AttachmentProductSendInput {
                logical_message_id: logical_message_id.clone(),
                target_did: target_did.clone(),
                conversation_id: conversation_id.clone(),
                object_target: resolved.request.target.clone(),
                request: attachment,
            },
        )
        .await?;
        (sent.direct, Some(sent.redacted_manifest))
    } else {
        let sent = crate::internal::secure_direct::v2_product::send_for_client(
            &core,
            client,
            true,
            crate::internal::secure_direct::v2_product::V2DirectProductSendInput {
                logical_message_id: logical_message_id.clone(),
                target_did: target_did.clone(),
                conversation_id,
                body:
                    crate::internal::secure_direct::v2_product::V2OrdinaryBody::from_message_body(
                        &resolved.request.body,
                    )?,
            },
        )
        .await?;
        (sent, None)
    };

    let mut result = direct_result(client, &resolved, &summary, attachment_projection.as_ref())?;
    match persist_direct_projection_async(
        client,
        &resolved,
        &target_did,
        target_handle.as_deref(),
        peer_scope.as_ref(),
        attachment_projection.as_ref(),
        &result,
    )
    .await
    {
        Ok(()) => client.emit_committed_local_message_projection("local_send"),
        Err(err) => result
            .warnings
            .push(format!("Failed to persist local P5 v2 message: {err}")),
    }
    Ok(result)
}

#[cfg(feature = "sqlite")]
pub(super) fn send_direct(
    client: &crate::core::ImClient,
    resolved: ResolvedSendRequest,
) -> crate::ImResult<crate::messages::SendMessageResult> {
    if tokio::runtime::Handle::try_current().is_ok() {
        return Err(crate::ImError::unsupported(
            "sync-p5-v2-send-inside-async-runtime",
        ));
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| crate::ImError::Internal {
            message: format!("create P5 v2 sync runtime: {err}"),
        })?;
    runtime.block_on(send_direct_async(client, resolved))
}

#[cfg(not(feature = "sqlite"))]
pub(super) fn send_direct(
    _client: &crate::core::ImClient,
    _resolved: ResolvedSendRequest,
) -> crate::ImResult<crate::messages::SendMessageResult> {
    Err(crate::ImError::unsupported("secure-direct"))
}

#[cfg(not(feature = "sqlite"))]
pub(super) async fn send_direct_async(
    _client: &crate::core::ImClient,
    _resolved: ResolvedSendRequest,
) -> crate::ImResult<crate::messages::SendMessageResult> {
    Err(crate::ImError::unsupported("secure-direct"))
}

fn normalize_message_identity(
    request: &mut crate::messages::SendMessageRequest,
) -> crate::ImResult<()> {
    if request.client_message_id.is_none() {
        request.client_message_id = Some(crate::ids::MessageId::parse(format!(
            "msg-{}",
            crate::internal::wire::common::generate_operation_id()
        ))?);
    }
    if request
        .delivery
        .idempotency_key
        .as_deref()
        .is_none_or(|value| value.trim().is_empty())
    {
        request.delivery.idempotency_key = Some(format!(
            "op-{}",
            request
                .client_message_id
                .as_ref()
                .expect("message id was assigned")
                .as_str()
        ));
    }
    Ok(())
}

fn required_message_id(request: &crate::messages::SendMessageRequest) -> crate::ImResult<&str> {
    request
        .client_message_id
        .as_ref()
        .map(crate::ids::MessageId::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            crate::ImError::invalid_input(
                Some("client_message_id".to_owned()),
                "logical P5/P6 message id is required",
            )
        })
}

fn resolved_direct_did(resolved: &ResolvedSendRequest) -> crate::ImResult<String> {
    let value = resolved
        .target_did
        .as_deref()
        .or_else(|| match &resolved.request.target {
            crate::messages::MessageTarget::Direct(peer) => Some(peer.as_str()),
            crate::messages::MessageTarget::Group(_) => None,
        })
        .map(str::trim)
        .filter(|value| value.starts_with("did:"))
        .ok_or_else(|| {
            crate::ImError::invalid_input(
                Some("target".to_owned()),
                "P5 v2 direct target must resolve to a DID",
            )
        })?;
    Ok(value.to_owned())
}

#[cfg(feature = "sqlite")]
fn direct_result(
    client: &crate::core::ImClient,
    resolved: &ResolvedSendRequest,
    summary: &crate::internal::secure_direct::v2_product::V2DirectProductSendSummary,
    attachment_projection: Option<&Value>,
) -> crate::ImResult<crate::messages::SendMessageResult> {
    let fully_accepted = summary.fully_accepted();
    let delivery = product_delivery(
        fully_accepted,
        summary.accepted_device_count,
        summary.failed_device_count,
    );
    let projection_body = attachment_projection
        .map(|payload| crate::messages::MessageBody::Payload {
            payload: payload.clone(),
        })
        .unwrap_or_else(|| resolved.request.body.clone());
    let mut result = crate::internal::message_runtime::local_projection::send_projection_result(
        client,
        &resolved.request.target,
        &projection_body,
        resolved
            .request
            .client_message_id
            .as_ref()
            .ok_or(crate::ImError::PermissionDenied)?,
        resolved.request.delivery.idempotency_key.as_deref(),
        delivery,
        Some(summary.target_did.as_str()),
        resolved.direct_handle(),
        resolved.peer_scope.as_ref(),
    )?;
    if let Some(manifest) = attachment_projection {
        let content_type =
            crate::attachments::manifest::attachment_manifest_content_type().to_owned();
        result.message.body = crate::messages::MessageBodyView::Unsupported {
            content_type: Some(content_type.clone()),
        };
        result.message.metadata.content_type = Some(content_type);
        result.message.metadata.retry_plan = None;
        add_product_attribute(
            &mut result,
            "attachment_manifest",
            &crate::attachments::manifest::manifest_content_string(manifest),
        );
    }
    result.message.sent_at = summary.accepted_at.clone();
    add_product_attribute(&mut result, "security", "direct-e2ee");
    add_product_attribute(&mut result, "message_security_profile", "direct-e2ee");
    add_product_attribute(&mut result, "e2ee_profile", "anp.direct.e2ee.v2");
    add_product_attribute(
        &mut result,
        "final_acceptance",
        if fully_accepted { "true" } else { "false" },
    );
    add_product_attribute(
        &mut result,
        "target_device_count",
        &summary.target_device_count.to_string(),
    );
    add_product_attribute(
        &mut result,
        "own_sync_device_count",
        &summary.own_sync_device_count.to_string(),
    );
    add_product_attribute(
        &mut result,
        "attempted_device_count",
        &summary.attempted_device_count.to_string(),
    );
    add_product_attribute(
        &mut result,
        "previously_accepted_device_count",
        &summary.previously_accepted_device_count.to_string(),
    );
    add_product_attribute(
        &mut result,
        "newly_accepted_device_count",
        &summary.newly_accepted_device_count.to_string(),
    );
    add_product_attribute(
        &mut result,
        "accepted_device_count",
        &summary.accepted_device_count.to_string(),
    );
    add_product_attribute(
        &mut result,
        "failed_device_count",
        &summary.failed_device_count.to_string(),
    );
    if summary.failed_device_count > 0 {
        result.warnings.push(format!(
            "P5 v2 accepted {} device delivery/deliveries and retained {} failed delivery/deliveries for retry",
            summary.accepted_device_count, summary.failed_device_count
        ));
    }
    Ok(result)
}

fn product_delivery(
    fully_accepted: bool,
    accepted_count: usize,
    failed_count: usize,
) -> crate::messages::DeliveryState {
    if fully_accepted {
        crate::messages::DeliveryState::Accepted
    } else if accepted_count > 0 {
        crate::messages::DeliveryState::Sent
    } else {
        crate::messages::DeliveryState::Failed {
            reason: format!("no device delivery accepted ({failed_count} failed)"),
        }
    }
}

fn add_product_attribute(result: &mut crate::messages::SendMessageResult, key: &str, value: &str) {
    result
        .message
        .metadata
        .attributes
        .push(crate::messages::MessageMetadataAttribute {
            key: key.to_owned(),
            value: value.to_owned(),
        });
}

#[cfg(feature = "sqlite")]
async fn persist_direct_projection_async(
    client: &crate::core::ImClient,
    resolved: &ResolvedSendRequest,
    target_did: &str,
    target_handle: Option<&str>,
    peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
    attachment_projection: Option<&Value>,
    result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    if let Some(manifest) = attachment_projection {
        return crate::internal::message_runtime::local_projection::persist_direct_e2ee_attachment_outgoing_async(
            client,
            target_did,
            target_handle,
            peer_scope,
            manifest,
            result,
        )
        .await;
    }
    match &resolved.request.body {
        crate::messages::MessageBody::Text { text, kind } => {
            crate::internal::message_runtime::local_projection::persist_direct_e2ee_outgoing_async(
                client,
                target_did,
                target_handle,
                peer_scope,
                text,
                kind,
                result,
            )
            .await
        }
        crate::messages::MessageBody::Payload { payload } => {
            crate::internal::message_runtime::local_projection::persist_direct_e2ee_payload_outgoing_async(
                client,
                target_did,
                target_handle,
                peer_scope,
                payload,
                result,
            )
            .await
        }
        crate::messages::MessageBody::Attachment { .. } => {
            Err(crate::ImError::PermissionDenied)
        }
    }
}

#[cfg(feature = "group-e2ee")]
async fn send_group_async_impl(
    client: &crate::core::ImClient,
    mut request: crate::messages::SendMessageRequest,
) -> crate::ImResult<crate::messages::SendMessageResult> {
    normalize_message_identity(&mut request)?;
    let group = match &request.target {
        crate::messages::MessageTarget::Group(group) => group.clone(),
        crate::messages::MessageTarget::Direct(_) => {
            return Err(crate::ImError::unsupported("direct-send"));
        }
    };
    let group_did = group.as_str().trim();
    if !group_did.starts_with("did:") {
        return Err(crate::ImError::invalid_input(
            Some("group".to_owned()),
            "P6 v2 group target must be a DID",
        ));
    }
    // P4 membership may have advanced independently (for example, a member
    // called group.leave). Before encrypting at the current MLS epoch, let the
    // active owner converge its accepted local tree to the authoritative P4/P2
    // roster. Non-owners and already-converged owners are no-ops.
    crate::internal::group_e2ee::v2_lifecycle::reconcile_group_device_roster_async(
        client,
        group.clone(),
    )
    .await?;
    let logical_message_id = required_message_id(&request)?.to_owned();
    let operation_id = request
        .delivery
        .idempotency_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(crate::ImError::PermissionDenied)?
        .to_owned();
    let created_at = crate::internal::wire::common::now_rfc3339();
    let device = current_vnext_device(client)?;
    let did_document = client
        .runtime()
        .key_provider
        .optional_did_document()?
        .ok_or_else(|| crate::ImError::LocalStateUnavailable {
            detail: "P6 v2 requires the current DID Document".to_owned(),
        })?;
    let group_state_ref =
        fetch_current_group_state_ref_async(client, group_did, &operation_id).await?;

    let runtime = crate::internal::group_e2ee::v2_runtime::runtime_for_client(client)?;
    let signer = client
        .runtime()
        .key_provider
        .async_session()
        .map(crate::internal::proof::origin::OriginProofSigner::Provider)
        .unwrap_or_else(|| {
            crate::internal::proof::origin::OriginProofSigner::Identity(std::sync::Arc::clone(
                &client.runtime().key_provider,
            ))
        });
    let proof_identity = crate::internal::proof::origin::OriginProofIdentity {
        identity_name: client.current_identity().id.as_str().to_owned(),
        did_document: Some(did_document.clone()),
        signer,
        verification_method: Some(device.signing_key_id),
    };
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    let p6_client_instance_id =
        crate::internal::local_state::sync_v2::load_or_create_sync_client_instance_id(
            &connection,
            client.current_identity().id.as_str(),
        )?;
    let host = crate::internal::group_e2ee::v2_product::RpcGroupE2eeV2Host::new(
        crate::internal::transport::CoreHttpTransport::new(client),
        proof_identity,
    )
    .with_p6_client_instance_id(p6_client_instance_id);
    let mut product =
        crate::internal::group_e2ee::v2_product::GroupE2eeV2Product::new(runtime, host);
    let meta = anp::group_e2ee::V2GroupSendMetadata {
        anp_version: None,
        profile: anp::group_e2ee::GROUP_E2EE_PROFILE_V2.to_owned(),
        security_profile: anp::group_e2ee::GROUP_E2EE_SECURITY_PROFILE_V2.to_owned(),
        sender_did: client.did().as_str().to_owned(),
        sender_device_id: device.device_id,
        target: anp::group_e2ee::V2Target {
            kind: "group".to_owned(),
            did: group_did.to_owned(),
        },
        operation_id: operation_id.clone(),
        message_id: logical_message_id.clone(),
        content_type: anp::group_e2ee::GROUP_CIPHER_CONTENT_TYPE_V2.to_owned(),
        created_at: Some(created_at.clone()),
    };
    let (application, attachment_projection) = match &request.body {
        crate::messages::MessageBody::Text { text, kind } => (
            crate::internal::group_e2ee::v2_application::V2ProductApplication::text(
                group_did,
                match kind {
                    crate::messages::MessageKind::Text => "text/plain",
                    crate::messages::MessageKind::Markdown => "text/markdown",
                },
                text.clone(),
            )?,
            None,
        ),
        crate::messages::MessageBody::Payload { payload } => (
            crate::internal::group_e2ee::v2_application::V2ProductApplication::json(
                group_did,
                payload.clone(),
            )?,
            None,
        ),
        crate::messages::MessageBody::Attachment { .. } => {
            let attachment =
                super::service::attachment_request_from_message_request(request.clone())?;
            let committed =
                crate::internal::attachment_runtime::upload::AttachmentUploadRuntime::new(
                    client,
                    crate::internal::auth::session::FileSessionProvider::new(client),
                    crate::internal::transport::CoreHttpTransport::new(client),
                )
                .prepare_and_commit_object_async(
                    crate::internal::attachment_runtime::upload::AttachmentPrepareObjectInput {
                        target: request.target.clone(),
                        request: attachment,
                        resolved_target_did: None,
                        message_security_profile: "group-e2ee",
                    },
                )
                .await?;
            let redacted = committed.redacted_manifest.clone();
            (
                crate::internal::group_e2ee::v2_application::V2ProductApplication::committed_attachment(
                    group_did,
                    &committed,
                )?,
                Some(redacted),
            )
        }
    };
    let prepared = product.prepare_product_application_send(
        meta,
        group_state_ref,
        application,
        did_document,
        created_at.clone(),
        true,
        format!("p6-v2-encrypt-{operation_id}"),
    )?;
    let accepted = product
        .submit_product_application_send_async(&prepared)
        .await?;
    let mut result = group_result(client, &request, &accepted, attachment_projection.as_ref())?;
    match persist_group_projection_async(
        client,
        group_did,
        &request,
        attachment_projection.as_ref(),
        &result,
    )
    .await
    {
        Ok(()) => client.emit_committed_local_message_projection("local_send"),
        Err(err) => result
            .warnings
            .push(format!("Failed to persist local P6 v2 message: {err}")),
    }
    Ok(result)
}

#[cfg(feature = "group-e2ee")]
pub(super) fn send_group(
    client: &crate::core::ImClient,
    request: crate::messages::SendMessageRequest,
) -> crate::ImResult<crate::messages::SendMessageResult> {
    if tokio::runtime::Handle::try_current().is_ok() {
        return Err(crate::ImError::unsupported(
            "sync-p6-v2-send-inside-async-runtime",
        ));
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| crate::ImError::Internal {
            message: format!("create P6 v2 sync runtime: {err}"),
        })?;
    runtime.block_on(send_group_async_impl(client, request))
}

#[cfg(not(feature = "group-e2ee"))]
pub(super) fn send_group(
    _client: &crate::core::ImClient,
    _request: crate::messages::SendMessageRequest,
) -> crate::ImResult<crate::messages::SendMessageResult> {
    Err(crate::ImError::unsupported("group-e2ee"))
}

pub(super) async fn send_group_async(
    client: &crate::core::ImClient,
    request: crate::messages::SendMessageRequest,
) -> crate::ImResult<crate::messages::SendMessageResult> {
    #[cfg(feature = "group-e2ee")]
    {
        send_group_async_impl(client, request).await
    }
    #[cfg(not(feature = "group-e2ee"))]
    {
        let _ = (client, request);
        Err(crate::ImError::unsupported("group-e2ee"))
    }
}

#[cfg(feature = "group-e2ee")]
struct CurrentVNextDevice {
    device_id: String,
    signing_key_id: String,
}

#[cfg(feature = "group-e2ee")]
fn current_vnext_device(client: &crate::core::ImClient) -> crate::ImResult<CurrentVNextDevice> {
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
    Ok(CurrentVNextDevice {
        device_id: authorization.protocol_device_id.as_str().to_owned(),
        signing_key_id: authorization.signing_key_id.clone(),
    })
}

#[cfg(feature = "group-e2ee")]
async fn fetch_current_group_state_ref_async(
    client: &crate::core::ImClient,
    group_did: &str,
    operation_id: &str,
) -> crate::ImResult<anp::group_e2ee::V2GroupStateRef> {
    let snapshot_params =
        crate::internal::wire::group::build_group_get_rpc_params(client.did().as_str(), group_did)?;
    let mut transport = crate::internal::transport::CoreHttpTransport::new(client);
    let snapshot = crate::internal::transport::AsyncAuthenticatedRpcTransport::authenticated_rpc(
        &mut transport,
        crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT,
        "group.get",
        snapshot_params,
    )
    .await?;
    ensure_group_e2ee_v2_send_not_paused(&snapshot)?;

    let params = crate::internal::wire::group::build_group_get_info_rpc_params(
        client.did().as_str(),
        group_did,
        operation_id,
        false,
    )?;
    let raw = crate::internal::transport::AsyncAuthenticatedRpcTransport::authenticated_rpc(
        &mut transport,
        crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT,
        "group.get_info",
        params,
    )
    .await?;
    let returned_group = raw
        .get("group_did")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| crate::ImError::Serialization {
            detail: "P4 group.get_info result is missing group_did".to_owned(),
        })?;
    if returned_group != group_did {
        return Err(crate::ImError::PermissionDenied);
    }
    if !raw.get("group_profile").is_some_and(Value::is_object) {
        return Err(crate::ImError::Serialization {
            detail: "P4 group.get_info result is missing group_profile".to_owned(),
        });
    }
    let version = raw
        .get("group_state_version")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| crate::ImError::Serialization {
            detail: "P4 group.get_info result is missing group_state_version".to_owned(),
        })?;
    Ok(anp::group_e2ee::V2GroupStateRef {
        group_did: group_did.to_owned(),
        group_state_version: version.to_owned(),
        policy_hash: None,
        roster_hash: None,
    })
}

#[cfg(feature = "group-e2ee")]
fn ensure_group_e2ee_v2_send_not_paused(snapshot: &Value) -> crate::ImResult<()> {
    if snapshot
        .pointer("/e2ee_maintenance/send_paused")
        .and_then(Value::as_bool)
        != Some(true)
    {
        return Ok(());
    }
    Err(crate::ImError::Service {
        status_code: None,
        code: Some("group.e2ee.state_not_ready".to_owned()),
        message: "P6 v2 send is paused while Group membership convergence is pending".to_owned(),
        data: None,
    })
}

#[cfg(feature = "group-e2ee")]
fn group_result(
    client: &crate::core::ImClient,
    request: &crate::messages::SendMessageRequest,
    accepted: &anp::group_e2ee::V2GroupSendResult,
    attachment_projection: Option<&Value>,
) -> crate::ImResult<crate::messages::SendMessageResult> {
    let projection_body = attachment_projection
        .map(|payload| crate::messages::MessageBody::Payload {
            payload: payload.clone(),
        })
        .unwrap_or_else(|| request.body.clone());
    let mut result = crate::internal::message_runtime::local_projection::send_projection_result(
        client,
        &request.target,
        &projection_body,
        request
            .client_message_id
            .as_ref()
            .ok_or(crate::ImError::PermissionDenied)?,
        Some(accepted.operation_id.as_str()),
        crate::messages::DeliveryState::Accepted,
        None,
        None,
        None,
    )?;
    if let Some(manifest) = attachment_projection {
        let content_type =
            crate::attachments::manifest::attachment_manifest_content_type().to_owned();
        result.message.body = crate::messages::MessageBodyView::Unsupported {
            content_type: Some(content_type.clone()),
        };
        result.message.metadata.content_type = Some(content_type);
        result.message.metadata.retry_plan = None;
        add_product_attribute(
            &mut result,
            "attachment_manifest",
            &crate::attachments::manifest::manifest_content_string(manifest),
        );
    }
    result.message.sent_at = Some(accepted.accepted_at.clone());
    add_product_attribute(&mut result, "security", "group-e2ee");
    add_product_attribute(&mut result, "message_security_profile", "group-e2ee");
    add_product_attribute(&mut result, "e2ee_profile", "anp.group.e2ee.v2");
    add_product_attribute(&mut result, "final_acceptance", "true");
    add_product_attribute(
        &mut result,
        "group_state_version",
        accepted.group_state_version.as_str(),
    );
    add_product_attribute(
        &mut result,
        "group_event_seq",
        accepted.group_event_seq.as_str(),
    );
    add_product_attribute(&mut result, "mls_epoch", accepted.epoch.as_str());
    add_product_attribute(&mut result, "mls_ciphertext_count", "1");
    Ok(result)
}

#[cfg(feature = "group-e2ee")]
async fn persist_group_projection_async(
    client: &crate::core::ImClient,
    group_did: &str,
    request: &crate::messages::SendMessageRequest,
    attachment_projection: Option<&Value>,
    result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    if let Some(manifest) = attachment_projection {
        return crate::internal::message_runtime::local_projection::persist_group_e2ee_attachment_outgoing_async(
            client, group_did, manifest, result,
        )
        .await;
    }
    match &request.body {
        crate::messages::MessageBody::Text { text, kind } => {
            crate::internal::message_runtime::local_projection::persist_group_e2ee_outgoing_async(
                client, group_did, text, kind, result,
            )
            .await
        }
        crate::messages::MessageBody::Payload { payload } => {
            crate::internal::message_runtime::local_projection::persist_group_e2ee_payload_outgoing_async(
                client, group_did, payload, result,
            )
            .await
        }
        crate::messages::MessageBody::Attachment { .. } => Err(crate::ImError::PermissionDenied),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "sqlite")]
    fn projection_client(root: &tempfile::TempDir) -> crate::core::ImClient {
        let identity_root = root.path().join("identities");
        let identity_dir = identity_root.join("alice");
        std::fs::create_dir_all(&identity_dir).unwrap();
        std::fs::create_dir_all(root.path().join("local")).unwrap();
        std::fs::write(identity_root.join("default"), "alice\n").unwrap();
        std::fs::write(
            identity_root.join("registry.json"),
            serde_json::json!({
                "default_identity": "alice",
                "identities": [{
                    "id": "alice-id",
                    "did": "did:example:alice",
                    "local_alias": "alice",
                    "ready_for_auth": true,
                    "ready_for_messaging": true,
                    "missing": []
                }]
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(identity_dir.join("did.json"), "{}").unwrap();
        crate::core::ImCore::new(
            crate::ImCoreConfig {
                service_base_url: crate::ServiceEndpoint::parse("https://example.test").unwrap(),
                did_domain: "example.test".to_owned(),
                client_version_info: None,
                user_service_endpoint: None,
                message_service_endpoint: None,
                mail_service_endpoint: None,
                anp_service_endpoint: None,
                anp_service_did: None,
                ca_bundle: None,
                transport_policy: crate::MessageTransportPolicy::HttpOnly,
            },
            crate::ImCorePaths {
                identities: crate::IdentityRegistryPaths {
                    identity_root_dir: identity_root.clone(),
                    registry_path: identity_root.join("registry.json"),
                    default_identity_path: Some(identity_root.join("default")),
                },
                local_state: crate::LocalStatePaths {
                    sqlite_path: root.path().join("local").join("im.sqlite"),
                },
                runtime: crate::RuntimePaths {
                    cache_dir: root.path().join("cache"),
                    temp_dir: root.path().join("tmp"),
                },
            },
        )
        .unwrap()
        .client(crate::identity::IdentitySelector::LocalAlias(
            "alice".to_owned(),
        ))
        .unwrap()
    }

    #[test]
    fn product_delivery_preserves_partial_device_acceptance() {
        assert_eq!(
            product_delivery(true, 3, 0),
            crate::messages::DeliveryState::Accepted
        );
        assert_eq!(
            product_delivery(false, 1, 2),
            crate::messages::DeliveryState::Sent
        );
        assert!(matches!(
            product_delivery(false, 0, 2),
            crate::messages::DeliveryState::Failed { .. }
        ));
    }

    #[test]
    #[cfg(feature = "sqlite")]
    fn direct_result_projects_aggregate_final_acceptance() {
        let root = tempfile::tempdir().unwrap();
        let client = projection_client(&root);
        let resolved = ResolvedSendRequest {
            request: crate::messages::SendMessageRequest {
                target: crate::messages::MessageTarget::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                body: crate::messages::MessageBody::Text {
                    text: "hello".to_owned(),
                    kind: crate::messages::MessageKind::Text,
                },
                security: crate::messages::MessageSecurityMode::SecureDirect,
                client_message_id: Some(crate::ids::MessageId::parse("msg-final").unwrap()),
                delivery: crate::messages::MessageDeliveryOptions {
                    idempotency_key: Some("op-final".to_owned()),
                    wait_for_final_acceptance: false,
                },
                delegated_signing: None,
            },
            target_did: Some("did:example:bob".to_owned()),
            peer_scope: None,
        };
        let summary = |accepted_device_count, failed_device_count| {
            crate::internal::secure_direct::v2_product::V2DirectProductSendSummary {
                logical_message_id: "msg-final".to_owned(),
                target_did: "did:example:bob".to_owned(),
                target_device_count: 2,
                own_sync_device_count: 0,
                attempted_device_count: 2,
                previously_accepted_device_count: 0,
                newly_accepted_device_count: accepted_device_count,
                accepted_device_count,
                failed_device_count,
                accepted_at: Some("2026-07-20T00:00:00Z".to_owned()),
            }
        };
        let attribute = |result: &crate::messages::SendMessageResult, key: &str| {
            result
                .message
                .metadata
                .attributes
                .iter()
                .find(|attribute| attribute.key == key)
                .map(|attribute| attribute.value.clone())
        };

        let partial = direct_result(&client, &resolved, &summary(1, 1), None).unwrap();
        assert_eq!(partial.delivery, crate::messages::DeliveryState::Sent);
        assert_eq!(
            attribute(&partial, "final_acceptance").as_deref(),
            Some("false")
        );

        let complete = direct_result(&client, &resolved, &summary(2, 0), None).unwrap();
        assert_eq!(complete.delivery, crate::messages::DeliveryState::Accepted);
        assert_eq!(
            attribute(&complete, "final_acceptance").as_deref(),
            Some("true")
        );

        let manifest = serde_json::json!({
            "type": "awiki.attachment.manifest.v1",
            "attachments": [{
                "attachment_id": "attachment-redacted",
                "object_uri": "https://objects.example/attachment-redacted"
            }]
        });
        let attachment =
            direct_result(&client, &resolved, &summary(2, 0), Some(&manifest)).unwrap();
        assert_eq!(
            attribute(&attachment, "security").as_deref(),
            Some("direct-e2ee")
        );
        assert_eq!(
            attribute(&attachment, "message_security_profile").as_deref(),
            Some("direct-e2ee")
        );
        let stored_manifest = attribute(&attachment, "attachment_manifest").unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&stored_manifest).unwrap(),
            manifest
        );
        assert!(!stored_manifest.contains("object_key"));
    }

    #[test]
    #[cfg(feature = "group-e2ee")]
    fn group_result_projects_typed_host_acceptance_as_final() {
        let root = tempfile::tempdir().unwrap();
        let client = projection_client(&root);
        let request = crate::messages::SendMessageRequest {
            target: crate::messages::MessageTarget::Group(
                crate::ids::GroupRef::parse("did:example:group").unwrap(),
            ),
            body: crate::messages::MessageBody::Text {
                text: "hello group".to_owned(),
                kind: crate::messages::MessageKind::Text,
            },
            security: crate::messages::MessageSecurityMode::GroupE2ee,
            client_message_id: Some(crate::ids::MessageId::parse("msg-group-final").unwrap()),
            delivery: crate::messages::MessageDeliveryOptions {
                idempotency_key: Some("op-group-final".to_owned()),
                wait_for_final_acceptance: true,
            },
            delegated_signing: None,
        };
        let accepted = anp::group_e2ee::V2GroupSendResult {
            accepted: true,
            group_did: "did:example:group".to_owned(),
            message_id: "msg-group-final".to_owned(),
            operation_id: "op-group-final".to_owned(),
            group_event_seq: "7".to_owned(),
            group_state_version: "state-2".to_owned(),
            accepted_at: "2026-07-22T00:00:00Z".to_owned(),
            epoch: "4".to_owned(),
            group_receipt: serde_json::json!({}),
        };
        accepted.validate().unwrap();

        let result = group_result(&client, &request, &accepted, None).unwrap();

        assert_eq!(result.delivery, crate::messages::DeliveryState::Accepted);
        assert!(result
            .message
            .metadata
            .attributes
            .iter()
            .any(|attribute| { attribute.key == "final_acceptance" && attribute.value == "true" }));
    }

    #[test]
    fn logical_message_identity_is_stable_across_product_retry() {
        let mut request = crate::messages::SendMessageRequest {
            target: crate::messages::MessageTarget::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
            body: crate::messages::MessageBody::Text {
                text: "hello".to_owned(),
                kind: crate::messages::MessageKind::Text,
            },
            security: crate::messages::MessageSecurityMode::SecureDirect,
            client_message_id: None,
            delivery: crate::messages::MessageDeliveryOptions::default(),
            delegated_signing: None,
        };
        normalize_message_identity(&mut request).unwrap();
        let first_id = required_message_id(&request).unwrap().to_owned();
        let first_operation = request.delivery.idempotency_key.clone();

        normalize_message_identity(&mut request).unwrap();

        assert_eq!(required_message_id(&request).unwrap(), first_id);
        assert_eq!(request.delivery.idempotency_key, first_operation);
    }

    #[test]
    #[cfg(feature = "group-e2ee")]
    fn p4_group_state_lookup_never_adds_device_selectors() {
        let params = crate::internal::wire::group::build_group_get_info_rpc_params(
            "did:example:alice",
            "did:example:group",
            "op-group-send",
            false,
        )
        .unwrap();
        let encoded = serde_json::to_string(&params).unwrap();

        assert_eq!(params["meta"]["profile"], "anp.group.base.v2");
        assert!(params["meta"].get("anp_version").is_none());
        assert_eq!(params["meta"]["target"]["did"], "did:example:group");
        assert_eq!(params["body"]["include_policy"], false);
        assert_eq!(params["body"]["include_member_list"], false);
        assert!(!encoded.contains("device_id"));
        assert!(!encoded.contains("deviceManifest"));
    }

    #[test]
    #[cfg(feature = "group-e2ee")]
    fn p6_v2_send_stops_before_encryption_when_host_projects_maintenance_pause() {
        ensure_group_e2ee_v2_send_not_paused(&serde_json::json!({
            "group_did": "did:example:group"
        }))
        .unwrap();

        let error = ensure_group_e2ee_v2_send_not_paused(&serde_json::json!({
            "group_did": "did:example:group",
            "e2ee_maintenance": {
                "reason": "device_revocation_pending",
                "send_paused": true
            }
        }))
        .unwrap_err();
        assert!(matches!(
            error,
            crate::ImError::Service { code: Some(code), .. }
                if code == "group.e2ee.state_not_ready"
        ));
    }
}
