//! Public message-service adapters for the P5/P6 v2 product runtimes.
//!
//! The protocol-specific runtimes own encryption and exact-device state. This
//! adapter keeps the public SDK contract at one logical message and persists
//! only the secret-free local projection.

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
                conversation_id: None,
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
                conversation_id: None,
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
    let delivery = product_delivery(
        summary.fully_accepted(),
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
    if attachment_projection.is_some() {
        let content_type =
            crate::attachments::manifest::attachment_manifest_content_type().to_owned();
        result.message.body = crate::messages::MessageBodyView::Unsupported {
            content_type: Some(content_type.clone()),
        };
        result.message.metadata.content_type = Some(content_type);
        result.message.metadata.retry_plan = None;
    }
    result.message.sent_at = summary.accepted_at.clone();
    add_product_attribute(&mut result, "e2ee_profile", "anp.direct.e2ee.v2");
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
pub(super) fn send_group(
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
    let group_state_ref = fetch_current_group_state_ref(client, group_did, &operation_id)?;

    let runtime = crate::internal::group_e2ee::v2_runtime::runtime_for_client(client)?;
    let proof_identity = crate::internal::proof::origin::OriginProofIdentity {
        identity_name: client.current_identity().id.as_str().to_owned(),
        did_document: Some(did_document.clone()),
        key1_private_pem: client
            .runtime()
            .key_provider
            .device_request_signing_private_pem()?,
        verification_method: Some(device.signing_key_id),
    };
    let host = crate::internal::group_e2ee::v2_product::RpcGroupE2eeV2Host::new(
        crate::internal::transport::CoreHttpTransport::new(client),
        proof_identity,
    );
    let mut product =
        crate::internal::group_e2ee::v2_product::GroupE2eeV2Product::new(runtime, host);
    let meta = anp::group_e2ee::V2GroupSendMetadata {
        anp_version: Some("2.0".to_owned()),
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
                .prepare_and_commit_object(
                    crate::internal::attachment_runtime::upload::AttachmentPrepareObjectInput {
                        target: request.target.clone(),
                        request: attachment,
                        resolved_target_did: None,
                        message_security_profile: "group-e2ee",
                    },
                )?;
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
    let accepted = product.submit_product_application_send(&prepared)?;
    let mut result = group_result(client, &request, &accepted, attachment_projection.as_ref())?;
    match persist_group_projection(
        client,
        group_did,
        &request,
        attachment_projection.as_ref(),
        &result,
    ) {
        Ok(()) => client.emit_committed_local_message_projection("local_send"),
        Err(err) => result
            .warnings
            .push(format!("Failed to persist local P6 v2 message: {err}")),
    }
    Ok(result)
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
        let client = client.clone();
        return crate::internal::runtime::worker::run_blocking(move || {
            send_group(&client, request)
        })
        .await
        .map_err(|err| crate::ImError::Internal {
            message: err.to_string(),
        })?;
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
fn fetch_current_group_state_ref(
    client: &crate::core::ImClient,
    group_did: &str,
    operation_id: &str,
) -> crate::ImResult<anp::group_e2ee::V2GroupStateRef> {
    let params = group_info_params(client.did().as_str(), group_did, operation_id);
    let mut transport = crate::internal::transport::CoreHttpTransport::new(client);
    let raw = crate::internal::transport::AuthenticatedRpcTransport::authenticated_rpc(
        &mut transport,
        crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT,
        "group.get_info",
        params,
    )?;
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
fn group_info_params(sender_did: &str, group_did: &str, operation_id: &str) -> Value {
    serde_json::json!({
        "meta": {
            "anp_version": "2.0",
            "profile": "anp.group.base.v2",
            "security_profile": "transport-protected",
            "sender_did": sender_did,
            "target": { "kind": "group", "did": group_did },
            "operation_id": format!("p4-group-info-{operation_id}"),
            "content_type": "application/json",
            "created_at": crate::internal::wire::common::now_rfc3339()
        },
        "body": {
            "include_policy": false,
            "include_member_list": false
        }
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
    if attachment_projection.is_some() {
        let content_type =
            crate::attachments::manifest::attachment_manifest_content_type().to_owned();
        result.message.body = crate::messages::MessageBodyView::Unsupported {
            content_type: Some(content_type.clone()),
        };
        result.message.metadata.content_type = Some(content_type);
        result.message.metadata.retry_plan = None;
    }
    result.message.sent_at = Some(accepted.accepted_at.clone());
    add_product_attribute(&mut result, "e2ee_profile", "anp.group.e2ee.v2");
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
fn persist_group_projection(
    client: &crate::core::ImClient,
    group_did: &str,
    request: &crate::messages::SendMessageRequest,
    attachment_projection: Option<&Value>,
    result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    if let Some(manifest) = attachment_projection {
        return crate::internal::message_runtime::local_projection::persist_group_e2ee_attachment_outgoing(
            client, group_did, manifest, result,
        );
    }
    match &request.body {
        crate::messages::MessageBody::Text { text, kind } => {
            crate::internal::message_runtime::local_projection::persist_group_e2ee_outgoing(
                client, group_did, text, kind, result,
            )
        }
        crate::messages::MessageBody::Payload { payload } => {
            crate::internal::message_runtime::local_projection::persist_group_e2ee_payload_outgoing(
                client, group_did, payload, result,
            )
        }
        crate::messages::MessageBody::Attachment { .. } => Err(crate::ImError::PermissionDenied),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let params = group_info_params("did:example:alice", "did:example:group", "op-group-send");
        let encoded = serde_json::to_string(&params).unwrap();

        assert_eq!(params["meta"]["profile"], "anp.group.base.v2");
        assert_eq!(params["meta"]["target"]["did"], "did:example:group");
        assert!(!encoded.contains("device_id"));
        assert!(!encoded.contains("deviceManifest"));
    }
}
