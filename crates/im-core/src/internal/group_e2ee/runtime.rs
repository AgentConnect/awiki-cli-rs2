use anp::group_e2ee::operations::EncryptInput;
use anp::group_e2ee::{GroupApplicationPlaintext, GroupStateRef};

use crate::internal::auth::session::{AsyncSessionProvider, SessionProvider};
use crate::internal::message_runtime::group::{
    content_type_for_message_type, group_target, load_credentials, load_credentials_async,
    message_type, sdk_result_from_group_result, sdk_text_result_from_group_result, GroupRpcResult,
    GroupTextCredentials, OutgoingGroupBody,
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
    Payload {
        payload: serde_json::Value,
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
            Self::Payload { payload } => GroupApplicationPlaintext {
                application_content_type: "application/json".to_owned(),
                thread_id: Some(group_did.to_owned()),
                reply_to_message_id: None,
                annotations: Default::default(),
                text: None,
                payload: Some(payload.clone()),
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
            Self::Payload { payload } => sdk_result_from_group_result(
                result,
                sender,
                group,
                &OutgoingGroupBody::Payload {
                    payload: payload.clone(),
                },
            ),
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
            Self::Payload { payload } => {
                crate::internal::message_runtime::local_projection::persist_group_e2ee_payload_outgoing(
                    client,
                    group_did,
                    payload,
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
            Self::Payload { payload } => {
                crate::internal::message_runtime::local_projection::persist_group_e2ee_payload_outgoing_async(
                    client,
                    group_did,
                    payload,
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

fn group_e2ee_application_body(
    body: &crate::messages::MessageBody,
) -> crate::ImResult<GroupE2eeApplicationBody> {
    match body {
        crate::messages::MessageBody::Text { text, .. } if text.trim().is_empty() => {
            Err(crate::ImError::invalid_input(
                Some("text".to_owned()),
                "text message must not be empty",
            ))
        }
        crate::messages::MessageBody::Text { text, kind } => Ok(GroupE2eeApplicationBody::Text {
            text: text.clone(),
            kind: kind.clone(),
        }),
        crate::messages::MessageBody::Payload { payload } if !payload.is_object() => {
            Err(crate::ImError::invalid_input(
                Some("payload".to_owned()),
                "message payload must be a JSON object",
            ))
        }
        crate::messages::MessageBody::Payload { payload } => {
            Ok(GroupE2eeApplicationBody::Payload {
                payload: payload.clone(),
            })
        }
        crate::messages::MessageBody::Attachment { .. } => {
            Err(crate::ImError::unsupported("attachments"))
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
        let body = group_e2ee_application_body(&input.request.body)?;
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
            body,
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
        match input
            .body
            .persist_outgoing(self.client, input.group.as_str(), &sdk_result)
        {
            Ok(()) => self
                .client
                .emit_committed_local_message_projection("local_send"),
            Err(err) => {
                sdk_result
                    .warnings
                    .push(format!("Failed to persist local group E2EE message: {err}"));
            }
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
        let body = group_e2ee_application_body(&input.request.body)?;
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
            body,
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
        match input
            .body
            .persist_outgoing_async(self.client, input.group.as_str(), &sdk_result)
            .await
        {
            Ok(()) => self
                .client
                .emit_committed_local_message_projection("local_send"),
            Err(err) => {
                sdk_result
                    .warnings
                    .push(format!("Failed to persist local group E2EE message: {err}"));
            }
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
        key: "security".to_string(),
        value: "group-e2ee".to_string(),
    });
    attributes.push(crate::messages::MessageMetadataAttribute {
        key: "message_security_profile".to_string(),
        value: "group-e2ee".to_string(),
    });
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
            group: Some(group.clone()),
            body: crate::messages::MessageBodyView::Unsupported {
                content_type: Some(
                    crate::attachments::manifest::attachment_manifest_content_type().to_string(),
                ),
            },
            sent_at: Some(result.accepted_at.clone()).filter(|value| !value.trim().is_empty()),
            received_at: None,
            metadata: crate::messages::MessageMetadata {
                conversation_identity: Some(
                    crate::messages::ConversationIdentity::from_thread_ref(
                        &crate::messages::ThreadRef::Group(group.clone()),
                    ),
                ),
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
mod tests;
