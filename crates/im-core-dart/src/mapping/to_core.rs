use std::path::PathBuf;

use crate::dto::{
    attachment::{
        DartAttachmentDestination, DartAttachmentInput, DartAttachmentSendRequest,
        DartDownloadAttachmentRequest, DartSendConversationAttachmentRequest,
    },
    auth::DartAuthScope,
    config::{DartImCoreConfig, DartImCorePaths, DartMessageTransportPolicy},
    directory::DartIdentitySubject,
    email::DartSendEmailRequest,
    error::DartImError,
    group::DartCreateGroupRequest,
    identity::DartIdentitySelector,
    message::{
        DartConversationReadRef, DartDelegatedSigningOptions, DartInboxAuth,
        DartInboxHistoryOptions, DartMarkConversationReadRequest, DartMessageSecurityMode,
        DartMessageTarget, DartScopedInboxToken, DartSendConversationPayloadRequest,
        DartSendConversationTextRequest, DartSendPayloadRequest, DartSendTextRequest,
        DartSyncConversationAfterRequest, DartSyncDeltaRequest, DartSyncThreadAfterRequest,
        DartThreadRef,
    },
    profile::DartProfilePatch,
    realtime::DartRealtimeOptions,
};

impl TryFrom<DartImCoreConfig> for im_core::ImCoreConfig {
    type Error = DartImError;

    fn try_from(value: DartImCoreConfig) -> Result<Self, Self::Error> {
        let mut config = im_core::ImCoreConfig::new(
            im_core::ServiceEndpoint::parse(value.service_base_url).map_err(DartImError::from)?,
            value.did_domain,
        )
        .map_err(DartImError::from)?;
        config.client_version_info = value
            .client_version_info
            .map(|info| {
                im_core::ClientVersionInfo::new(
                    info.product,
                    info.release,
                    info.version,
                    info.build,
                )
            })
            .transpose()
            .map_err(DartImError::from)?;
        config.user_service_endpoint = parse_endpoint(value.user_service_endpoint)?;
        config.message_service_endpoint = parse_endpoint(value.message_service_endpoint)?;
        config.mail_service_endpoint = parse_endpoint(value.mail_service_endpoint)?;
        config.anp_service_endpoint = parse_endpoint(value.anp_service_endpoint)?;
        config.anp_service_did = value
            .anp_service_did
            .map(im_core::ids::Did::parse)
            .transpose()
            .map_err(DartImError::from)?;
        config.transport_policy = value.transport_policy.into();
        Ok(config)
    }
}

impl TryFrom<DartSendEmailRequest> for im_core::email::SendEmailRequest {
    type Error = DartImError;

    fn try_from(value: DartSendEmailRequest) -> Result<Self, Self::Error> {
        Ok(Self {
            to: parse_email_addresses(value.to)?,
            cc: parse_email_addresses(value.cc)?,
            subject: value.subject,
            body_text: value.body_text,
            body_html: value.body_html,
        })
    }
}

fn parse_email_addresses(
    values: Vec<String>,
) -> Result<Vec<im_core::email::EmailAddress>, DartImError> {
    values
        .into_iter()
        .map(im_core::email::EmailAddress::parse)
        .collect::<Result<Vec<_>, _>>()
        .map_err(DartImError::from)
}

fn parse_endpoint(value: Option<String>) -> Result<Option<im_core::ServiceEndpoint>, DartImError> {
    value
        .map(im_core::ServiceEndpoint::parse)
        .transpose()
        .map_err(DartImError::from)
}

impl From<DartMessageTransportPolicy> for im_core::MessageTransportPolicy {
    fn from(value: DartMessageTransportPolicy) -> Self {
        match value {
            DartMessageTransportPolicy::Auto => Self::Auto,
            DartMessageTransportPolicy::HttpOnly => Self::HttpOnly,
            DartMessageTransportPolicy::RealtimePreferred => Self::RealtimePreferred,
        }
    }
}

impl TryFrom<DartRealtimeOptions> for im_core::realtime::RealtimeOptions {
    type Error = DartImError;

    fn try_from(value: DartRealtimeOptions) -> Result<Self, Self::Error> {
        let reconnect = match value.reconnect.as_str() {
            "disabled" | "" => im_core::realtime::ReconnectPolicy::Disabled,
            "fixed" => im_core::realtime::ReconnectPolicy::Fixed {
                delay_ms: value.reconnect_delay_ms.unwrap_or(1000),
                max_attempts: value.reconnect_max_attempts,
            },
            "exponential" => im_core::realtime::ReconnectPolicy::Exponential {
                base_delay_ms: value.reconnect_base_delay_ms.unwrap_or(1000),
                max_delay_ms: value.reconnect_max_delay_ms.unwrap_or(30_000),
                max_attempts: value.reconnect_max_attempts,
            },
            other => {
                return Err(DartImError::invalid_input(
                    Some("reconnect".to_string()),
                    format!("unsupported realtime reconnect policy: {other}"),
                ));
            }
        };
        Ok(im_core::realtime::RealtimeOptions {
            reconnect,
            event_buffer: value.event_buffer.max(1) as usize,
            subscriptions: value
                .subscriptions
                .into_iter()
                .map(realtime_subscription_from_string)
                .collect::<Result<Vec<_>, _>>()?,
        })
    }
}

fn realtime_subscription_from_string(
    value: String,
) -> Result<im_core::realtime::RealtimeSubscription, DartImError> {
    match value.as_str() {
        "messages" => Ok(im_core::realtime::RealtimeSubscription::Messages),
        "groups" => Ok(im_core::realtime::RealtimeSubscription::Groups),
        "notifications" => Ok(im_core::realtime::RealtimeSubscription::Notifications),
        "host_notifications" => Ok(im_core::realtime::RealtimeSubscription::HostNotifications),
        other => Err(DartImError::invalid_input(
            Some("subscriptions".to_string()),
            format!("unsupported realtime subscription: {other}"),
        )),
    }
}

impl TryFrom<DartImCorePaths> for im_core::ImCorePaths {
    type Error = DartImError;

    fn try_from(value: DartImCorePaths) -> Result<Self, Self::Error> {
        Ok(Self {
            identities: im_core::IdentityRegistryPaths {
                identity_root_dir: PathBuf::from(value.identity_root_dir),
                registry_path: PathBuf::from(value.registry_path),
                default_identity_path: value.default_identity_path.map(PathBuf::from),
            },
            local_state: im_core::LocalStatePaths {
                sqlite_path: PathBuf::from(value.sqlite_path),
            },
            runtime: im_core::RuntimePaths {
                cache_dir: PathBuf::from(value.cache_dir),
                temp_dir: PathBuf::from(value.temp_dir),
            },
        })
    }
}

impl TryFrom<crate::dto::config::DartImCoreOpenOptions> for im_core::ImCoreOpenOptions {
    type Error = DartImError;

    fn try_from(value: crate::dto::config::DartImCoreOpenOptions) -> Result<Self, Self::Error> {
        let mut options = im_core::ImCoreOpenOptions {
            identity_secret_storage_policy: value.identity_secret_storage_policy.into(),
            identity_secret_vault: None,
            multi_device_device_revoke_enabled: value.multi_device_device_revoke_enabled,
            multi_device_direct_e2ee_enabled: value.multi_device_direct_e2ee_enabled,
            multi_device_group_e2ee_enabled: value.multi_device_group_e2ee_enabled,
            multi_device_handle_recovery_enabled: value.multi_device_handle_recovery_enabled,
            multi_device_audience: value.multi_device_audience,
            external_http_allow_insecure_loopback_for_testing: false,
        };
        if let Some(vault) = value.identity_secret_vault {
            options.identity_secret_vault = Some(vault.try_into()?);
        }
        Ok(options)
    }
}

impl From<crate::dto::config::DartIdentitySecretStoragePolicy>
    for im_core::IdentitySecretStoragePolicy
{
    fn from(value: crate::dto::config::DartIdentitySecretStoragePolicy) -> Self {
        match value {
            crate::dto::config::DartIdentitySecretStoragePolicy::FileCompat => Self::FileCompat,
            crate::dto::config::DartIdentitySecretStoragePolicy::VaultPreferred => {
                Self::VaultPreferred
            }
            crate::dto::config::DartIdentitySecretStoragePolicy::VaultRequired => {
                Self::VaultRequired
            }
        }
    }
}

impl TryFrom<crate::dto::config::DartImCoreSecretVaultOptions>
    for im_core::ImCoreSecretVaultOptions
{
    type Error = DartImError;

    fn try_from(
        value: crate::dto::config::DartImCoreSecretVaultOptions,
    ) -> Result<Self, Self::Error> {
        Ok(Self::new(
            value.root_key.try_into()?,
            PathBuf::from(value.vault_dir),
            value.workspace_id,
            value.device_id,
        ))
    }
}

impl TryFrom<crate::dto::config::DartDeviceVaultRootKey> for im_core::vault::DeviceVaultRootKey {
    type Error = DartImError;

    fn try_from(value: crate::dto::config::DartDeviceVaultRootKey) -> Result<Self, Self::Error> {
        let bytes: [u8; im_core::vault::DEVICE_VAULT_ROOT_KEY_LEN] =
            value.bytes.try_into().map_err(|_| {
                DartImError::invalid_input(
                    Some("root_key".to_string()),
                    format!(
                        "device vault root key must be {} bytes",
                        im_core::vault::DEVICE_VAULT_ROOT_KEY_LEN
                    ),
                )
            })?;
        Ok(Self::from_bytes(bytes))
    }
}

impl TryFrom<DartIdentitySelector> for im_core::identity::IdentitySelector {
    type Error = DartImError;

    fn try_from(value: DartIdentitySelector) -> Result<Self, Self::Error> {
        match value {
            DartIdentitySelector::Default => Ok(Self::Default),
            DartIdentitySelector::Id { id } => im_core::ids::IdentityId::parse(id)
                .map(Self::Id)
                .map_err(DartImError::from),
            DartIdentitySelector::Did { did } => im_core::ids::Did::parse(did)
                .map(Self::Did)
                .map_err(DartImError::from),
            DartIdentitySelector::Handle { handle } => im_core::ids::Handle::parse(handle, "")
                .map(Self::Handle)
                .map_err(DartImError::from),
            DartIdentitySelector::LocalAlias { alias } => Ok(Self::LocalAlias(alias)),
        }
    }
}

impl From<DartAuthScope> for im_core::auth::AuthScope {
    fn from(value: DartAuthScope) -> Self {
        match value {
            DartAuthScope::UserProfile => Self::UserProfile,
            DartAuthScope::Messaging => Self::Messaging,
            DartAuthScope::GroupMessaging => Self::GroupMessaging,
        }
    }
}

impl TryFrom<DartMessageTarget> for im_core::messages::MessageTarget {
    type Error = DartImError;

    fn try_from(value: DartMessageTarget) -> Result<Self, Self::Error> {
        match value {
            DartMessageTarget::Direct { peer } => im_core::ids::PeerRef::parse(peer, "")
                .map(Self::Direct)
                .map_err(DartImError::from),
            DartMessageTarget::Group { group } => im_core::ids::GroupRef::parse(group)
                .map(Self::Group)
                .map_err(DartImError::from),
        }
    }
}

impl TryFrom<DartThreadRef> for im_core::messages::ThreadRef {
    type Error = DartImError;

    fn try_from(value: DartThreadRef) -> Result<Self, Self::Error> {
        match value {
            DartThreadRef::Direct { peer } => im_core::ids::PeerRef::parse(peer, "")
                .map(Self::Direct)
                .map_err(DartImError::from),
            DartThreadRef::Group { group } => im_core::ids::GroupRef::parse(group)
                .map(Self::Group)
                .map_err(DartImError::from),
            DartThreadRef::Thread { thread_id } => im_core::ids::ThreadId::parse(thread_id)
                .map(Self::Thread)
                .map_err(DartImError::from),
        }
    }
}

impl TryFrom<DartAttachmentInput> for im_core::attachments::AttachmentInput {
    type Error = DartImError;

    fn try_from(value: DartAttachmentInput) -> Result<Self, Self::Error> {
        match value {
            DartAttachmentInput::LocalFile { path } => Ok(Self::LocalFile(PathBuf::from(path))),
            DartAttachmentInput::Bytes {
                filename,
                mime_type,
                bytes,
            } => Ok(Self::Bytes {
                filename,
                mime_type,
                bytes,
            }),
        }
    }
}

impl TryFrom<DartAttachmentDestination> for im_core::attachments::AttachmentDestination {
    type Error = DartImError;

    fn try_from(value: DartAttachmentDestination) -> Result<Self, Self::Error> {
        match value {
            DartAttachmentDestination::LocalFile { path } => {
                Ok(Self::LocalFile(PathBuf::from(path)))
            }
            DartAttachmentDestination::Memory => Ok(Self::Memory),
        }
    }
}

impl DartAttachmentSendRequest {
    pub fn into_core(
        self,
    ) -> Result<
        (
            im_core::messages::MessageTarget,
            im_core::attachments::AttachmentSendRequest,
        ),
        DartImError,
    > {
        Ok((
            self.target.try_into()?,
            im_core::attachments::AttachmentSendRequest {
                input: self.input.try_into()?,
                caption: self.caption,
                mention_payload: parse_optional_json(
                    "mention_payload_json",
                    self.mention_payload_json,
                )?,
                mime_type: self.mime_type,
                filename: self.filename,
                delivery: im_core::messages::MessageDeliveryOptions {
                    idempotency_key: self.idempotency_key,
                    wait_for_final_acceptance: self.wait_for_final_acceptance,
                },
                security: self.security.into(),
            },
        ))
    }
}

impl TryFrom<DartSendConversationAttachmentRequest>
    for im_core::attachments::SendConversationAttachmentRequest
{
    type Error = DartImError;

    fn try_from(value: DartSendConversationAttachmentRequest) -> Result<Self, Self::Error> {
        let mention_payload =
            parse_optional_json("mention_payload_json", value.mention_payload_json)?;
        Ok(Self {
            conversation: value.conversation.try_into()?,
            input: value.input.try_into()?,
            caption: value.caption,
            mention_payload,
            mime_type: value.mime_type,
            filename: value.filename,
            security: value.security.into(),
            client_message_id: value
                .client_message_id
                .map(im_core::ids::MessageId::parse)
                .transpose()
                .map_err(DartImError::from)?,
            idempotency_key: value.idempotency_key,
            wait_for_final_acceptance: value.wait_for_final_acceptance,
        })
    }
}

fn parse_optional_json(
    field: &str,
    value: Option<String>,
) -> Result<Option<serde_json::Value>, DartImError> {
    value
        .map(|text| {
            serde_json::from_str::<serde_json::Value>(&text).map_err(|err| {
                im_core::ImError::invalid_input(Some(field.to_string()), err.to_string()).into()
            })
        })
        .transpose()
}

impl TryFrom<DartDownloadAttachmentRequest> for im_core::attachments::DownloadAttachmentRequest {
    type Error = DartImError;

    fn try_from(value: DartDownloadAttachmentRequest) -> Result<Self, Self::Error> {
        Ok(Self {
            thread: value.thread.try_into()?,
            message_id: im_core::ids::MessageId::parse(value.message_id)
                .map_err(DartImError::from)?,
            attachment_id: value.attachment_id,
            destination: value.destination.try_into()?,
            overwrite: value.overwrite,
        })
    }
}

impl From<DartMessageSecurityMode> for im_core::messages::MessageSecurityMode {
    fn from(value: DartMessageSecurityMode) -> Self {
        match value {
            DartMessageSecurityMode::DefaultPlain => Self::DefaultPlain,
            DartMessageSecurityMode::Plain => Self::Plain,
            DartMessageSecurityMode::E2eeRequired => Self::E2eeRequired,
            DartMessageSecurityMode::SecureDirect => Self::SecureDirect,
            DartMessageSecurityMode::GroupE2ee => Self::GroupE2ee,
        }
    }
}

impl TryFrom<DartSendTextRequest> for im_core::messages::SendMessageRequest {
    type Error = DartImError;

    fn try_from(value: DartSendTextRequest) -> Result<Self, Self::Error> {
        Ok(Self {
            target: value.target.try_into()?,
            body: im_core::messages::MessageBody::Text {
                text: value.text,
                kind: if value.markdown {
                    im_core::messages::MessageKind::Markdown
                } else {
                    im_core::messages::MessageKind::Text
                },
            },
            security: value.security.into(),
            client_message_id: value
                .client_message_id
                .map(im_core::ids::MessageId::parse)
                .transpose()
                .map_err(DartImError::from)?,
            delivery: im_core::messages::MessageDeliveryOptions {
                idempotency_key: value.idempotency_key,
                wait_for_final_acceptance: value.wait_for_final_acceptance,
            },
            delegated_signing: value.delegated_signing.map(Into::into),
        })
    }
}

impl TryFrom<DartSendPayloadRequest> for im_core::messages::SendMessageRequest {
    type Error = DartImError;

    fn try_from(value: DartSendPayloadRequest) -> Result<Self, Self::Error> {
        let payload = parse_payload_json(value.payload_json)?;
        Ok(Self {
            target: value.target.try_into()?,
            body: im_core::messages::MessageBody::Payload { payload },
            security: value.security.into(),
            client_message_id: value
                .client_message_id
                .map(im_core::ids::MessageId::parse)
                .transpose()
                .map_err(DartImError::from)?,
            delivery: im_core::messages::MessageDeliveryOptions {
                idempotency_key: value.idempotency_key,
                wait_for_final_acceptance: value.wait_for_final_acceptance,
            },
            delegated_signing: value.delegated_signing.map(Into::into),
        })
    }
}

impl TryFrom<DartSendConversationTextRequest> for im_core::messages::SendConversationTextRequest {
    type Error = DartImError;

    fn try_from(value: DartSendConversationTextRequest) -> Result<Self, Self::Error> {
        Ok(Self {
            conversation: value.conversation.try_into()?,
            text: value.text,
            markdown: value.markdown,
            security: value.security.into(),
            client_message_id: value
                .client_message_id
                .map(im_core::ids::MessageId::parse)
                .transpose()
                .map_err(DartImError::from)?,
            idempotency_key: value.idempotency_key,
            wait_for_final_acceptance: value.wait_for_final_acceptance,
            delegated_signing: value.delegated_signing.map(Into::into),
        })
    }
}

impl TryFrom<DartSendConversationPayloadRequest>
    for im_core::messages::SendConversationPayloadRequest
{
    type Error = DartImError;

    fn try_from(value: DartSendConversationPayloadRequest) -> Result<Self, Self::Error> {
        Ok(Self {
            conversation: value.conversation.try_into()?,
            payload: parse_payload_json(value.payload_json)?,
            security: value.security.into(),
            client_message_id: value
                .client_message_id
                .map(im_core::ids::MessageId::parse)
                .transpose()
                .map_err(DartImError::from)?,
            idempotency_key: value.idempotency_key,
            wait_for_final_acceptance: value.wait_for_final_acceptance,
            delegated_signing: value.delegated_signing.map(Into::into),
        })
    }
}

fn parse_payload_json(value: String) -> Result<serde_json::Value, DartImError> {
    let payload: serde_json::Value = serde_json::from_str(&value).map_err(|err| {
        DartImError::invalid_input(
            Some("payload_json".to_string()),
            format!("payload_json must be a JSON object: {err}"),
        )
    })?;
    if !payload.is_object() {
        return Err(DartImError::invalid_input(
            Some("payload_json".to_string()),
            "payload_json must be a JSON object",
        ));
    }
    Ok(payload)
}

impl From<DartDelegatedSigningOptions> for im_core::messages::DelegatedSigningOptions {
    fn from(value: DartDelegatedSigningOptions) -> Self {
        Self {
            logical_sender_did: value.logical_sender_did,
            signing_verification_method: value.signing_verification_method,
            signing_key_ref: value.signing_key_ref,
            actor_agent_did: value.actor_agent_did,
        }
    }
}

impl From<DartInboxHistoryOptions> for im_core::messages::InboxHistoryOptions {
    fn from(value: DartInboxHistoryOptions) -> Self {
        Self {
            inbox_owner_did: value.inbox_owner_did,
            inbox_auth_verification_method: value.inbox_auth_verification_method,
            inbox_auth_key_ref: value.inbox_auth_key_ref,
            inbox_auth: value.inbox_auth.map(Into::into),
        }
    }
}

impl From<DartInboxAuth> for im_core::messages::InboxAuth {
    fn from(value: DartInboxAuth) -> Self {
        match value {
            DartInboxAuth::ScopedInboxToken { token } => Self::ScopedInboxToken {
                token: token.into(),
            },
        }
    }
}

impl From<DartScopedInboxToken> for im_core::messages::ScopedInboxToken {
    fn from(value: DartScopedInboxToken) -> Self {
        Self { token: value.token }
    }
}

impl From<DartSyncDeltaRequest> for im_core::messages::SyncDeltaRequest {
    fn from(value: DartSyncDeltaRequest) -> Self {
        Self {
            limit: value.limit,
            device_id: value.device_id,
            reason: value.reason,
        }
    }
}

impl TryFrom<DartSyncThreadAfterRequest> for im_core::messages::SyncThreadAfterRequest {
    type Error = DartImError;

    fn try_from(value: DartSyncThreadAfterRequest) -> Result<Self, Self::Error> {
        Ok(Self {
            thread: value.thread.try_into()?,
            after_server_seq: value.after_server_seq,
            limit: value.limit,
        })
    }
}

impl TryFrom<DartConversationReadRef> for im_core::messages::ConversationReadRef {
    type Error = DartImError;

    fn try_from(value: DartConversationReadRef) -> Result<Self, Self::Error> {
        im_core::messages::ConversationReadRef::new(value.conversation_id).map_err(Into::into)
    }
}

impl TryFrom<DartMarkConversationReadRequest> for im_core::messages::MarkConversationReadRequest {
    type Error = DartImError;

    fn try_from(value: DartMarkConversationReadRequest) -> Result<Self, Self::Error> {
        Ok(Self {
            conversation: value.conversation.try_into()?,
            watermark: value
                .watermark
                .map(dart_read_watermark_to_core)
                .transpose()
                .map_err(DartImError::from)?,
            fallback_max_message_ids: value.fallback_max_message_ids,
        })
    }
}

impl TryFrom<DartSyncConversationAfterRequest> for im_core::messages::SyncConversationAfterRequest {
    type Error = DartImError;

    fn try_from(value: DartSyncConversationAfterRequest) -> Result<Self, Self::Error> {
        Ok(Self {
            conversation: value.conversation.try_into()?,
            after_server_seq: value.after_server_seq,
            limit: value.limit,
        })
    }
}

fn dart_read_watermark_to_core(
    value: crate::dto::message::DartReadWatermark,
) -> im_core::ImResult<im_core::messages::ReadWatermark> {
    Ok(im_core::messages::ReadWatermark {
        last_read_message_id: value
            .last_read_message_id
            .map(im_core::ids::MessageId::parse)
            .transpose()?,
        last_read_thread_seq: value.last_read_thread_seq,
        read_at: value
            .read_at
            .map(|value| {
                chrono::DateTime::parse_from_rfc3339(value.trim())
                    .map(|value| value.with_timezone(&chrono::Utc))
                    .map_err(|err| {
                        im_core::ImError::invalid_input(Some("read_at".to_owned()), err.to_string())
                    })
            })
            .transpose()?,
    })
}

impl DartCreateGroupRequest {
    pub fn into_core(self) -> Result<im_core::groups::GroupCreateRequest, DartImError> {
        Ok(im_core::groups::GroupCreateRequest {
            name: self.name,
            creator_handle: crate::api::groups::identity_handle(
                self.identity_mode,
                self.identity_handle,
            )?,
            description: self.description,
            avatar_uri: self.avatar_uri,
            discoverability: match self.discoverability {
                Some(value) => Some(
                    im_core::groups::GroupDiscoverability::parse(value)
                        .map_err(DartImError::from)?,
                ),
                None => None,
            },
            admission_mode: match self.admission_mode {
                Some(value) => Some(
                    im_core::groups::GroupAdmissionMode::parse(value).map_err(DartImError::from)?,
                ),
                None => None,
            },
            message_security_profile: match self.message_security_profile {
                Some(value) => Some(
                    im_core::groups::GroupMessageSecurityProfile::parse(value)
                        .map_err(DartImError::from)?,
                ),
                None => None,
            },
            security: Default::default(),
            e2ee: self.e2ee,
            slug: self.slug,
            goal: self.goal,
            rules: self.rules,
            message_prompt: self.message_prompt,
            doc_url: self.doc_url,
            attachments_allowed: self.attachments_allowed,
            max_members: match self.max_members {
                Some(value) => Some(
                    im_core::groups::GroupMemberLimit::parse(value).map_err(DartImError::from)?,
                ),
                None => None,
            },
            member_max_messages: self.member_max_messages,
            member_max_total_chars: self.member_max_total_chars,
        })
    }
}

impl From<DartProfilePatch> for im_core::identity::ProfilePatch {
    fn from(value: DartProfilePatch) -> Self {
        Self {
            display_name: value.display_name,
            bio: value.bio,
            tags: value.tags,
            markdown: value.markdown,
            avatar_uri: value.avatar_uri,
            avatar_url: value.avatar_url,
        }
    }
}

impl TryFrom<DartIdentitySubject> for im_core::directory::IdentitySubject {
    type Error = DartImError;

    fn try_from(value: DartIdentitySubject) -> Result<Self, Self::Error> {
        match value {
            DartIdentitySubject::Did { did } => im_core::ids::Did::parse(did)
                .map(Self::Did)
                .map_err(DartImError::from),
            DartIdentitySubject::Handle { handle } => im_core::ids::Handle::parse(handle, "")
                .map(Self::Handle)
                .map_err(DartImError::from),
            DartIdentitySubject::Any { value } => Ok(Self::Any(value)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use im_core::realtime::{RealtimeSubscription, ReconnectPolicy};

    #[test]
    fn realtime_options_map_reconnect_and_subscriptions_without_transport_details() {
        let options = DartRealtimeOptions {
            reconnect: "exponential".to_string(),
            event_buffer: 32,
            reconnect_delay_ms: None,
            reconnect_base_delay_ms: Some(250),
            reconnect_max_delay_ms: Some(5_000),
            reconnect_max_attempts: Some(3),
            subscriptions: vec![
                "messages".to_string(),
                "groups".to_string(),
                "notifications".to_string(),
                "host_notifications".to_string(),
            ],
        };

        let mapped = im_core::realtime::RealtimeOptions::try_from(options).unwrap();
        assert_eq!(
            mapped.reconnect,
            ReconnectPolicy::Exponential {
                base_delay_ms: 250,
                max_delay_ms: 5_000,
                max_attempts: Some(3),
            }
        );
        assert_eq!(mapped.event_buffer, 32);
        assert_eq!(
            mapped.subscriptions,
            vec![
                RealtimeSubscription::Messages,
                RealtimeSubscription::Groups,
                RealtimeSubscription::Notifications,
                RealtimeSubscription::HostNotifications,
            ]
        );
    }

    #[test]
    fn realtime_options_reject_unknown_subscription() {
        let options = DartRealtimeOptions {
            reconnect: "disabled".to_string(),
            event_buffer: 1,
            reconnect_delay_ms: None,
            reconnect_base_delay_ms: None,
            reconnect_max_delay_ms: None,
            reconnect_max_attempts: None,
            subscriptions: vec!["websocket_frames".to_string()],
        };

        let error = im_core::realtime::RealtimeOptions::try_from(options).unwrap_err();
        assert_eq!(error.code, "invalid_input");
        assert_eq!(error.field.as_deref(), Some("subscriptions"));
        assert!(error.message.contains("unsupported realtime subscription"));
    }
}
