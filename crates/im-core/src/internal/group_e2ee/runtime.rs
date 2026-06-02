use anp::group_e2ee::operations::EncryptInput;
use anp::group_e2ee::{GroupApplicationPlaintext, GroupStateRef};

use crate::internal::auth::session::{AsyncSessionProvider, SessionProvider};
use crate::internal::message_runtime::group::{
    content_type_for_message_type, group_target, load_credentials, load_credentials_async,
    message_type, sdk_text_result_from_group_result, text_body, GroupRpcResult,
    GroupTextCredentials,
};
use crate::internal::transport::{AsyncAuthenticatedRpcTransport, AuthenticatedRpcTransport};

use super::provider::GroupMlsProvider;
use super::state_ref::{resolve_group_state_ref_service_first, ResolveGroupStateRef};
use super::DEFAULT_GROUP_MLS_DEVICE_ID;

pub(crate) struct GroupE2eeTextSender<'a, P, T, M> {
    client: &'a crate::core::ImClient,
    session_provider: P,
    transport: T,
    mls_provider: M,
}

pub(crate) struct GroupE2eeTextSend {
    pub request: crate::messages::SendMessageRequest,
    pub group_state_ref: Option<GroupStateRef>,
    pub credentials: Option<GroupTextCredentials>,
}

pub(crate) struct GroupE2eeAttachmentSend {
    pub request: crate::messages::SendMessageRequest,
    pub group_state_ref: Option<GroupStateRef>,
    pub credentials: Option<GroupTextCredentials>,
    pub committed: crate::internal::attachment_runtime::upload::PreparedCommittedAttachment,
}

pub(crate) struct GroupE2eeTextSendResult {
    pub sdk_result: crate::messages::SendMessageResult,
    pub group_did: String,
    pub operation_id: String,
    pub message_id: String,
    pub raw: serde_json::Value,
}

struct GroupE2eePreparedTextSend {
    group: crate::ids::GroupRef,
    body: GroupE2eeApplicationBody,
    operation_id: String,
    message_id: String,
    credentials: GroupTextCredentials,
    group_state_ref: Option<GroupStateRef>,
    client_context: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq)]
enum GroupE2eeApplicationBody {
    Text {
        text: String,
        kind: crate::messages::MessageKind,
    },
    Attachment {
        full_manifest: serde_json::Value,
        redacted_manifest: serde_json::Value,
    },
}

impl GroupE2eeApplicationBody {
    fn application_plaintext(&self, group_did: &str) -> GroupApplicationPlaintext {
        match self {
            Self::Text { text, kind } => GroupApplicationPlaintext {
                application_content_type: content_type_for_message_type(message_type(kind))
                    .to_owned(),
                thread_id: Some(group_did.to_owned()),
                reply_to_message_id: None,
                annotations: Default::default(),
                text: Some(text.clone()),
                payload: None,
                payload_b64u: None,
            },
            Self::Attachment { full_manifest, .. } => GroupApplicationPlaintext {
                application_content_type:
                    crate::attachments::manifest::attachment_manifest_content_type().to_owned(),
                thread_id: Some(group_did.to_owned()),
                reply_to_message_id: None,
                annotations: Default::default(),
                text: None,
                payload: Some(full_manifest.clone()),
                payload_b64u: None,
            },
        }
    }

    fn sdk_result(
        &self,
        result: &GroupRpcResult,
        sender: crate::ids::Did,
        group: crate::ids::GroupRef,
    ) -> crate::ImResult<crate::messages::SendMessageResult> {
        match self {
            Self::Text { text, kind } => {
                sdk_text_result_from_group_result(result, sender, group, text, kind.clone())
            }
            Self::Attachment {
                redacted_manifest, ..
            } => sdk_attachment_result_from_group_result(result, sender, group, redacted_manifest),
        }
    }

    fn persist_outgoing(
        &self,
        client: &crate::core::ImClient,
        group_did: &str,
        sdk_result: &crate::messages::SendMessageResult,
    ) -> crate::ImResult<()> {
        match self {
            Self::Text { text, kind } => {
                crate::internal::message_runtime::local_projection::persist_group_e2ee_outgoing(
                    client,
                    group_did,
                    text,
                    kind,
                    sdk_result,
                )
            }
            Self::Attachment {
                redacted_manifest, ..
            } => crate::internal::message_runtime::local_projection::persist_group_e2ee_attachment_outgoing(
                client,
                group_did,
                redacted_manifest,
                sdk_result,
            ),
        }
    }

    async fn persist_outgoing_async(
        &self,
        client: &crate::core::ImClient,
        group_did: &str,
        sdk_result: &crate::messages::SendMessageResult,
    ) -> crate::ImResult<()> {
        match self {
            Self::Text { text, kind } => {
                crate::internal::message_runtime::local_projection::persist_group_e2ee_outgoing_async(
                    client,
                    group_did,
                    text,
                    kind,
                    sdk_result,
                )
                .await
            }
            Self::Attachment {
                redacted_manifest, ..
            } => crate::internal::message_runtime::local_projection::persist_group_e2ee_attachment_outgoing_async(
                client,
                group_did,
                redacted_manifest,
                sdk_result,
            )
            .await,
        }
    }
}

impl<'a, P, T, M> GroupE2eeTextSender<'a, P, T, M>
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

    pub(crate) fn send(
        mut self,
        input: GroupE2eeTextSend,
    ) -> crate::ImResult<GroupE2eeTextSendResult> {
        let group = group_target(&input.request.target)?;
        let (text, kind) = text_body(&input.request.body)?;
        validate_group_e2ee_security(&input.request.security)?;
        self.session_provider
            .ensure_session(crate::auth::AuthScope::GroupMessaging)?;

        let credentials = match input.credentials {
            Some(credentials) => credentials,
            None => load_credentials(self.client)?,
        };
        let operation_id = input
            .request
            .delivery
            .idempotency_key
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| {
                format!(
                    "op-{}",
                    crate::internal::wire::common::generate_operation_id()
                )
            });
        let message_id = input
            .request
            .client_message_id
            .as_ref()
            .map(|value| value.as_str().to_owned())
            .unwrap_or_else(|| {
                format!(
                    "msg-{}",
                    crate::internal::wire::common::generate_operation_id()
                )
            });
        let prepared = GroupE2eePreparedTextSend {
            group,
            body: GroupE2eeApplicationBody::Text {
                text: text.to_owned(),
                kind,
            },
            operation_id,
            message_id,
            credentials,
            group_state_ref: input.group_state_ref,
            client_context: None,
        };
        self.send_prepared(&prepared, false).or_else(|err| {
            if !is_group_e2ee_epoch_mismatch(&err) {
                return Err(err);
            }
            let retry = self.repair_for_epoch_mismatch(&prepared.group, &prepared.credentials)?;
            let retry_prepared = GroupE2eePreparedTextSend {
                group_state_ref: None,
                ..prepared
            };
            let mut result = self.send_prepared(&retry_prepared, true)?;
            let mut warnings = retry.warnings;
            warnings.extend(result.sdk_result.warnings);
            result.sdk_result.warnings = compact_warnings(warnings);
            Ok(result)
        })
    }

    pub(crate) fn send_attachment(
        mut self,
        input: GroupE2eeAttachmentSend,
    ) -> crate::ImResult<GroupE2eeTextSendResult> {
        let group = group_target(&input.request.target)?;
        validate_group_e2ee_security(&input.request.security)?;
        self.session_provider
            .ensure_session(crate::auth::AuthScope::GroupMessaging)?;

        let credentials = match input.credentials {
            Some(credentials) => credentials,
            None => load_credentials(self.client)?,
        };
        let operation_id = input
            .request
            .delivery
            .idempotency_key
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| {
                format!(
                    "op-{}",
                    crate::internal::wire::common::generate_operation_id()
                )
            });
        let message_id = input
            .request
            .client_message_id
            .as_ref()
            .map(|value| value.as_str().to_owned())
            .unwrap_or_else(|| {
                format!(
                    "msg-{}",
                    crate::internal::wire::common::generate_operation_id()
                )
            });
        let prepared = GroupE2eePreparedTextSend {
            group,
            body: GroupE2eeApplicationBody::Attachment {
                full_manifest: input.committed.full_manifest,
                redacted_manifest: input.committed.redacted_manifest,
            },
            operation_id,
            message_id,
            credentials,
            group_state_ref: input.group_state_ref,
            client_context: Some(
                crate::internal::secure_direct::send::attachment_client_context(
                    input.committed.grant_ref,
                ),
            ),
        };
        self.send_prepared(&prepared, false).or_else(|err| {
            if !is_group_e2ee_epoch_mismatch(&err) {
                return Err(err);
            }
            let retry = self.repair_for_epoch_mismatch(&prepared.group, &prepared.credentials)?;
            let retry_prepared = GroupE2eePreparedTextSend {
                group_state_ref: None,
                ..prepared
            };
            let mut result = self.send_prepared(&retry_prepared, true)?;
            let mut warnings = retry.warnings;
            warnings.extend(result.sdk_result.warnings);
            result.sdk_result.warnings = compact_warnings(warnings);
            Ok(result)
        })
    }

    fn send_prepared(
        &mut self,
        input: &GroupE2eePreparedTextSend,
        retry: bool,
    ) -> crate::ImResult<GroupE2eeTextSendResult> {
        let group_state_ref = match input.group_state_ref.clone() {
            Some(group_state_ref) => group_state_ref,
            None => {
                resolve_group_state_ref_service_first(
                    self.client,
                    &self.session_provider,
                    &mut self.transport,
                    &self.mls_provider,
                    ResolveGroupStateRef {
                        group: input.group.clone(),
                        credentials: Some(input.credentials.clone()),
                    },
                )?
                .group_state_ref
            }
        };
        let device_id = self
            .client
            .current_identity()
            .device_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(DEFAULT_GROUP_MLS_DEVICE_ID)
            .to_owned();
        let encrypted = self.mls_provider.encrypt(EncryptInput {
            sender_did: self.client.did().as_str().to_owned(),
            device_id,
            group_state_ref,
            message_id: input.message_id.clone(),
            operation_id: input.operation_id.clone(),
            application_plaintext: input.body.application_plaintext(input.group.as_str()),
            request_id: if retry {
                format!("group-e2ee-encrypt-retry-{}", input.operation_id)
            } else {
                format!("group-e2ee-encrypt-{}", input.operation_id)
            },
        })?;
        let params = super::wire::build_group_e2ee_send_rpc_params_with_client_context(
            &input.credentials,
            self.client.did().as_str(),
            input.group.as_str(),
            &encrypted.group_cipher_object,
            &input.operation_id,
            &input.message_id,
            input.client_context.clone(),
        )?;
        let raw = self.transport.authenticated_rpc(
            crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT,
            "group.e2ee.send",
            params,
        )?;
        let mut result: GroupRpcResult =
            serde_json::from_value(raw.clone()).map_err(|err| crate::ImError::Serialization {
                detail: err.to_string(),
            })?;
        if result.group_did.trim().is_empty() {
            result.group_did = input.group.as_str().to_owned();
        }
        if result.message_id.trim().is_empty() {
            result.message_id = input.message_id.clone();
        }
        if result.operation_id.trim().is_empty() {
            result.operation_id = input.operation_id.clone();
        }
        let mut sdk_result =
            input
                .body
                .sdk_result(&result, self.client.did().clone(), input.group.clone())?;
        if let Err(err) =
            input
                .body
                .persist_outgoing(self.client, input.group.as_str(), &sdk_result)
        {
            sdk_result
                .warnings
                .push(format!("Failed to persist local group E2EE message: {err}"));
        }
        Ok(GroupE2eeTextSendResult {
            sdk_result,
            group_did: input.group.as_str().to_owned(),
            operation_id: input.operation_id.clone(),
            message_id: input.message_id.clone(),
            raw,
        })
    }

    fn repair_for_epoch_mismatch(
        &mut self,
        group: &crate::ids::GroupRef,
        credentials: &GroupTextCredentials,
    ) -> crate::ImResult<super::repair::GroupE2eeRepairResult> {
        let repair = super::repair::GroupE2eeRepairRuntime::new(
            self.client,
            &self.session_provider,
            &mut self.transport,
            &self.mls_provider,
        )
        .repair(super::repair::GroupE2eeRepairInput {
            group: group.clone(),
            credentials: Some(credentials.clone()),
            notice_limit: 50,
        })?;
        let mut warnings = repair.warnings.clone();
        warnings.push(
            "group E2EE send saw stale epoch; repaired local notices and retried once".to_owned(),
        );
        Ok(super::repair::GroupE2eeRepairResult {
            warnings: compact_warnings(warnings),
            ..repair
        })
    }
}

impl<'a, P, T, M> GroupE2eeTextSender<'a, P, T, M>
where
    P: AsyncSessionProvider,
    T: AsyncAuthenticatedRpcTransport,
    M: GroupMlsProvider + Clone + Send + 'static,
{
    pub(crate) async fn send_async(
        mut self,
        input: GroupE2eeTextSend,
    ) -> crate::ImResult<GroupE2eeTextSendResult> {
        let group = group_target(&input.request.target)?;
        let (text, kind) = text_body(&input.request.body)?;
        validate_group_e2ee_security(&input.request.security)?;
        self.session_provider
            .ensure_session(crate::auth::AuthScope::GroupMessaging)
            .await?;

        let credentials = match input.credentials {
            Some(credentials) => credentials,
            None => load_credentials_async(self.client).await?,
        };
        let operation_id = input
            .request
            .delivery
            .idempotency_key
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| {
                format!(
                    "op-{}",
                    crate::internal::wire::common::generate_operation_id()
                )
            });
        let message_id = input
            .request
            .client_message_id
            .as_ref()
            .map(|value| value.as_str().to_owned())
            .unwrap_or_else(|| {
                format!(
                    "msg-{}",
                    crate::internal::wire::common::generate_operation_id()
                )
            });
        let prepared = GroupE2eePreparedTextSend {
            group,
            body: GroupE2eeApplicationBody::Text {
                text: text.to_owned(),
                kind,
            },
            operation_id,
            message_id,
            credentials,
            group_state_ref: input.group_state_ref,
            client_context: None,
        };
        match self.send_prepared_async(&prepared, false).await {
            Ok(result) => Ok(result),
            Err(err) => {
                if !is_group_e2ee_epoch_mismatch(&err) {
                    return Err(err);
                }
                let retry = self
                    .repair_for_epoch_mismatch_async(&prepared.group, &prepared.credentials)
                    .await?;
                let retry_prepared = GroupE2eePreparedTextSend {
                    group_state_ref: None,
                    ..prepared
                };
                let mut result = self.send_prepared_async(&retry_prepared, true).await?;
                let mut warnings = retry.warnings;
                warnings.extend(result.sdk_result.warnings);
                result.sdk_result.warnings = compact_warnings(warnings);
                Ok(result)
            }
        }
    }

    pub(crate) async fn send_attachment_async(
        mut self,
        input: GroupE2eeAttachmentSend,
    ) -> crate::ImResult<GroupE2eeTextSendResult> {
        let group = group_target(&input.request.target)?;
        validate_group_e2ee_security(&input.request.security)?;
        self.session_provider
            .ensure_session(crate::auth::AuthScope::GroupMessaging)
            .await?;

        let credentials = match input.credentials {
            Some(credentials) => credentials,
            None => load_credentials_async(self.client).await?,
        };
        let operation_id = input
            .request
            .delivery
            .idempotency_key
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| {
                format!(
                    "op-{}",
                    crate::internal::wire::common::generate_operation_id()
                )
            });
        let message_id = input
            .request
            .client_message_id
            .as_ref()
            .map(|value| value.as_str().to_owned())
            .unwrap_or_else(|| {
                format!(
                    "msg-{}",
                    crate::internal::wire::common::generate_operation_id()
                )
            });
        let prepared = GroupE2eePreparedTextSend {
            group,
            body: GroupE2eeApplicationBody::Attachment {
                full_manifest: input.committed.full_manifest,
                redacted_manifest: input.committed.redacted_manifest,
            },
            operation_id,
            message_id,
            credentials,
            group_state_ref: input.group_state_ref,
            client_context: Some(
                crate::internal::secure_direct::send::attachment_client_context(
                    input.committed.grant_ref,
                ),
            ),
        };
        match self.send_prepared_async(&prepared, false).await {
            Ok(result) => Ok(result),
            Err(err) => {
                if !is_group_e2ee_epoch_mismatch(&err) {
                    return Err(err);
                }
                let retry = self
                    .repair_for_epoch_mismatch_async(&prepared.group, &prepared.credentials)
                    .await?;
                let retry_prepared = GroupE2eePreparedTextSend {
                    group_state_ref: None,
                    ..prepared
                };
                let mut result = self.send_prepared_async(&retry_prepared, true).await?;
                let mut warnings = retry.warnings;
                warnings.extend(result.sdk_result.warnings);
                result.sdk_result.warnings = compact_warnings(warnings);
                Ok(result)
            }
        }
    }

    async fn send_prepared_async(
        &mut self,
        input: &GroupE2eePreparedTextSend,
        retry: bool,
    ) -> crate::ImResult<GroupE2eeTextSendResult> {
        let group_state_ref = match input.group_state_ref.clone() {
            Some(group_state_ref) => group_state_ref,
            None => {
                super::state_ref::resolve_group_state_ref_service_first_async(
                    self.client,
                    &self.session_provider,
                    &mut self.transport,
                    &self.mls_provider,
                    ResolveGroupStateRef {
                        group: input.group.clone(),
                        credentials: Some(input.credentials.clone()),
                    },
                )
                .await?
                .group_state_ref
            }
        };
        let device_id = self
            .client
            .current_identity()
            .device_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(DEFAULT_GROUP_MLS_DEVICE_ID)
            .to_owned();
        let encrypt_input = EncryptInput {
            sender_did: self.client.did().as_str().to_owned(),
            device_id,
            group_state_ref,
            message_id: input.message_id.clone(),
            operation_id: input.operation_id.clone(),
            application_plaintext: input.body.application_plaintext(input.group.as_str()),
            request_id: if retry {
                format!("group-e2ee-encrypt-retry-{}", input.operation_id)
            } else {
                format!("group-e2ee-encrypt-{}", input.operation_id)
            },
        };
        let mls_provider = self.mls_provider.clone();
        let encrypted = crate::internal::runtime::worker::run_blocking(move || {
            mls_provider.encrypt(encrypt_input)
        })
        .await
        .map_err(|err| crate::ImError::Internal {
            message: format!("group E2EE encrypt worker failed: {err}"),
        })??;
        let params = super::wire::build_group_e2ee_send_rpc_params_with_client_context(
            &input.credentials,
            self.client.did().as_str(),
            input.group.as_str(),
            &encrypted.group_cipher_object,
            &input.operation_id,
            &input.message_id,
            input.client_context.clone(),
        )?;
        let raw = self
            .transport
            .authenticated_rpc(
                crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT,
                "group.e2ee.send",
                params,
            )
            .await?;
        let mut result: GroupRpcResult =
            serde_json::from_value(raw.clone()).map_err(|err| crate::ImError::Serialization {
                detail: err.to_string(),
            })?;
        if result.group_did.trim().is_empty() {
            result.group_did = input.group.as_str().to_owned();
        }
        if result.message_id.trim().is_empty() {
            result.message_id = input.message_id.clone();
        }
        if result.operation_id.trim().is_empty() {
            result.operation_id = input.operation_id.clone();
        }
        let mut sdk_result =
            input
                .body
                .sdk_result(&result, self.client.did().clone(), input.group.clone())?;
        if let Err(err) = input
            .body
            .persist_outgoing_async(self.client, input.group.as_str(), &sdk_result)
            .await
        {
            sdk_result
                .warnings
                .push(format!("Failed to persist local group E2EE message: {err}"));
        }
        Ok(GroupE2eeTextSendResult {
            sdk_result,
            group_did: input.group.as_str().to_owned(),
            operation_id: input.operation_id.clone(),
            message_id: input.message_id.clone(),
            raw,
        })
    }

    async fn repair_for_epoch_mismatch_async(
        &mut self,
        group: &crate::ids::GroupRef,
        credentials: &GroupTextCredentials,
    ) -> crate::ImResult<super::repair::GroupE2eeRepairResult> {
        let repair = super::repair::GroupE2eeRepairRuntime::new(
            self.client,
            &self.session_provider,
            &mut self.transport,
            self.mls_provider.clone(),
        )
        .repair_async(super::repair::GroupE2eeRepairInput {
            group: group.clone(),
            credentials: Some(credentials.clone()),
            notice_limit: 50,
        })
        .await?;
        let mut warnings = repair.warnings.clone();
        warnings.push(
            "group E2EE send saw stale epoch; repaired local notices and retried once".to_owned(),
        );
        Ok(super::repair::GroupE2eeRepairResult {
            warnings: compact_warnings(warnings),
            ..repair
        })
    }
}

fn validate_group_e2ee_security(
    security: &crate::messages::MessageSecurityMode,
) -> crate::ImResult<()> {
    match security {
        crate::messages::MessageSecurityMode::E2eeRequired
        | crate::messages::MessageSecurityMode::GroupE2ee => Ok(()),
        crate::messages::MessageSecurityMode::DefaultPlain
        | crate::messages::MessageSecurityMode::Plain => {
            Err(crate::ImError::unsupported("plain-group-e2ee-runtime"))
        }
        crate::messages::MessageSecurityMode::SecureDirect => {
            Err(crate::ImError::unsupported("secure-direct"))
        }
    }
}

pub(crate) fn is_group_e2ee_epoch_mismatch(err: &crate::ImError) -> bool {
    err.to_string()
        .to_ascii_lowercase()
        .contains("epoch mismatch")
}

fn compact_warnings(warnings: Vec<String>) -> Vec<String> {
    let mut compact = Vec::new();
    for warning in warnings {
        let warning = warning.trim().to_owned();
        if warning.is_empty() || compact.iter().any(|known| known == &warning) {
            continue;
        }
        compact.push(warning);
    }
    compact
}

fn sdk_attachment_result_from_group_result(
    result: &GroupRpcResult,
    sender: crate::ids::Did,
    group: crate::ids::GroupRef,
    redacted_manifest: &serde_json::Value,
) -> crate::ImResult<crate::messages::SendMessageResult> {
    let message_id =
        if !result.group_did.trim().is_empty() && !result.group_event_seq.trim().is_empty() {
            crate::ids::MessageId::parse(format!(
                "{}:{}",
                result.group_did.trim(),
                result.group_event_seq.trim()
            ))?
        } else if !result.group_event_seq.trim().is_empty() {
            crate::ids::MessageId::parse(format!(
                "{}:{}",
                group.as_str().trim(),
                result.group_event_seq.trim()
            ))?
        } else if !result.message_id.trim().is_empty() {
            crate::ids::MessageId::parse(&result.message_id)?
        } else {
            crate::ids::MessageId::parse(format!(
                "msg-{}",
                crate::internal::wire::common::generate_operation_id()
            ))?
        };
    let delivery = if result.accepted || result.final_acceptance {
        crate::messages::DeliveryState::Accepted
    } else {
        crate::messages::DeliveryState::Failed {
            reason: "not accepted".to_owned(),
        }
    };
    let (send_state, retry_plan) =
        crate::internal::message_runtime::state::send_state_from_delivery(
            &delivery,
            Some(result.operation_id.clone()).filter(|value| !value.trim().is_empty()),
            Some(message_id.clone()),
            Some(result.accepted_at.clone()).filter(|value| !value.trim().is_empty()),
            None,
        );
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
    attributes.push(crate::messages::MessageMetadataAttribute {
        key: "attachment_manifest".to_string(),
        value: crate::attachments::manifest::manifest_content_string(redacted_manifest),
    });
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
            metadata: crate::messages::MessageMetadata {
                operation_id: Some(result.operation_id.clone())
                    .filter(|value| !value.trim().is_empty()),
                delivery_state: Some(
                    crate::internal::message_runtime::state::send_state_label(&send_state.state)
                        .to_string(),
                ),
                send_state: Some(send_state),
                retry_plan,
                server_sequence: result.group_event_seq.trim().parse().ok(),
                content_type: Some(
                    crate::attachments::manifest::attachment_manifest_content_type().to_string(),
                ),
                attributes,
            },
        },
        delivery,
        warnings: Vec::new(),
    })
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
        GenerateKeyPackageInput, GroupKeyPackageOutput, LeaveGroupInput, PreparedMlsCommitOutput,
        ProcessNoticeInput, ProcessNoticeOutput, ProcessWelcomeInput, ProcessWelcomeOutput,
        RecoverMemberInput, RemoveMemberInput, StatusInput, StatusOutput, UpdateMemberInput,
    };
    use anp::group_e2ee::{GroupCipherObject, GroupStateRef};
    use serde_json::{json, Value};

    use super::*;

    #[test]
    fn group_e2ee_text_sender_encrypts_then_sends_cipher_without_plaintext() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let group_did = "did:example:groups:e2ee";
        let sender = GroupE2eeTextSender::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({
                    "accepted": true,
                    "final_acceptance": true,
                    "group_did": group_did,
                    "message_id": "server-e2ee-message-id",
                    "operation_id": "op-client-e2ee",
                    "group_event_seq": "77",
                    "group_state_version": "11",
                    "accepted_at": "2026-05-21T00:00:00Z"
                }),
                responses: Vec::new(),
            },
            StaticMlsProvider,
        );

        let result = sender
            .send(GroupE2eeTextSend {
                request: group_e2ee_text_request(group_did, "secret group text"),
                group_state_ref: Some(GroupStateRef {
                    group_did: group_did.to_owned(),
                    group_state_version: "10".to_owned(),
                    policy_hash: None,
                }),
                credentials: Some(fixture.credentials()),
            })
            .unwrap();

        assert_eq!(result.group_did, group_did);
        assert_eq!(result.operation_id, "op-client-e2ee");
        assert_eq!(result.sdk_result.message.metadata.server_sequence, Some(77));
        assert!(matches!(
            result.sdk_result.delivery,
            crate::messages::DeliveryState::Accepted
        ));

        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        let call = &calls[0];
        assert_eq!(
            call.endpoint,
            crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT
        );
        assert_eq!(call.method, "group.e2ee.send");
        assert_eq!(call.params["meta"]["profile"], anp::group_e2ee::PROFILE);
        assert_eq!(call.params["meta"]["security_profile"], "group-e2ee");
        assert_eq!(
            call.params["meta"]["content_type"],
            anp::group_e2ee::GROUP_CIPHER_CONTENT_TYPE
        );
        assert_eq!(call.params["meta"]["operation_id"], "op-client-e2ee");
        assert_eq!(
            call.params["body"]["private_message_b64u"],
            "c2VhbGVkLW1scy1jaXBoZXI"
        );
        let encoded = serde_json::to_string(&call.params).unwrap();
        assert!(!encoded.contains("secret group text"));
        assert!(!encoded.contains("application_plaintext"));
        assert!(encoded.contains("origin_proof"));

        let stored = rusqlite::Connection::open(fixture.root.join("local").join("im.sqlite"))
            .unwrap()
            .query_row(
                r#"
SELECT thread_id, group_did, content, is_e2ee, server_seq, metadata
FROM messages
WHERE owner_did = ?1 AND msg_id = ?2"#,
                rusqlite::params![client.did().as_str(), "did:example:groups:e2ee:77"],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(stored.0, "group:did:example:groups:e2ee");
        assert_eq!(stored.1, group_did);
        assert_eq!(stored.2, "secret group text");
        assert_eq!(stored.3, 1);
        assert_eq!(stored.4, 77);
        let metadata: Value = serde_json::from_str(&stored.5).unwrap();
        assert_eq!(metadata["security"], "group-e2ee");
        assert_eq!(metadata["contains_sensitive"], false);
        assert_eq!(metadata["group_event_seq"], "77");
        let metadata_text = stored.5;
        for forbidden in [
            "private_message_b64u",
            "crypto_group_id",
            "epoch_authenticator",
            "OpenMLS",
            "KeyPackage",
            "commit_b64u",
            "welcome_b64u",
            "cipher",
        ] {
            assert!(
                !metadata_text.contains(forbidden),
                "metadata leaked {forbidden}: {metadata_text}"
            );
        }
    }

    #[test]
    fn secure_attachment_send_group_sender_encrypts_manifest_and_sends_non_secret_grant_ref() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let group_did = "did:example:groups:e2ee";
        let committed = committed_attachment("group-secret.pdf", "group caption");
        let object_key = committed.full_manifest["attachments"][0]["encryption_info"]
            ["object_key_b64u"]
            .as_str()
            .unwrap()
            .to_owned();
        let nonce = committed.full_manifest["attachments"][0]["encryption_info"]["nonce_b64u"]
            .as_str()
            .unwrap()
            .to_owned();
        let provider = AttachmentMlsProvider {
            expected_object_key: object_key.clone(),
            expected_nonce: nonce.clone(),
        };
        let sender = GroupE2eeTextSender::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({
                    "accepted": true,
                    "final_acceptance": true,
                    "group_did": group_did,
                    "message_id": "server-e2ee-attachment-id",
                    "operation_id": "op-client-e2ee-attachment",
                    "group_event_seq": "88",
                    "group_state_version": "11",
                    "accepted_at": "2026-05-21T00:00:00Z"
                }),
                responses: Vec::new(),
            },
            provider,
        );

        let result = sender
            .send_attachment(GroupE2eeAttachmentSend {
                request: group_e2ee_attachment_request(group_did),
                group_state_ref: Some(GroupStateRef {
                    group_did: group_did.to_owned(),
                    group_state_version: "10".to_owned(),
                    policy_hash: None,
                }),
                credentials: Some(fixture.credentials()),
                committed,
            })
            .unwrap();

        assert_eq!(result.group_did, group_did);
        assert_eq!(result.operation_id, "op-client-e2ee-attachment");
        assert_eq!(result.sdk_result.message.metadata.server_sequence, Some(88));
        assert!(matches!(
            result.sdk_result.message.body,
            crate::messages::MessageBodyView::Unsupported { content_type }
                if content_type.as_deref()
                    == Some(crate::attachments::manifest::attachment_manifest_content_type())
        ));

        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        let call = &calls[0];
        assert_eq!(call.method, "group.e2ee.send");
        assert_eq!(
            call.params["meta"]["content_type"],
            anp::group_e2ee::GROUP_CIPHER_CONTENT_TYPE
        );
        let outer_text = serde_json::to_string(&call.params["body"]).unwrap();
        assert!(!outer_text.contains(&object_key));
        assert!(!outer_text.contains(&nonce));
        assert!(!outer_text.contains("object_key_b64u"));
        assert!(!outer_text.contains("nonce_b64u"));
        let grant_refs = call.params["client"]["attachment_grant_refs"]
            .as_array()
            .unwrap();
        assert_eq!(grant_refs.len(), 1);
        assert_eq!(grant_refs[0]["attachment_id"], "att-group-secure-1");
        assert_eq!(grant_refs[0]["object_encryption_mode"], "object-e2ee");
        assert_eq!(grant_refs[0]["plaintext_size"], "23");
        let grant_text = serde_json::to_string(&call.params["client"]).unwrap();
        assert!(!grant_text.contains(&object_key));
        assert!(!grant_text.contains(&nonce));
        assert!(!grant_text.contains("object_key_b64u"));
        assert!(!grant_text.contains("nonce_b64u"));

        let stored = rusqlite::Connection::open(fixture.root.join("local").join("im.sqlite"))
            .unwrap()
            .query_row(
                r#"
SELECT content, is_e2ee, metadata
FROM messages
WHERE owner_did = ?1 AND msg_id = ?2"#,
                rusqlite::params![client.did().as_str(), "did:example:groups:e2ee:88"],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(stored.1, 1);
        assert!(stored.0.contains("object-e2ee"));
        assert!(!stored.0.contains(&object_key));
        assert!(!stored.0.contains(&nonce));
        assert!(!stored.0.contains("object_key_b64u"));
        assert!(!stored.0.contains("nonce_b64u"));
        assert!(!stored.2.contains(&object_key));
        assert!(!stored.2.contains(&nonce));
        assert!(!stored.2.contains("object_key_b64u"));
        assert!(!stored.2.contains("nonce_b64u"));
    }

    #[test]
    fn group_e2ee_text_sender_resolves_state_ref_before_encrypting() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let group_did = "did:example:groups:e2ee";
        let sender = GroupE2eeTextSender::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({}),
                responses: vec![
                    (
                        "group.e2ee.head".to_owned(),
                        json!({
                            "group_state_ref": {
                                "group_did": group_did,
                                "group_state_version": "service-state-12"
                            }
                        }),
                    ),
                    (
                        "group.e2ee.send".to_owned(),
                        json!({
                            "accepted": true,
                            "final_acceptance": true,
                            "group_did": group_did,
                            "message_id": "server-e2ee-message-id",
                            "operation_id": "op-client-e2ee",
                            "group_event_seq": "78",
                            "group_state_version": "12",
                            "accepted_at": "2026-05-21T00:00:00Z"
                        }),
                    ),
                ],
            },
            ResolvingMlsProvider,
        );

        let result = sender
            .send(GroupE2eeTextSend {
                request: group_e2ee_text_request(group_did, "resolved group text"),
                group_state_ref: None,
                credentials: Some(fixture.credentials()),
            })
            .unwrap();

        assert_eq!(result.sdk_result.message.metadata.server_sequence, Some(78));
        let calls = calls.borrow();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].method, "group.e2ee.head");
        assert_eq!(calls[1].method, "group.e2ee.send");
        assert_eq!(
            calls[1].params["body"]["group_state_ref"]["group_state_version"],
            "service-state-12"
        );
        let encoded = serde_json::to_string(&calls[1].params).unwrap();
        assert!(!encoded.contains("resolved group text"));
        assert!(!encoded.contains("application_plaintext"));
    }

    #[test]
    fn group_e2ee_text_sender_repairs_epoch_mismatch_and_retries_once() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let group_did = "did:example:groups:e2ee";
        let provider = RepairingMlsProvider::new();
        let processed = Rc::clone(&provider.processed_notices);
        let sender = GroupE2eeTextSender::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({}),
                responses: vec![
                    (
                        "group.e2ee.head".to_owned(),
                        json!({
                            "group_state_ref": {
                                "group_did": group_did,
                                "group_state_version": "service-state-stale"
                            },
                            "epoch": "1"
                        }),
                    ),
                    (
                        "group.e2ee.send".to_owned(),
                        json_rpc_service_error("group.e2ee.send epoch mismatch"),
                    ),
                    (
                        "group.e2ee.head".to_owned(),
                        json!({
                            "group_state_ref": {
                                "group_did": group_did,
                                "group_state_version": "service-state-current"
                            },
                            "epoch": "2"
                        }),
                    ),
                    (
                        "group.e2ee.notice".to_owned(),
                        json!({
                            "notices": [{
                                "notice_id": "notice-commit-2",
                                "notice_type": "commit-delivery",
                                "group_did": group_did,
                                "recipient_did": "did:example:alice",
                                "device_id": DEFAULT_GROUP_MLS_DEVICE_ID,
                                "commit_b64u": "COMMIT",
                                "from_epoch": "1",
                                "to_epoch": "2",
                                "group_state_version": "service-state-current"
                            }]
                        }),
                    ),
                    (
                        "group.e2ee.notice".to_owned(),
                        json!({
                            "notices": []
                        }),
                    ),
                    (
                        "group.e2ee.head".to_owned(),
                        json!({
                            "group_state_ref": {
                                "group_did": group_did,
                                "group_state_version": "service-state-current"
                            },
                            "epoch": "2"
                        }),
                    ),
                    (
                        "group.e2ee.notice".to_owned(),
                        json!({
                            "notices": []
                        }),
                    ),
                    (
                        "group.e2ee.head".to_owned(),
                        json!({
                            "group_state_ref": {
                                "group_did": group_did,
                                "group_state_version": "service-state-current"
                            },
                            "epoch": "2"
                        }),
                    ),
                    (
                        "group.e2ee.send".to_owned(),
                        json!({
                            "accepted": true,
                            "final_acceptance": true,
                            "group_did": group_did,
                            "message_id": "server-e2ee-message-id",
                            "operation_id": "op-client-e2ee",
                            "group_event_seq": "79",
                            "group_state_version": "2",
                            "accepted_at": "2026-05-21T00:00:00Z"
                        }),
                    ),
                ],
            },
            provider,
        );

        let result = sender
            .send(GroupE2eeTextSend {
                request: group_e2ee_text_request(group_did, "retry group text"),
                group_state_ref: None,
                credentials: Some(fixture.credentials()),
            })
            .unwrap();

        assert_eq!(result.sdk_result.message.metadata.server_sequence, Some(79));
        assert!(result
            .sdk_result
            .warnings
            .iter()
            .any(|warning| warning.contains("retried once")));
        assert_eq!(processed.borrow().as_slice(), ["did:example:groups:e2ee:1"]);
        let calls = calls.borrow();
        let methods = calls
            .iter()
            .map(|call| call.method.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            methods,
            vec![
                "group.e2ee.head",
                "group.e2ee.send",
                "group.e2ee.head",
                "group.e2ee.notice",
                "group.e2ee.notice",
                "group.e2ee.head",
                "group.e2ee.notice",
                "group.e2ee.head",
                "group.e2ee.send"
            ]
        );
        assert_eq!(
            calls[1].params["body"]["group_state_ref"]["group_state_version"],
            "service-state-stale"
        );
        assert_eq!(
            calls[8].params["body"]["group_state_ref"]["group_state_version"],
            "service-state-current"
        );
        let encoded_retry = serde_json::to_string(&calls[8].params).unwrap();
        assert!(!encoded_retry.contains("retry group text"));
        assert!(!encoded_retry.contains("application_plaintext"));
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
            unreachable!("group E2EE sender should not refresh through the session provider")
        }

        fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
            unreachable!("group E2EE sender should not read status")
        }
    }

    struct RecordingTransport {
        calls: Rc<RefCell<Vec<RecordedCall>>>,
        response: Value,
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
            Ok(self.response.clone())
        }
    }

    struct RecordedCall {
        endpoint: String,
        method: String,
        params: Value,
    }

    struct StaticMlsProvider;

    impl GroupMlsProvider for StaticMlsProvider {
        fn encrypt(&self, input: EncryptInput) -> crate::ImResult<EncryptOutput> {
            assert_eq!(input.sender_did, "did:example:alice");
            assert_eq!(input.operation_id, "op-client-e2ee");
            assert_eq!(input.message_id, "msg-client-e2ee");
            assert_eq!(
                input.application_plaintext.text.as_deref(),
                Some("secret group text")
            );
            Ok(EncryptOutput {
                group_cipher_object: GroupCipherObject {
                    crypto_group_id_b64u: "Y3J5cHRvLWdyb3Vw".to_owned(),
                    epoch: "10".to_owned(),
                    private_message_b64u: "c2VhbGVkLW1scy1jaXBoZXI".to_owned(),
                    group_state_ref: input.group_state_ref,
                    epoch_authenticator: Some("YXV0aGVudGljYXRvcg".to_owned()),
                    non_cryptographic: false,
                    artifact_mode: None,
                },
                authenticated_data_sha256_b64u: "YWFkLWRpZ2VzdA".to_owned(),
            })
        }

        fn generate_key_package(
            &self,
            _input: GenerateKeyPackageInput,
        ) -> crate::ImResult<GroupKeyPackageOutput> {
            unreachable!("send should not generate key packages")
        }

        fn create_group_prepare(
            &self,
            _input: CreateGroupInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("send should not create groups")
        }

        fn add_member_prepare(
            &self,
            _input: AddMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("send should not add members")
        }

        fn remove_member_prepare(
            &self,
            _input: RemoveMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("send should not remove members")
        }

        fn leave_prepare(
            &self,
            _input: LeaveGroupInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("send should not leave groups")
        }

        fn update_member_prepare(
            &self,
            _input: UpdateMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("send should not update members")
        }

        fn recover_member_prepare(
            &self,
            _input: RecoverMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("send should not recover members")
        }

        fn finalize_commit(
            &self,
            _input: FinalizeCommitInput,
        ) -> crate::ImResult<FinalizeCommitOutput> {
            unreachable!("send should not finalize commits")
        }

        fn abort_commit(&self, _input: AbortCommitInput) -> crate::ImResult<AbortCommitOutput> {
            unreachable!("send should not abort commits")
        }

        fn process_welcome(
            &self,
            _input: ProcessWelcomeInput,
        ) -> crate::ImResult<ProcessWelcomeOutput> {
            unreachable!("send should not process welcomes")
        }

        fn process_notice(
            &self,
            _input: ProcessNoticeInput,
        ) -> crate::ImResult<ProcessNoticeOutput> {
            unreachable!("send should not process notices")
        }

        fn decrypt(&self, _input: DecryptInput) -> crate::ImResult<DecryptOutput> {
            unreachable!("send should not decrypt")
        }

        fn status(&self, _input: StatusInput) -> crate::ImResult<StatusOutput> {
            unreachable!("send should not read status")
        }
    }

    struct AttachmentMlsProvider {
        expected_object_key: String,
        expected_nonce: String,
    }

    impl GroupMlsProvider for AttachmentMlsProvider {
        fn encrypt(&self, input: EncryptInput) -> crate::ImResult<EncryptOutput> {
            assert_eq!(input.sender_did, "did:example:alice");
            assert_eq!(input.operation_id, "op-client-e2ee-attachment");
            assert_eq!(input.message_id, "msg-client-e2ee-attachment");
            assert_eq!(
                input.application_plaintext.application_content_type,
                crate::attachments::manifest::attachment_manifest_content_type()
            );
            assert!(input.application_plaintext.text.is_none());
            let payload = input.application_plaintext.payload.as_ref().unwrap();
            assert_eq!(payload["primary_attachment_id"], "att-group-secure-1");
            assert_eq!(
                payload["attachments"][0]["encryption_info"]["object_key_b64u"],
                self.expected_object_key
            );
            assert_eq!(
                payload["attachments"][0]["encryption_info"]["nonce_b64u"],
                self.expected_nonce
            );
            Ok(EncryptOutput {
                group_cipher_object: GroupCipherObject {
                    crypto_group_id_b64u: "Y3J5cHRvLWdyb3Vw".to_owned(),
                    epoch: "10".to_owned(),
                    private_message_b64u: "YXR0YWNobWVudC1jaXBoZXI".to_owned(),
                    group_state_ref: input.group_state_ref,
                    epoch_authenticator: Some("YXV0aGVudGljYXRvcg".to_owned()),
                    non_cryptographic: false,
                    artifact_mode: None,
                },
                authenticated_data_sha256_b64u: "YWFkLWRpZ2VzdA".to_owned(),
            })
        }

        fn generate_key_package(
            &self,
            _input: GenerateKeyPackageInput,
        ) -> crate::ImResult<GroupKeyPackageOutput> {
            unreachable!("send should not generate key packages")
        }

        fn create_group_prepare(
            &self,
            _input: CreateGroupInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("send should not create groups")
        }

        fn add_member_prepare(
            &self,
            _input: AddMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("send should not add members")
        }

        fn remove_member_prepare(
            &self,
            _input: RemoveMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("send should not remove members")
        }

        fn leave_prepare(
            &self,
            _input: LeaveGroupInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("send should not leave groups")
        }

        fn update_member_prepare(
            &self,
            _input: UpdateMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("send should not update members")
        }

        fn recover_member_prepare(
            &self,
            _input: RecoverMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("send should not recover members")
        }

        fn finalize_commit(
            &self,
            _input: FinalizeCommitInput,
        ) -> crate::ImResult<FinalizeCommitOutput> {
            unreachable!("send should not finalize commits")
        }

        fn abort_commit(&self, _input: AbortCommitInput) -> crate::ImResult<AbortCommitOutput> {
            unreachable!("send should not abort commits")
        }

        fn process_welcome(
            &self,
            _input: ProcessWelcomeInput,
        ) -> crate::ImResult<ProcessWelcomeOutput> {
            unreachable!("send should not process welcomes")
        }

        fn process_notice(
            &self,
            _input: ProcessNoticeInput,
        ) -> crate::ImResult<ProcessNoticeOutput> {
            unreachable!("send should not process notices")
        }

        fn decrypt(&self, _input: DecryptInput) -> crate::ImResult<DecryptOutput> {
            unreachable!("send should not decrypt")
        }

        fn status(&self, _input: StatusInput) -> crate::ImResult<StatusOutput> {
            unreachable!("send should not read status")
        }
    }

    struct ResolvingMlsProvider;

    impl GroupMlsProvider for ResolvingMlsProvider {
        fn encrypt(&self, input: EncryptInput) -> crate::ImResult<EncryptOutput> {
            assert_eq!(
                input.group_state_ref.group_state_version,
                "service-state-12"
            );
            assert_eq!(
                input.application_plaintext.text.as_deref(),
                Some("resolved group text")
            );
            Ok(EncryptOutput {
                group_cipher_object: GroupCipherObject {
                    crypto_group_id_b64u: "Y3J5cHRvLWdyb3Vw".to_owned(),
                    epoch: "12".to_owned(),
                    private_message_b64u: "cmVzb2x2ZWQtY2lwaGVy".to_owned(),
                    group_state_ref: input.group_state_ref,
                    epoch_authenticator: Some("YXV0aGVudGljYXRvcg".to_owned()),
                    non_cryptographic: false,
                    artifact_mode: None,
                },
                authenticated_data_sha256_b64u: "YWFkLWRpZ2VzdA".to_owned(),
            })
        }

        fn status(&self, input: StatusInput) -> crate::ImResult<StatusOutput> {
            assert_eq!(input.agent_did.as_deref(), Some("did:example:alice"));
            assert_eq!(input.group_did.as_deref(), Some("did:example:groups:e2ee"));
            Ok(StatusOutput {
                status: "active".to_owned(),
                epoch: Some("12".to_owned()),
                local_epoch: Some("12".to_owned()),
                pending_commits: Vec::new(),
                epoch_authenticator: Some("YXV0aGVudGljYXRvcg".to_owned()),
            })
        }

        fn generate_key_package(
            &self,
            _input: GenerateKeyPackageInput,
        ) -> crate::ImResult<GroupKeyPackageOutput> {
            unreachable!("send should not generate key packages")
        }

        fn create_group_prepare(
            &self,
            _input: CreateGroupInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("send should not create groups")
        }

        fn add_member_prepare(
            &self,
            _input: AddMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("send should not add members")
        }

        fn remove_member_prepare(
            &self,
            _input: RemoveMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("send should not remove members")
        }

        fn leave_prepare(
            &self,
            _input: LeaveGroupInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("send should not leave groups")
        }

        fn update_member_prepare(
            &self,
            _input: UpdateMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("send should not update members")
        }

        fn recover_member_prepare(
            &self,
            _input: RecoverMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("send should not recover members")
        }

        fn finalize_commit(
            &self,
            _input: FinalizeCommitInput,
        ) -> crate::ImResult<FinalizeCommitOutput> {
            unreachable!("send should not finalize commits")
        }

        fn abort_commit(&self, _input: AbortCommitInput) -> crate::ImResult<AbortCommitOutput> {
            unreachable!("send should not abort commits")
        }

        fn process_welcome(
            &self,
            _input: ProcessWelcomeInput,
        ) -> crate::ImResult<ProcessWelcomeOutput> {
            unreachable!("send should not process welcomes")
        }

        fn process_notice(
            &self,
            _input: ProcessNoticeInput,
        ) -> crate::ImResult<ProcessNoticeOutput> {
            unreachable!("send should not process notices")
        }

        fn decrypt(&self, _input: DecryptInput) -> crate::ImResult<DecryptOutput> {
            unreachable!("send should not decrypt")
        }
    }

    #[derive(Clone)]
    struct RepairingMlsProvider {
        status: Rc<RefCell<StatusOutput>>,
        processed_notices: Rc<RefCell<Vec<String>>>,
    }

    impl RepairingMlsProvider {
        fn new() -> Self {
            Self {
                status: Rc::new(RefCell::new(StatusOutput {
                    status: "active".to_owned(),
                    epoch: Some("1".to_owned()),
                    local_epoch: Some("1".to_owned()),
                    pending_commits: Vec::new(),
                    epoch_authenticator: None,
                })),
                processed_notices: Rc::new(RefCell::new(Vec::new())),
            }
        }
    }

    impl GroupMlsProvider for RepairingMlsProvider {
        fn encrypt(&self, input: EncryptInput) -> crate::ImResult<EncryptOutput> {
            assert_eq!(
                input.application_plaintext.text.as_deref(),
                Some("retry group text")
            );
            Ok(EncryptOutput {
                group_cipher_object: GroupCipherObject {
                    crypto_group_id_b64u: "Y3J5cHRvLWdyb3Vw".to_owned(),
                    epoch: self
                        .status
                        .borrow()
                        .local_epoch
                        .clone()
                        .unwrap_or_else(|| "1".to_owned()),
                    private_message_b64u: "cmV0cnktY2lwaGVy".to_owned(),
                    group_state_ref: input.group_state_ref,
                    epoch_authenticator: Some("YXV0aGVudGljYXRvcg".to_owned()),
                    non_cryptographic: false,
                    artifact_mode: None,
                },
                authenticated_data_sha256_b64u: "YWFkLWRpZ2VzdA".to_owned(),
            })
        }

        fn status(&self, _input: StatusInput) -> crate::ImResult<StatusOutput> {
            Ok(self.status.borrow().clone())
        }

        fn process_notice(
            &self,
            input: ProcessNoticeInput,
        ) -> crate::ImResult<ProcessNoticeOutput> {
            self.processed_notices
                .borrow_mut()
                .push(format!("{}:{}", input.group_did, input.from_epoch));
            let mut status = self.status.borrow_mut();
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
            unreachable!("send should not generate key packages")
        }

        fn create_group_prepare(
            &self,
            _input: CreateGroupInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("send should not create groups")
        }

        fn add_member_prepare(
            &self,
            _input: AddMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("send should not add members")
        }

        fn remove_member_prepare(
            &self,
            _input: RemoveMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("send should not remove members")
        }

        fn leave_prepare(
            &self,
            _input: LeaveGroupInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("send should not leave groups")
        }

        fn update_member_prepare(
            &self,
            _input: UpdateMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("send should not update members")
        }

        fn recover_member_prepare(
            &self,
            _input: RecoverMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("send should not recover members")
        }

        fn finalize_commit(
            &self,
            _input: FinalizeCommitInput,
        ) -> crate::ImResult<FinalizeCommitOutput> {
            unreachable!("send should not finalize commits in this retry test")
        }

        fn abort_commit(&self, _input: AbortCommitInput) -> crate::ImResult<AbortCommitOutput> {
            unreachable!("send should not abort commits")
        }

        fn process_welcome(
            &self,
            _input: ProcessWelcomeInput,
        ) -> crate::ImResult<ProcessWelcomeOutput> {
            unreachable!("retry test should process commit notices only")
        }

        fn decrypt(&self, _input: DecryptInput) -> crate::ImResult<DecryptOutput> {
            unreachable!("send should not decrypt")
        }
    }

    fn json_rpc_service_error(message: &str) -> Value {
        json!({
            "error": {
                "code": "epoch_mismatch",
                "message": message
            }
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
                    challenge: Some("group-e2ee-runtime-test".to_owned()),
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

    fn group_e2ee_text_request(group: &str, text: &str) -> crate::messages::SendMessageRequest {
        crate::messages::SendMessageRequest {
            target: crate::messages::MessageTarget::Group(
                crate::ids::GroupRef::parse(group).unwrap(),
            ),
            body: crate::messages::MessageBody::Text {
                text: text.to_owned(),
                kind: crate::messages::MessageKind::Text,
            },
            security: crate::messages::MessageSecurityMode::GroupE2ee,
            client_message_id: Some(crate::ids::MessageId::parse("msg-client-e2ee").unwrap()),
            delivery: crate::messages::MessageDeliveryOptions {
                idempotency_key: Some("op-client-e2ee".to_owned()),
                wait_for_final_acceptance: true,
            },
        }
    }

    fn group_e2ee_attachment_request(group: &str) -> crate::messages::SendMessageRequest {
        crate::messages::SendMessageRequest {
            target: crate::messages::MessageTarget::Group(
                crate::ids::GroupRef::parse(group).unwrap(),
            ),
            body: crate::messages::MessageBody::Attachment {
                input: crate::attachments::AttachmentInput::Bytes {
                    filename: Some("group-secret.pdf".to_owned()),
                    mime_type: Some("application/pdf".to_owned()),
                    bytes: b"group attachment secret".to_vec(),
                },
                caption: Some("group caption".to_owned()),
                mime_type: Some("application/pdf".to_owned()),
                filename: None,
            },
            security: crate::messages::MessageSecurityMode::GroupE2ee,
            client_message_id: Some(
                crate::ids::MessageId::parse("msg-client-e2ee-attachment").unwrap(),
            ),
            delivery: crate::messages::MessageDeliveryOptions {
                idempotency_key: Some("op-client-e2ee-attachment".to_owned()),
                wait_for_final_acceptance: true,
            },
        }
    }

    fn committed_attachment(
        filename: &str,
        caption: &str,
    ) -> crate::internal::attachment_runtime::upload::PreparedCommittedAttachment {
        let e2ee = crate::attachments::manifest::prepare_object_e2ee_attachment_payload(
            filename,
            "application/pdf",
            b"group attachment secret".to_vec(),
        )
        .unwrap();
        let slot = crate::internal::wire::attachment::AttachmentCreateSlotResult {
            attachment_id: "att-group-secure-1".to_owned(),
            slot_id: "slot-group-secure-1".to_owned(),
            upload_uri: "https://upload.example/slot-group-secure-1".to_owned(),
            upload_headers: serde_json::Map::new(),
            object_uri: "https://objects.example/att-group-secure-1".to_owned(),
            commit_token: "commit-group-secure-1".to_owned(),
            expires_at: "2026-05-24T00:00:00Z".to_owned(),
            request_service_did: "did:example:message-service".to_owned(),
        };
        let descriptor = crate::attachments::manifest::AttachmentDescriptor::from_prepared(
            &e2ee.prepared,
            slot.attachment_id.clone(),
            slot.object_uri.clone(),
        );
        let redacted_manifest =
            crate::attachments::manifest::build_attachment_manifest(&descriptor, caption);
        let full_manifest =
            crate::attachments::manifest::build_attachment_manifest_with_object_e2ee_secrets(
                &descriptor,
                caption,
                &e2ee.secrets,
            );
        let grant_ref = crate::attachments::manifest::build_attachment_grant_ref(&descriptor)
            .expect("grant ref");
        crate::internal::attachment_runtime::upload::PreparedCommittedAttachment {
            target_kind: "group",
            target_did: "did:example:groups:e2ee".to_owned(),
            prepared: e2ee.prepared,
            slot,
            descriptor,
            redacted_manifest,
            full_manifest,
            grant_ref,
        }
    }

    fn unique_temp_root() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "im-core-group-e2ee-runtime-{}-{nanos}",
            std::process::id()
        ))
    }
}
