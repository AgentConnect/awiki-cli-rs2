use serde_json::Value;

use crate::internal::auth::session::SessionProvider;
use crate::internal::transport::{AttachmentObjectTransport, AuthenticatedRpcTransport};

const MESSAGE_RPC_ENDPOINT: &str = crate::internal::message_runtime::direct::MESSAGE_RPC_ENDPOINT;

pub(crate) struct AttachmentUploadRuntime<'a, P, T> {
    client: &'a crate::core::ImClient,
    session_provider: P,
    transport: T,
}

pub(crate) struct AttachmentSendInput {
    pub target: crate::messages::MessageTarget,
    pub request: crate::attachments::AttachmentSendRequest,
    pub resolved_target_did: Option<String>,
    pub credentials: Option<AttachmentUploadCredentials>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AttachmentUploadCredentials {
    pub identity_name: String,
    pub did_document: Option<Value>,
    pub key1_private_pem: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AttachmentUploadResult {
    pub sdk_result: crate::messages::SendMessageResult,
    pub target_kind: &'static str,
    pub target_did: String,
    pub prepared: crate::attachments::manifest::PreparedAttachment,
    pub slot: crate::internal::wire::attachment::AttachmentCreateSlotResult,
    pub manifest: Value,
    pub raw: Value,
}

struct PreparedAttachmentUpload {
    prepared: crate::attachments::manifest::PreparedAttachment,
    caption: String,
}

struct ManifestSendResult {
    raw: Value,
    meta: Value,
}

impl<'a, P, T> AttachmentUploadRuntime<'a, P, T>
where
    P: SessionProvider,
    T: AuthenticatedRpcTransport + AttachmentObjectTransport,
{
    pub(crate) fn new(
        client: &'a crate::core::ImClient,
        session_provider: P,
        transport: T,
    ) -> Self {
        Self {
            client,
            session_provider,
            transport,
        }
    }

    pub(crate) fn send(
        mut self,
        input: AttachmentSendInput,
    ) -> crate::ImResult<AttachmentUploadResult> {
        let target = send_target(&input.target, input.resolved_target_did)?;
        let service_did = message_service_did(self.client)?;
        self.session_provider.ensure_session(auth_scope(&target))?;

        let upload = prepare_request(input.request)?;
        let prepared = upload.prepared;
        let slot = self.create_slot(&target, &service_did, &prepared)?;
        self.transport.put_attachment_object(
            slot.upload_uri.as_str(),
            upload_headers(&slot.upload_headers),
            prepared.payload.clone(),
        )?;
        self.commit_object(&service_did, &prepared, &slot)?;

        let descriptor = crate::attachments::manifest::AttachmentDescriptor::from_prepared(
            &prepared,
            slot.attachment_id.clone(),
            slot.object_uri.clone(),
        );
        let manifest =
            crate::attachments::manifest::build_attachment_manifest(&descriptor, &upload.caption);
        let credentials = match input.credentials {
            Some(credentials) => credentials,
            None => load_credentials(self.client)?,
        };
        let send_result = self.send_manifest(&target, manifest.clone(), credentials)?;
        let sdk_result = sdk_result_from_raw(
            send_result.raw.clone(),
            &send_result.meta,
            self.client.did().clone(),
            &target,
            &manifest,
        )?;

        Ok(AttachmentUploadResult {
            sdk_result,
            target_kind: target.kind(),
            target_did: target.did().to_string(),
            prepared,
            slot,
            manifest,
            raw: send_result.raw,
        })
    }

    fn create_slot(
        &mut self,
        target: &ResolvedAttachmentTarget,
        service_did: &str,
        prepared: &crate::attachments::manifest::PreparedAttachment,
    ) -> crate::ImResult<crate::internal::wire::attachment::AttachmentCreateSlotResult> {
        let params = crate::internal::wire::attachment::build_attachment_create_slot_rpc_params(
            self.client.did().as_str(),
            service_did,
            target.kind(),
            target.did(),
            prepared,
        )?;
        let raw = self.transport.authenticated_rpc(
            MESSAGE_RPC_ENDPOINT,
            "attachment.create_slot",
            params,
        )?;
        let mut slot: crate::internal::wire::attachment::AttachmentCreateSlotResult =
            serde_json::from_value(raw).map_err(|err| crate::ImError::Serialization {
                detail: err.to_string(),
            })?;
        slot.request_service_did = service_did.to_string();
        Ok(slot)
    }

    fn commit_object(
        &mut self,
        service_did: &str,
        prepared: &crate::attachments::manifest::PreparedAttachment,
        slot: &crate::internal::wire::attachment::AttachmentCreateSlotResult,
    ) -> crate::ImResult<()> {
        let params = crate::internal::wire::attachment::build_attachment_commit_object_rpc_params(
            self.client.did().as_str(),
            service_did,
            prepared,
            slot,
        )?;
        let _: crate::internal::wire::attachment::AttachmentCommitObjectResult =
            serde_json::from_value(self.transport.authenticated_rpc(
                MESSAGE_RPC_ENDPOINT,
                "attachment.commit_object",
                params,
            )?)
            .map_err(|err| crate::ImError::Serialization {
                detail: err.to_string(),
            })?;
        Ok(())
    }

    fn send_manifest(
        &mut self,
        target: &ResolvedAttachmentTarget,
        manifest: Value,
        credentials: AttachmentUploadCredentials,
    ) -> crate::ImResult<ManifestSendResult> {
        let identity = crate::internal::wire::attachment::AttachmentSigningIdentity {
            identity_name: credentials.identity_name,
            did: self.client.did().as_str().to_string(),
            did_document: credentials.did_document,
            key1_private_pem: credentials.key1_private_pem,
        };
        let (method, params) = match target {
            ResolvedAttachmentTarget::Direct { target_did, .. } => (
                "direct.send",
                crate::internal::wire::attachment::build_direct_attachment_send_rpc_params(
                    &identity, target_did, manifest,
                )?,
            ),
            ResolvedAttachmentTarget::Group { group } => (
                "group.send",
                crate::internal::wire::attachment::build_group_attachment_send_rpc_params(
                    &identity,
                    group.as_str(),
                    manifest,
                )?,
            ),
        };
        let meta = params.get("meta").cloned().unwrap_or(Value::Null);
        let raw = self
            .transport
            .authenticated_rpc(MESSAGE_RPC_ENDPOINT, method, params)?;
        Ok(ManifestSendResult { raw, meta })
    }
}

fn send_target(
    target: &crate::messages::MessageTarget,
    resolved_target_did: Option<String>,
) -> crate::ImResult<ResolvedAttachmentTarget> {
    match target {
        crate::messages::MessageTarget::Direct(peer) => {
            let resolved = resolved_target_did
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| peer.as_str().trim());
            if !resolved.starts_with("did:") {
                return Err(crate::ImError::PeerNotFound {
                    peer: peer.as_str().to_string(),
                });
            }
            Ok(ResolvedAttachmentTarget::Direct {
                peer: peer.clone(),
                target_did: resolved.to_string(),
            })
        }
        crate::messages::MessageTarget::Group(group) => {
            if group.as_str().trim().is_empty() {
                return Err(crate::ImError::invalid_input(
                    Some("group".to_string()),
                    "group target is required",
                ));
            }
            Ok(ResolvedAttachmentTarget::Group {
                group: group.clone(),
            })
        }
    }
}

fn auth_scope(target: &ResolvedAttachmentTarget) -> crate::auth::AuthScope {
    match target {
        ResolvedAttachmentTarget::Direct { .. } => crate::auth::AuthScope::Messaging,
        ResolvedAttachmentTarget::Group { .. } => crate::auth::AuthScope::GroupMessaging,
    }
}

fn prepare_request(
    request: crate::attachments::AttachmentSendRequest,
) -> crate::ImResult<PreparedAttachmentUpload> {
    let caption = request.caption.unwrap_or_default();
    let request_filename = request.filename;
    let request_mime = request.mime_type;
    let source = crate::internal::blob::source::attachment_input_to_blob_source(request.input)?;
    let filename = request_filename
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            source
                .filename
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        });
    let mime_type = request_mime
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            source
                .mime_type
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_default();
    let prepared = if let Some(filename) = filename {
        crate::attachments::manifest::prepare_attachment_payload(
            &filename,
            &mime_type,
            source.bytes,
        )?
    } else if let Some(path) = source.path.as_deref() {
        crate::attachments::manifest::prepare_attachment_payload_from_path(
            path,
            &mime_type,
            source.bytes,
        )?
    } else {
        return Err(crate::ImError::invalid_input(
            Some("filename".to_string()),
            "attachment filename is required",
        ));
    };
    Ok(PreparedAttachmentUpload { prepared, caption })
}

fn message_service_did(client: &crate::core::ImClient) -> crate::ImResult<String> {
    client
        .core_inner()
        .sdk_config()
        .anp_service_did
        .as_ref()
        .map(|did| did.as_str().to_string())
        .filter(|did| !did.trim().is_empty())
        .ok_or_else(|| {
            crate::ImError::invalid_input(
                Some("service_did".to_string()),
                "message service did is required",
            )
        })
}

fn upload_headers(
    headers: &serde_json::Map<String, Value>,
) -> std::collections::BTreeMap<String, String> {
    headers
        .iter()
        .filter_map(|(key, value)| value.as_str().map(|value| (key.clone(), value.to_string())))
        .collect()
}

fn load_credentials(
    client: &crate::core::ImClient,
) -> crate::ImResult<AttachmentUploadCredentials> {
    let runtime = client.runtime();
    let did_document = read_optional_json(&runtime.did_document_path)?;
    let key1_private_pem = std::fs::read_to_string(&runtime.private_key_path).map_err(|err| {
        crate::ImError::CredentialFileUnreadable {
            path_kind: "private_key".to_string(),
            detail: err.to_string(),
        }
    })?;
    Ok(AttachmentUploadCredentials {
        identity_name: runtime.owner.identity_id.as_str().to_string(),
        did_document,
        key1_private_pem,
    })
}

fn read_optional_json(path: &std::path::Path) -> crate::ImResult<Option<Value>> {
    let raw = match std::fs::read(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(crate::ImError::CredentialFileUnreadable {
                path_kind: "did_document".to_string(),
                detail: err.to_string(),
            });
        }
    };
    serde_json::from_slice(&raw)
        .map(Some)
        .map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })
}

fn sdk_result_from_raw(
    raw: Value,
    meta: &Value,
    sender: crate::ids::Did,
    target: &ResolvedAttachmentTarget,
    manifest: &Value,
) -> crate::ImResult<crate::messages::SendMessageResult> {
    match target {
        ResolvedAttachmentTarget::Direct { peer, target_did } => {
            let mut result: DirectAttachmentRpcResult =
                serde_json::from_value(raw).map_err(|err| crate::ImError::Serialization {
                    detail: err.to_string(),
                })?;
            fill_direct_result_defaults(&mut result, meta, target_did);
            sdk_result_from_direct_result(&result, sender, peer.clone(), manifest)
        }
        ResolvedAttachmentTarget::Group { group } => {
            let mut result: GroupAttachmentRpcResult =
                serde_json::from_value(raw).map_err(|err| crate::ImError::Serialization {
                    detail: err.to_string(),
                })?;
            fill_group_result_defaults(&mut result, meta, group.as_str());
            sdk_result_from_group_result(&result, sender, group.clone(), manifest)
        }
    }
}

fn fill_direct_result_defaults(
    result: &mut DirectAttachmentRpcResult,
    meta: &Value,
    target_did: &str,
) {
    if result.message_id.trim().is_empty() {
        result.message_id = string_value(meta.get("message_id")).unwrap_or_else(|| {
            format!(
                "msg-{}",
                crate::internal::wire::common::generate_operation_id()
            )
        });
    }
    if result.operation_id.trim().is_empty() {
        result.operation_id = string_value(meta.get("operation_id")).unwrap_or_else(|| {
            format!(
                "op-{}",
                crate::internal::wire::common::generate_operation_id()
            )
        });
    }
    if result.target_did.trim().is_empty() {
        result.target_did = target_did.to_string();
    }
}

fn fill_group_result_defaults(
    result: &mut GroupAttachmentRpcResult,
    meta: &Value,
    group_did: &str,
) {
    if result.group_did.trim().is_empty() {
        result.group_did = group_did.to_string();
    }
    if result.message_id.trim().is_empty() && result.group_event_seq.trim().is_empty() {
        result.message_id = string_value(meta.get("message_id")).unwrap_or_else(|| {
            format!(
                "msg-{}",
                crate::internal::wire::common::generate_operation_id()
            )
        });
    }
    if result.operation_id.trim().is_empty() {
        result.operation_id = string_value(meta.get("operation_id")).unwrap_or_else(|| {
            format!(
                "op-{}",
                crate::internal::wire::common::generate_operation_id()
            )
        });
    }
}

fn string_value(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn sdk_result_from_direct_result(
    result: &DirectAttachmentRpcResult,
    sender: crate::ids::Did,
    peer: crate::ids::PeerRef,
    manifest: &Value,
) -> crate::ImResult<crate::messages::SendMessageResult> {
    let message_id = crate::ids::MessageId::parse(&result.message_id)?;
    let delivery = direct_delivery_state(result);
    let (send_state, retry_plan) =
        crate::internal::message_runtime::state::send_state_from_delivery(
            &delivery,
            Some(result.operation_id.clone()).filter(|value| !value.trim().is_empty()),
            Some(message_id.clone()),
            Some(result.accepted_at.clone()).filter(|value| !value.trim().is_empty()),
            None,
        );
    let delivery_state = Some(result.delivery_state.clone())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            crate::internal::message_runtime::state::send_state_label(&send_state.state).to_string()
        });
    Ok(crate::messages::SendMessageResult {
        message: crate::messages::Message {
            id: message_id,
            thread: crate::messages::ThreadRef::Direct(peer.clone()),
            direction: crate::messages::MessageDirection::Outgoing,
            sender: crate::ids::PeerRef::parse(sender.as_str(), "")?,
            receiver: Some(peer),
            group: None,
            body: crate::messages::MessageBodyView::Unsupported {
                content_type: Some(
                    crate::attachments::manifest::attachment_manifest_content_type().to_string(),
                ),
            },
            sent_at: Some(result.accepted_at.clone()).filter(|value| !value.trim().is_empty()),
            received_at: None,
            metadata: attachment_metadata(
                Some(result.operation_id.clone()),
                Some(delivery_state),
                Some(send_state),
                retry_plan,
                None,
                manifest,
            ),
        },
        delivery,
        warnings: Vec::new(),
    })
}

fn sdk_result_from_group_result(
    result: &GroupAttachmentRpcResult,
    sender: crate::ids::Did,
    group: crate::ids::GroupRef,
    manifest: &Value,
) -> crate::ImResult<crate::messages::SendMessageResult> {
    let message_id = group_message_id(group.as_str(), result)?;
    let delivery = group_delivery_state(result);
    let (send_state, retry_plan) =
        crate::internal::message_runtime::state::send_state_from_delivery(
            &delivery,
            Some(result.operation_id.clone()).filter(|value| !value.trim().is_empty()),
            Some(message_id.clone()),
            Some(result.accepted_at.clone()).filter(|value| !value.trim().is_empty()),
            None,
        );
    Ok(crate::messages::SendMessageResult {
        message: crate::messages::Message {
            id: message_id,
            thread: crate::messages::ThreadRef::Group(group.clone()),
            direction: crate::messages::MessageDirection::Outgoing,
            sender: crate::ids::PeerRef::parse(sender.as_str(), "")?,
            receiver: None,
            group: Some(group),
            body: crate::messages::MessageBodyView::Unsupported {
                content_type: Some(
                    crate::attachments::manifest::attachment_manifest_content_type().to_string(),
                ),
            },
            sent_at: Some(result.accepted_at.clone()).filter(|value| !value.trim().is_empty()),
            received_at: None,
            metadata: attachment_metadata(
                Some(result.operation_id.clone()),
                Some(
                    crate::internal::message_runtime::state::send_state_label(&send_state.state)
                        .to_string(),
                ),
                Some(send_state),
                retry_plan,
                result.group_event_seq.trim().parse().ok(),
                manifest,
            )
            .with_attributes(group_metadata_attributes(result)),
        },
        delivery,
        warnings: Vec::new(),
    })
}

fn attachment_metadata(
    operation_id: Option<String>,
    delivery_state: Option<String>,
    send_state: Option<crate::messages::MessageSendState>,
    retry_plan: Option<crate::messages::MessageRetryPlan>,
    server_sequence: Option<i64>,
    manifest: &Value,
) -> crate::messages::MessageMetadata {
    let attributes = vec![crate::messages::MessageMetadataAttribute {
        key: "attachment_manifest".to_string(),
        value: crate::attachments::manifest::manifest_content_string(manifest),
    }];
    crate::messages::MessageMetadata {
        operation_id: operation_id.filter(|value| !value.trim().is_empty()),
        delivery_state: delivery_state.filter(|value| !value.trim().is_empty()),
        send_state,
        retry_plan,
        server_sequence,
        content_type: Some(
            crate::attachments::manifest::attachment_manifest_content_type().to_string(),
        ),
        attributes,
    }
}

trait WithAttributes {
    fn with_attributes(self, attributes: Vec<crate::messages::MessageMetadataAttribute>) -> Self;
}

impl WithAttributes for crate::messages::MessageMetadata {
    fn with_attributes(
        mut self,
        attributes: Vec<crate::messages::MessageMetadataAttribute>,
    ) -> Self {
        self.attributes.extend(attributes);
        self
    }
}

fn group_message_id(
    group_did: &str,
    result: &GroupAttachmentRpcResult,
) -> crate::ImResult<crate::ids::MessageId> {
    if !result.group_did.trim().is_empty() && !result.group_event_seq.trim().is_empty() {
        return crate::ids::MessageId::parse(format!(
            "{}:{}",
            result.group_did.trim(),
            result.group_event_seq.trim()
        ));
    }
    if !result.group_event_seq.trim().is_empty() {
        return crate::ids::MessageId::parse(format!(
            "{}:{}",
            group_did.trim(),
            result.group_event_seq.trim()
        ));
    }
    if !result.message_id.trim().is_empty() {
        return crate::ids::MessageId::parse(&result.message_id);
    }
    crate::ids::MessageId::parse(format!(
        "msg-{}",
        crate::internal::wire::common::generate_operation_id()
    ))
}

fn group_metadata_attributes(
    result: &GroupAttachmentRpcResult,
) -> Vec<crate::messages::MessageMetadataAttribute> {
    let mut attributes = Vec::new();
    if !result.message_id.trim().is_empty() {
        attributes.push(crate::messages::MessageMetadataAttribute {
            key: "raw_message_id".to_string(),
            value: result.message_id.clone(),
        });
    }
    if !result.group_event_seq.trim().is_empty() {
        attributes.push(crate::messages::MessageMetadataAttribute {
            key: "group_event_seq".to_string(),
            value: result.group_event_seq.clone(),
        });
    }
    if !result.group_state_version.trim().is_empty() {
        attributes.push(crate::messages::MessageMetadataAttribute {
            key: "group_state_version".to_string(),
            value: result.group_state_version.clone(),
        });
    }
    attributes
}

fn direct_delivery_state(result: &DirectAttachmentRpcResult) -> crate::messages::DeliveryState {
    if !result.delivery_state.trim().is_empty() {
        match result.delivery_state.trim().to_ascii_lowercase().as_str() {
            "sent" => crate::messages::DeliveryState::Sent,
            "stored_locally" | "stored-locally" => crate::messages::DeliveryState::StoredLocally,
            "failed" => crate::messages::DeliveryState::Failed {
                reason: result.delivery_state.clone(),
            },
            _ => crate::messages::DeliveryState::Accepted,
        }
    } else if result.accepted || result.final_acceptance {
        crate::messages::DeliveryState::Accepted
    } else {
        crate::messages::DeliveryState::Failed {
            reason: "not accepted".to_string(),
        }
    }
}

fn group_delivery_state(result: &GroupAttachmentRpcResult) -> crate::messages::DeliveryState {
    if result.accepted || result.final_acceptance {
        crate::messages::DeliveryState::Accepted
    } else {
        crate::messages::DeliveryState::Failed {
            reason: "not accepted".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum ResolvedAttachmentTarget {
    Direct {
        peer: crate::ids::PeerRef,
        target_did: String,
    },
    Group {
        group: crate::ids::GroupRef,
    },
}

impl ResolvedAttachmentTarget {
    fn kind(&self) -> &'static str {
        match self {
            Self::Direct { .. } => "agent",
            Self::Group { .. } => "group",
        }
    }

    fn did(&self) -> &str {
        match self {
            Self::Direct { target_did, .. } => target_did,
            Self::Group { group } => group.as_str(),
        }
    }
}

#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize, PartialEq)]
struct DirectAttachmentRpcResult {
    #[serde(default)]
    accepted: bool,
    #[serde(default)]
    message_id: String,
    #[serde(default)]
    operation_id: String,
    #[serde(default)]
    target_did: String,
    #[serde(default)]
    accepted_at: String,
    #[serde(default)]
    final_acceptance: bool,
    #[serde(default)]
    delivery_state: String,
}

#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize, PartialEq)]
struct GroupAttachmentRpcResult {
    #[serde(default)]
    accepted: bool,
    #[serde(default)]
    final_acceptance: bool,
    #[serde(default)]
    group_did: String,
    #[serde(default)]
    message_id: String,
    #[serde(default)]
    operation_id: String,
    #[serde(default)]
    group_event_seq: String,
    #[serde(default)]
    group_state_version: String,
    #[serde(default)]
    accepted_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal::transport::{AttachmentObjectTransport, AuthenticatedRpcTransport};
    use serde_json::{json, Value};
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;
    use std::rc::Rc;

    #[test]
    fn attachments_upload_runtime_bytes_direct_runs_create_put_commit_and_send() {
        let fixture = Fixture::new(Some("did:example:message-service"));
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let sessions = Rc::new(RefCell::new(Vec::new()));
        let result = AttachmentUploadRuntime::new(
            &client,
            ReadySessionProvider {
                scopes: Rc::clone(&sessions),
            },
            RecordingTransport {
                calls: Rc::clone(&calls),
            },
        )
        .send(AttachmentSendInput {
            target: crate::messages::MessageTarget::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
            request: bytes_request(
                Some("input.txt"),
                Some("text/plain"),
                b"hello".to_vec(),
                Some("override.bin"),
                Some("application/custom"),
                Some("caption"),
            ),
            resolved_target_did: None,
            credentials: Some(fixture.credentials()),
        })
        .unwrap();

        assert_eq!(
            sessions.borrow().as_slice(),
            &[crate::auth::AuthScope::Messaging]
        );
        assert_eq!(result.target_kind, "agent");
        assert_eq!(result.target_did, "did:example:bob");
        assert_eq!(result.prepared.filename, "override.bin");
        assert_eq!(result.prepared.mime_type, "application/custom");
        assert_eq!(result.prepared.payload, b"hello".to_vec());
        assert_eq!(result.manifest["caption"], "caption");
        assert_eq!(
            result.sdk_result.message.metadata.content_type.as_deref(),
            Some(crate::attachments::manifest::attachment_manifest_content_type())
        );
        assert!(matches!(
            result.sdk_result.message.body,
            crate::messages::MessageBodyView::Unsupported { content_type }
                if content_type.as_deref()
                    == Some(crate::attachments::manifest::attachment_manifest_content_type())
        ));
        assert_eq!(
            result.sdk_result.message.receiver.unwrap().as_str(),
            "did:example:bob"
        );

        let calls = calls.borrow();
        assert_eq!(calls.len(), 4);
        let create = calls[0].rpc("attachment.create_slot");
        assert_eq!(create.endpoint, MESSAGE_RPC_ENDPOINT);
        assert_eq!(
            create.params["meta"]["target"],
            json!({"kind": "service", "did": "did:example:message-service"})
        );
        assert_eq!(create.params["body"]["filename"], "override.bin");
        assert_eq!(create.params["body"]["mime_type"], "application/custom");
        assert_eq!(create.params["body"]["expected_size"], "5");
        assert_eq!(
            create.params["body"]["intended_target"],
            json!({"kind": "agent", "did": "did:example:bob"})
        );

        let put = calls[1].put("https://upload.example/slot-1");
        assert_eq!(
            put.headers.get("X-Upload-Token").map(String::as_str),
            Some("token-1")
        );
        assert_eq!(put.headers.get("Ignored-Number"), None);
        assert_eq!(put.body.as_slice(), b"hello");

        let commit = calls[2].rpc("attachment.commit_object");
        assert_eq!(commit.params["body"]["attachment_id"], "att-1");
        assert_eq!(commit.params["body"]["slot_id"], "slot-1");
        assert_eq!(
            commit.params["body"]["digest"]["value_b64u"],
            result.prepared.digest_b64u
        );

        let send = calls[3].rpc("direct.send");
        assert_eq!(
            send.params["meta"]["content_type"],
            crate::attachments::manifest::attachment_manifest_content_type()
        );
        assert_eq!(
            send.params["body"]["payload"]["primary_attachment_id"],
            "att-1"
        );
        assert_eq!(send.params["body"]["payload"]["caption"], "caption");
        assert_eq!(
            result.sdk_result.message.id.as_str(),
            send.params["meta"]["message_id"].as_str().unwrap()
        );
        assert_eq!(
            result.sdk_result.message.metadata.operation_id.as_deref(),
            send.params["meta"]["operation_id"].as_str()
        );
    }

    #[test]
    fn attachments_upload_runtime_local_file_reads_only_explicit_path() {
        let fixture = Fixture::new(Some("did:example:message-service"));
        let client = fixture.client();
        let file = fixture.root.join("explicit").join("report.txt");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, b"file bytes").unwrap();

        let calls = Rc::new(RefCell::new(Vec::new()));
        let result = AttachmentUploadRuntime::new(
            &client,
            ReadySessionProvider {
                scopes: Rc::new(RefCell::new(Vec::new())),
            },
            RecordingTransport {
                calls: Rc::clone(&calls),
            },
        )
        .send(AttachmentSendInput {
            target: crate::messages::MessageTarget::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
            request: crate::attachments::AttachmentSendRequest {
                input: crate::attachments::AttachmentInput::LocalFile(file.clone()),
                caption: None,
                mime_type: None,
                filename: None,
                delivery: crate::messages::MessageDeliveryOptions::default(),
            },
            resolved_target_did: None,
            credentials: Some(fixture.credentials()),
        })
        .unwrap();

        assert_eq!(result.prepared.filename, "report.txt");
        assert_eq!(result.prepared.mime_type, "text/plain; charset=utf-8");
        let calls = calls.borrow();
        assert_eq!(
            calls[1]
                .put("https://upload.example/slot-1")
                .body
                .as_slice(),
            b"file bytes"
        );
    }

    #[test]
    fn attachments_upload_runtime_group_uses_group_scope_and_wire() {
        let fixture = Fixture::new(Some("did:example:message-service"));
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let sessions = Rc::new(RefCell::new(Vec::new()));

        let result = AttachmentUploadRuntime::new(
            &client,
            ReadySessionProvider {
                scopes: Rc::clone(&sessions),
            },
            RecordingTransport {
                calls: Rc::clone(&calls),
            },
        )
        .send(AttachmentSendInput {
            target: crate::messages::MessageTarget::Group(
                crate::ids::GroupRef::parse("did:example:group").unwrap(),
            ),
            request: bytes_request(
                Some("group.txt"),
                None,
                b"group attachment".to_vec(),
                None,
                None,
                None,
            ),
            resolved_target_did: None,
            credentials: Some(fixture.credentials()),
        })
        .unwrap();

        assert_eq!(
            sessions.borrow().as_slice(),
            &[crate::auth::AuthScope::GroupMessaging]
        );
        assert_eq!(result.target_kind, "group");
        assert_eq!(result.target_did, "did:example:group");
        assert_eq!(result.sdk_result.message.id.as_str(), "did:example:group:7");
        assert_eq!(result.sdk_result.message.metadata.server_sequence, Some(7));
        assert!(result
            .sdk_result
            .message
            .metadata
            .attributes
            .iter()
            .any(|attribute| attribute.key == "group_event_seq" && attribute.value == "7"));

        let calls = calls.borrow();
        let create = calls[0].rpc("attachment.create_slot");
        assert_eq!(
            create.params["body"]["intended_target"],
            json!({"kind": "group", "did": "did:example:group"})
        );
        let send = calls[3].rpc("group.send");
        assert_eq!(
            send.params["meta"]["target"],
            json!({"kind": "group", "did": "did:example:group"})
        );
    }

    #[test]
    fn attachments_upload_runtime_bytes_requires_filename_before_transport() {
        let fixture = Fixture::new(Some("did:example:message-service"));
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));

        let err = AttachmentUploadRuntime::new(
            &client,
            ReadySessionProvider {
                scopes: Rc::new(RefCell::new(Vec::new())),
            },
            RecordingTransport {
                calls: Rc::clone(&calls),
            },
        )
        .send(AttachmentSendInput {
            target: crate::messages::MessageTarget::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
            request: bytes_request(None, None, b"hello".to_vec(), None, None, None),
            resolved_target_did: None,
            credentials: Some(fixture.credentials()),
        })
        .expect_err("bytes input without filename should fail");

        assert!(matches!(
            err,
            crate::ImError::InvalidInput { field: Some(field), message }
                if field == "filename" && message == "attachment filename is required"
        ));
        assert!(calls.borrow().is_empty());
    }

    #[test]
    fn attachments_upload_runtime_direct_handle_requires_resolved_did() {
        let fixture = Fixture::new(Some("did:example:message-service"));
        let client = fixture.client();

        let err = AttachmentUploadRuntime::new(
            &client,
            ReadySessionProvider {
                scopes: Rc::new(RefCell::new(Vec::new())),
            },
            RecordingTransport {
                calls: Rc::new(RefCell::new(Vec::new())),
            },
        )
        .send(AttachmentSendInput {
            target: crate::messages::MessageTarget::Direct(
                crate::ids::PeerRef::parse("bob.awiki.test", "").unwrap(),
            ),
            request: bytes_request(Some("note.txt"), None, b"hello".to_vec(), None, None, None),
            resolved_target_did: None,
            credentials: Some(fixture.credentials()),
        })
        .expect_err("unresolved direct handle should fail");

        assert!(matches!(
            err,
            crate::ImError::PeerNotFound { peer } if peer == "bob.awiki.test"
        ));
    }

    #[derive(Clone)]
    struct ReadySessionProvider {
        scopes: Rc<RefCell<Vec<crate::auth::AuthScope>>>,
    }

    impl SessionProvider for ReadySessionProvider {
        fn ensure_session(
            &self,
            scope: crate::auth::AuthScope,
        ) -> crate::ImResult<crate::auth::SessionBundle> {
            self.scopes.borrow_mut().push(scope);
            Ok(crate::auth::SessionBundle {
                subject: crate::ids::Did::parse("did:example:alice")?,
                scope,
                expires_at: None,
                refreshed: false,
            })
        }

        fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
            unreachable!("attachment upload runtime should not refresh through the test provider")
        }

        fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
            unreachable!("attachment upload runtime should not read status")
        }
    }

    struct RecordingTransport {
        calls: Rc<RefCell<Vec<RecordedCall>>>,
    }

    impl AuthenticatedRpcTransport for RecordingTransport {
        fn authenticated_rpc(
            &mut self,
            endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            self.calls.borrow_mut().push(RecordedCall::Rpc {
                endpoint: endpoint.to_string(),
                method: method.to_string(),
                params: params.clone(),
            });
            match method {
                "attachment.create_slot" => Ok(json!({
                    "attachment_id": "att-1",
                    "slot_id": "slot-1",
                    "upload_uri": "https://upload.example/slot-1",
                    "upload_headers": {
                        "X-Upload-Token": "token-1",
                        "Ignored-Number": 7
                    },
                    "object_uri": "https://objects.example/att-1",
                    "commit_token": "commit-token-1",
                    "expires_at": "2026-05-23T01:00:00Z"
                })),
                "attachment.commit_object" => Ok(json!({
                    "committed": true,
                    "attachment_id": "att-1",
                    "object_uri": "https://objects.example/att-1",
                    "committed_at": "2026-05-23T00:00:01Z"
                })),
                "direct.send" => Ok(json!({
                    "accepted": true,
                    "accepted_at": "2026-05-23T00:00:02Z",
                    "delivery_state": "accepted"
                })),
                "group.send" => Ok(json!({
                    "accepted": true,
                    "group_did": "did:example:group",
                    "message_id": "group-raw-message-7",
                    "operation_id": "group-op-7",
                    "group_event_seq": "7",
                    "group_state_version": "3",
                    "accepted_at": "2026-05-23T00:00:03Z"
                })),
                _ => Err(crate::ImError::TransportUnavailable {
                    detail: format!("unexpected method {method} at {endpoint}"),
                }),
            }
        }
    }

    impl AttachmentObjectTransport for RecordingTransport {
        fn put_attachment_object(
            &mut self,
            upload_uri: &str,
            headers: BTreeMap<String, String>,
            body: Vec<u8>,
        ) -> crate::ImResult<()> {
            self.calls.borrow_mut().push(RecordedCall::Put {
                upload_uri: upload_uri.to_string(),
                headers,
                body,
            });
            Ok(())
        }

        fn get_attachment_object(
            &mut self,
            _object_uri: &str,
            _download_ticket: &str,
        ) -> crate::ImResult<crate::internal::transport::AttachmentObjectResponse> {
            unreachable!("upload runtime should not download objects")
        }
    }

    #[derive(Debug, Clone)]
    enum RecordedCall {
        Rpc {
            endpoint: String,
            method: String,
            params: Value,
        },
        Put {
            upload_uri: String,
            headers: BTreeMap<String, String>,
            body: Vec<u8>,
        },
    }

    impl RecordedCall {
        fn rpc(&self, expected_method: &str) -> RecordedRpc<'_> {
            match self {
                Self::Rpc {
                    endpoint,
                    method,
                    params,
                } => {
                    assert_eq!(method, expected_method);
                    RecordedRpc { endpoint, params }
                }
                Self::Put { .. } => panic!("expected rpc call {expected_method}, got put call"),
            }
        }

        fn put(&self, expected_uri: &str) -> RecordedPut<'_> {
            match self {
                Self::Put {
                    upload_uri,
                    headers,
                    body,
                } => {
                    assert_eq!(upload_uri, expected_uri);
                    RecordedPut { headers, body }
                }
                Self::Rpc { method, .. } => panic!("expected put call, got rpc call {method}"),
            }
        }
    }

    struct RecordedRpc<'a> {
        endpoint: &'a str,
        params: &'a Value,
    }

    struct RecordedPut<'a> {
        headers: &'a BTreeMap<String, String>,
        body: &'a Vec<u8>,
    }

    struct Fixture {
        root: PathBuf,
        service_did: Option<&'static str>,
    }

    impl Fixture {
        fn new(service_did: Option<&'static str>) -> Self {
            let root = unique_temp_root();
            let identities = root.join("identities");
            fs::create_dir_all(identities.join("alice")).unwrap();
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
            Self { root, service_did }
        }

        fn client(&self) -> crate::core::ImClient {
            crate::core::ImCore::new(
                crate::ImCoreConfig {
                    service_base_url: crate::ServiceEndpoint::parse("https://example.test")
                        .unwrap(),
                    did_domain: "awiki.test".to_string(),
                    user_service_endpoint: None,
                    message_service_endpoint: None,
                    mail_service_endpoint: None,
                    anp_service_endpoint: None,
                    anp_service_did: self
                        .service_did
                        .map(crate::ids::Did::parse)
                        .transpose()
                        .unwrap(),
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
                "alice".to_string(),
            ))
            .unwrap()
        }

        fn credentials(&self) -> AttachmentUploadCredentials {
            let bundle = anp::authentication::create_did_wba_document(
                "awiki.test",
                anp::authentication::DidDocumentOptions {
                    path_segments: vec!["user".to_string()],
                    domain: Some("awiki.test".to_string()),
                    challenge: Some("attachment-upload-runtime-test".to_string()),
                    ..anp::authentication::DidDocumentOptions::default()
                },
            )
            .unwrap();
            AttachmentUploadCredentials {
                identity_name: "alice".to_string(),
                key1_private_pem: bundle.private_key_pem("key-1").unwrap().to_string(),
                did_document: Some(bundle.did_document),
            }
        }
    }

    fn bytes_request(
        input_filename: Option<&str>,
        input_mime: Option<&str>,
        bytes: Vec<u8>,
        request_filename: Option<&str>,
        request_mime: Option<&str>,
        caption: Option<&str>,
    ) -> crate::attachments::AttachmentSendRequest {
        crate::attachments::AttachmentSendRequest {
            input: crate::attachments::AttachmentInput::Bytes {
                filename: input_filename.map(ToOwned::to_owned),
                mime_type: input_mime.map(ToOwned::to_owned),
                bytes,
            },
            caption: caption.map(ToOwned::to_owned),
            mime_type: request_mime.map(ToOwned::to_owned),
            filename: request_filename.map(ToOwned::to_owned),
            delivery: crate::messages::MessageDeliveryOptions::default(),
        }
    }

    fn unique_temp_root() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "im-core-attachment-upload-runtime-{}-{nanos}",
            std::process::id()
        ))
    }
}
