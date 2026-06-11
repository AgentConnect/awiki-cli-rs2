use crate::dto::{
    attachment::{
        DartAttachmentSendResult, DartDownloadedAttachment, DartDownloadedAttachmentDestination,
        DartUploadedAttachment,
    },
    auth::{DartAuthScope, DartAuthStatus, DartSessionBundle, DartSessionUpdate},
    directory::{DartDirectoryResolution, DartRelationStatus},
    email::{
        DartEmailAccount, DartEmailAttachmentContent, DartEmailAttachmentMetadata,
        DartEmailAttribute, DartEmailMarkReadResult, DartEmailMessage, DartEmailMessageSummary,
        DartEmailMessageSummaryPage, DartEmailNotification, DartEmailNotificationPage,
        DartSendEmailResult,
    },
    group::{DartGroupMember, DartGroupReadResult, DartGroupSnapshot, DartGroupSummary},
    identity::{
        DartDefaultIdentityChange, DartDeleteLocalIdentityResult, DartHandleRegistrationResult,
        DartIdentitySummary, DartRecoverHandleResult,
    },
    message::{
        DartConversation, DartConversationPage, DartMarkReadResult, DartMessage,
        DartMessageBodyView, DartMessageDirection, DartMessageMetadata,
        DartMessageMetadataAttribute, DartMessagePage, DartSendMessageResult,
    },
    profile::DartUserProfile,
    realtime::{DartRealtimeEvent, DartRealtimeStatus},
    secure::{
        DartDirectSecurePrepareResult, DartDirectSecureRepairResult, DartDirectSecureState,
        DartDirectSecureStatus, DartGroupSecureLocalReadiness, DartGroupSecurePendingWork,
        DartGroupSecurePrepareResult, DartGroupSecureRepairResult, DartGroupSecureState,
        DartGroupSecureStatus, DartSecureDelivery, DartSecureOutboxEntry, DartSecureOutboxResult,
        DartSecureOutboxStatus, DartSecureProblem, DartSecureProblemCode,
    },
};

impl From<im_core::identity::IdentitySummary> for DartIdentitySummary {
    fn from(value: im_core::identity::IdentitySummary) -> Self {
        Self {
            id: value.id.as_str().to_string(),
            did: value.did.as_str().to_string(),
            handle: value.handle.map(|handle| handle.as_str().to_string()),
            display_name: value.display_name,
            local_alias: value.local_alias,
            device_id: value.device_id,
            is_default: value.is_default,
            ready_for_auth: value.readiness.ready_for_auth,
            ready_for_messaging: value.readiness.ready_for_messaging,
            missing: value
                .readiness
                .missing
                .into_iter()
                .map(identity_missing_item_to_string)
                .collect(),
        }
    }
}

impl From<im_core::email::EmailAttribute> for DartEmailAttribute {
    fn from(value: im_core::email::EmailAttribute) -> Self {
        Self {
            key: value.key,
            value: value.value,
        }
    }
}

impl From<im_core::email::EmailAccount> for DartEmailAccount {
    fn from(value: im_core::email::EmailAccount) -> Self {
        Self {
            mailbox_address: value
                .mailbox_address
                .map(|address| address.as_str().to_string()),
            display_name: value.display_name,
            status: value.status,
            attributes: value.attributes.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<im_core::email::EmailMessageSummary> for DartEmailMessageSummary {
    fn from(value: im_core::email::EmailMessageSummary) -> Self {
        Self {
            id: value.id.as_str().to_string(),
            folder: value.folder.map(|folder| folder.as_str().to_string()),
            from: value
                .from
                .into_iter()
                .map(|address| address.as_str().to_string())
                .collect(),
            to: value
                .to
                .into_iter()
                .map(|address| address.as_str().to_string())
                .collect(),
            cc: value
                .cc
                .into_iter()
                .map(|address| address.as_str().to_string())
                .collect(),
            subject: value.subject,
            preview: value.preview,
            received_at: value.received_at,
            sent_at: value.sent_at,
            unread: value.unread,
            has_attachments: value.has_attachments,
            attachment_count: value.attachment_count,
            attributes: value.attributes.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<im_core::ids::Page<im_core::email::EmailMessageSummary>> for DartEmailMessageSummaryPage {
    fn from(value: im_core::ids::Page<im_core::email::EmailMessageSummary>) -> Self {
        Self {
            items: value.items.into_iter().map(Into::into).collect(),
            next_cursor: value.next_cursor.map(|cursor| cursor.as_str().to_string()),
            has_more: value.has_more,
        }
    }
}

impl From<im_core::email::EmailAttachmentMetadata> for DartEmailAttachmentMetadata {
    fn from(value: im_core::email::EmailAttachmentMetadata) -> Self {
        Self {
            index: value.index,
            filename: value.filename,
            content_type: value.content_type,
            size: value.size,
        }
    }
}

impl From<im_core::email::EmailMessage> for DartEmailMessage {
    fn from(value: im_core::email::EmailMessage) -> Self {
        Self {
            summary: value.summary.into(),
            body_text: value.body_text,
            body_html: value.body_html,
            attachments: value.attachments.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<im_core::email::EmailAttachmentContent> for DartEmailAttachmentContent {
    fn from(value: im_core::email::EmailAttachmentContent) -> Self {
        Self {
            message_id: value.message_id.as_str().to_string(),
            attachment_index: value.attachment_index,
            filename: value.filename,
            content_type: value.content_type,
            size: value.size,
            bytes: value.bytes,
        }
    }
}

impl From<im_core::email::EmailMarkReadResult> for DartEmailMarkReadResult {
    fn from(value: im_core::email::EmailMarkReadResult) -> Self {
        Self {
            updated: value.updated,
        }
    }
}

impl From<im_core::email::SendEmailResult> for DartSendEmailResult {
    fn from(value: im_core::email::SendEmailResult) -> Self {
        Self {
            accepted: value.accepted,
            message_id: value.message_id.map(|id| id.as_str().to_string()),
            warnings: value.warnings,
        }
    }
}

impl From<im_core::email::EmailNotification> for DartEmailNotification {
    fn from(value: im_core::email::EmailNotification) -> Self {
        Self {
            id: value.id.as_str().to_string(),
            mailbox_address: value
                .mailbox_address
                .map(|address| address.as_str().to_string()),
            from_addr: value.from_addr,
            subject: value.subject,
            preview: value.preview,
            has_attachments: value.has_attachments,
            received_at: value.received_at,
            attributes: value.attributes.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<im_core::ids::Page<im_core::email::EmailNotification>> for DartEmailNotificationPage {
    fn from(value: im_core::ids::Page<im_core::email::EmailNotification>) -> Self {
        Self {
            items: value.items.into_iter().map(Into::into).collect(),
            next_cursor: value.next_cursor.map(|cursor| cursor.as_str().to_string()),
            has_more: value.has_more,
        }
    }
}

impl From<im_core::identity::DefaultIdentityChange> for DartDefaultIdentityChange {
    fn from(value: im_core::identity::DefaultIdentityChange) -> Self {
        Self {
            previous: value.previous.map(Into::into),
            next: value.next.into(),
            requires_default_identity_write: value.requires_default_identity_write,
            warnings: value.warnings,
        }
    }
}

impl From<im_core::identity::DeleteLocalIdentityResult> for DartDeleteLocalIdentityResult {
    fn from(value: im_core::identity::DeleteLocalIdentityResult) -> Self {
        Self {
            deleted: value.deleted.into(),
            was_default: value.was_default,
            next_default: value.next_default.map(Into::into),
            warnings: value.warnings,
        }
    }
}

impl From<im_core::identity::HandleRegistrationResult> for DartHandleRegistrationResult {
    fn from(value: im_core::identity::HandleRegistrationResult) -> Self {
        Self {
            identity: value.identity.map(Into::into),
            handle: value.handle.as_str().to_string(),
            method: registration_method_to_string(value.method),
            state: registration_state_to_string(value.state),
            default_identity_change: value.default_identity_change.map(Into::into),
            warnings: value.warnings,
        }
    }
}

fn registration_method_to_string(value: im_core::identity::RegistrationMethod) -> String {
    match value {
        im_core::identity::RegistrationMethod::Phone => "phone".to_string(),
        im_core::identity::RegistrationMethod::Email => "email".to_string(),
        im_core::identity::RegistrationMethod::AlreadyVerified => "already_verified".to_string(),
    }
}

fn registration_state_to_string(value: im_core::identity::HandleRegistrationState) -> String {
    match value {
        im_core::identity::HandleRegistrationState::OtpSent => "otp_sent".to_string(),
        im_core::identity::HandleRegistrationState::EmailSent => "email_sent".to_string(),
        im_core::identity::HandleRegistrationState::EmailPending => "email_pending".to_string(),
        im_core::identity::HandleRegistrationState::Registered => "registered".to_string(),
    }
}

impl From<im_core::identity::RecoverHandleResult> for DartRecoverHandleResult {
    fn from(value: im_core::identity::RecoverHandleResult) -> Self {
        let (recovered_identity, user_id, access_token_present) = value
            .recovered_identity
            .map(|recovered| {
                (
                    Some(recovered.identity.into()),
                    recovered.user_id,
                    recovered.access_token_present,
                )
            })
            .unwrap_or((None, None, false));
        Self {
            handle: value.handle.as_str().to_string(),
            phone: value.phone,
            state: recover_state_to_string(value.state),
            recovered_identity,
            user_id,
            access_token_present,
            warnings: value.warnings,
        }
    }
}

fn recover_state_to_string(value: im_core::identity::RecoverHandleState) -> String {
    match value {
        im_core::identity::RecoverHandleState::OtpSent => "otp_sent".to_string(),
        im_core::identity::RecoverHandleState::Recovered => "recovered".to_string(),
    }
}

fn identity_missing_item_to_string(value: im_core::identity::IdentityMissingItem) -> String {
    match value {
        im_core::identity::IdentityMissingItem::DidDocument => "did_document".to_string(),
        im_core::identity::IdentityMissingItem::PrivateKey => "private_key".to_string(),
        im_core::identity::IdentityMissingItem::AuthState => "auth_state".to_string(),
        im_core::identity::IdentityMissingItem::Handle => "handle".to_string(),
        im_core::identity::IdentityMissingItem::MessageEndpoint => "message_endpoint".to_string(),
        im_core::identity::IdentityMissingItem::Other(value) => value,
    }
}

impl From<im_core::auth::AuthScope> for DartAuthScope {
    fn from(value: im_core::auth::AuthScope) -> Self {
        match value {
            im_core::auth::AuthScope::UserProfile => Self::UserProfile,
            im_core::auth::AuthScope::Messaging => Self::Messaging,
            im_core::auth::AuthScope::GroupMessaging => Self::GroupMessaging,
        }
    }
}

impl From<im_core::auth::AuthStatus> for DartAuthStatus {
    fn from(value: im_core::auth::AuthStatus) -> Self {
        Self {
            subject: value.subject.as_str().to_string(),
            has_session: value.has_session,
            expires_at: value.expires_at,
            needs_refresh: value.needs_refresh,
            warnings: value.warnings,
        }
    }
}

impl From<im_core::auth::SessionBundle> for DartSessionBundle {
    fn from(value: im_core::auth::SessionBundle) -> Self {
        Self {
            subject: value.subject.as_str().to_string(),
            scope: value.scope.into(),
            expires_at: value.expires_at,
            refreshed: value.refreshed,
            bearer_token: value.bearer_token,
        }
    }
}

impl From<im_core::auth::SessionUpdate> for DartSessionUpdate {
    fn from(value: im_core::auth::SessionUpdate) -> Self {
        Self {
            subject: value.subject.as_str().to_string(),
            previous_expires_at: value.previous_expires_at,
            new_expires_at: value.new_expires_at,
            refreshed: value.refreshed,
            bearer_token: value.bearer_token,
        }
    }
}

impl From<im_core::identity::Profile> for DartUserProfile {
    fn from(value: im_core::identity::Profile) -> Self {
        let full_handle = value.handle.map(|handle| handle.as_str().to_string());
        Self {
            subject: value.subject.as_str().to_string(),
            handle: full_handle.clone(),
            full_handle,
            display_name: value.display_name,
            bio: value.bio,
            tags: value.tags,
            markdown: value.markdown,
            avatar_url: value.avatar_url,
            updated_at: value.updated_at,
        }
    }
}

impl From<im_core::directory::PublicProfile> for DartUserProfile {
    fn from(value: im_core::directory::PublicProfile) -> Self {
        value.profile.into()
    }
}

impl From<im_core::directory::DirectoryResolution> for DartDirectoryResolution {
    fn from(value: im_core::directory::DirectoryResolution) -> Self {
        Self {
            input: value.input,
            did: value.did.as_str().to_string(),
            handle: value.handle.map(|handle| handle.as_str().to_string()),
            profile: value.profile.map(Into::into),
            warnings: value.warnings,
        }
    }
}

impl From<im_core::directory::RelationStatus> for DartRelationStatus {
    fn from(value: im_core::directory::RelationStatus) -> Self {
        Self {
            peer: value.peer.as_str().to_string(),
            relationship: value.relationship,
            display_name: None,
        }
    }
}

impl From<im_core::messages::MessageDirection> for DartMessageDirection {
    fn from(value: im_core::messages::MessageDirection) -> Self {
        match value {
            im_core::messages::MessageDirection::Outgoing => Self::Outgoing,
            im_core::messages::MessageDirection::Incoming => Self::Incoming,
            im_core::messages::MessageDirection::Unknown => Self::Unknown,
        }
    }
}

impl From<im_core::messages::MessageBodyView> for DartMessageBodyView {
    fn from(value: im_core::messages::MessageBodyView) -> Self {
        match value {
            im_core::messages::MessageBodyView::Text { text, kind } => Self {
                text: Some(text),
                kind: Some(message_kind_to_string(kind)),
                payload_json: None,
                unsupported_content_type: None,
            },
            im_core::messages::MessageBodyView::Payload { payload } => Self {
                text: None,
                kind: Some("payload".to_string()),
                payload_json: Some(payload.to_string()),
                unsupported_content_type: None,
            },
            im_core::messages::MessageBodyView::Unsupported { content_type } => Self {
                text: None,
                kind: None,
                payload_json: None,
                unsupported_content_type: content_type,
            },
        }
    }
}

fn message_kind_to_string(value: im_core::messages::MessageKind) -> String {
    match value {
        im_core::messages::MessageKind::Text => "text".to_string(),
        im_core::messages::MessageKind::Markdown => "markdown".to_string(),
    }
}

impl From<im_core::messages::MessageMetadata> for DartMessageMetadata {
    fn from(value: im_core::messages::MessageMetadata) -> Self {
        Self {
            operation_id: value.operation_id,
            delivery_state: value.delivery_state,
            send_state: value
                .send_state
                .map(|state| message_send_state_to_string(state.state)),
            retryable: value.retry_plan.as_ref().map(|plan| plan.retryable),
            retry_action: value
                .retry_plan
                .map(|plan| message_retry_action_to_string(plan.action)),
            server_sequence: value.server_sequence,
            content_type: value.content_type,
            attributes: value.attributes.into_iter().map(Into::into).collect(),
        }
    }
}

fn message_send_state_to_string(value: im_core::messages::MessageSendStateKind) -> String {
    match value {
        im_core::messages::MessageSendStateKind::Accepted => "accepted".to_string(),
        im_core::messages::MessageSendStateKind::Sent => "sent".to_string(),
        im_core::messages::MessageSendStateKind::StoredLocally => "stored_locally".to_string(),
        im_core::messages::MessageSendStateKind::Failed => "failed".to_string(),
    }
}

fn message_retry_action_to_string(value: im_core::messages::MessageRetryAction) -> String {
    match value {
        im_core::messages::MessageRetryAction::None => "none".to_string(),
        im_core::messages::MessageRetryAction::RetryDirectText => "retry_direct_text".to_string(),
        im_core::messages::MessageRetryAction::RetryGroupText => "retry_group_text".to_string(),
        im_core::messages::MessageRetryAction::RetryDirectPayload => {
            "retry_direct_payload".to_string()
        }
        im_core::messages::MessageRetryAction::RetryGroupPayload => {
            "retry_group_payload".to_string()
        }
    }
}

impl From<im_core::messages::MessageMetadataAttribute> for DartMessageMetadataAttribute {
    fn from(value: im_core::messages::MessageMetadataAttribute) -> Self {
        Self {
            key: value.key,
            value: value.value,
        }
    }
}

impl From<im_core::messages::MessageTarget> for crate::dto::message::DartMessageTarget {
    fn from(value: im_core::messages::MessageTarget) -> Self {
        match value {
            im_core::messages::MessageTarget::Direct(peer) => Self::Direct {
                peer: peer.as_str().to_string(),
            },
            im_core::messages::MessageTarget::Group(group) => Self::Group {
                group: group.as_str().to_string(),
            },
        }
    }
}

impl From<im_core::messages::Message> for DartMessage {
    fn from(value: im_core::messages::Message) -> Self {
        let (thread_kind, thread_id) = thread_ref_parts(value.thread);
        Self {
            id: value.id.as_str().to_string(),
            thread_kind,
            thread_id,
            direction: value.direction.into(),
            sender: value.sender.as_str().to_string(),
            receiver: value.receiver.map(|receiver| receiver.as_str().to_string()),
            group: value.group.map(|group| group.as_str().to_string()),
            body: value.body.into(),
            sent_at: value.sent_at,
            received_at: value.received_at,
            metadata: value.metadata.into(),
        }
    }
}

fn thread_ref_parts(value: im_core::messages::ThreadRef) -> (String, String) {
    match value {
        im_core::messages::ThreadRef::Direct(peer) => {
            ("direct".to_string(), peer.as_str().to_string())
        }
        im_core::messages::ThreadRef::Group(group) => {
            ("group".to_string(), group.as_str().to_string())
        }
        im_core::messages::ThreadRef::Thread(id) => ("thread".to_string(), id.as_str().to_string()),
    }
}

impl From<im_core::ids::Page<im_core::messages::Message>> for DartMessagePage {
    fn from(value: im_core::ids::Page<im_core::messages::Message>) -> Self {
        Self {
            items: value.items.into_iter().map(Into::into).collect(),
            next_cursor: value.next_cursor.map(|cursor| cursor.as_str().to_string()),
            has_more: value.has_more,
        }
    }
}

impl From<im_core::messages::Conversation> for DartConversation {
    fn from(value: im_core::messages::Conversation) -> Self {
        let (thread_kind, thread_id) = thread_ref_parts(value.thread);
        Self {
            thread_kind,
            thread_id,
            title: value.title,
            participants: value
                .participants
                .into_iter()
                .map(|peer| peer.as_str().to_string())
                .collect(),
            last_message: value.last_message.map(Into::into),
            unread_count: value.unread_count,
            message_count: value.message_count,
            last_message_at: value.last_message_at,
        }
    }
}

impl From<im_core::ids::Page<im_core::messages::Conversation>> for DartConversationPage {
    fn from(value: im_core::ids::Page<im_core::messages::Conversation>) -> Self {
        Self {
            items: value.items.into_iter().map(Into::into).collect(),
            next_cursor: value.next_cursor.map(|cursor| cursor.as_str().to_string()),
            has_more: value.has_more,
        }
    }
}

impl From<im_core::messages::SendMessageResult> for DartSendMessageResult {
    fn from(value: im_core::messages::SendMessageResult) -> Self {
        Self {
            message: value.message.into(),
            delivery_state: delivery_state_to_string(value.delivery),
            warnings: value.warnings,
        }
    }
}

impl From<im_core::attachments::UploadedAttachment> for DartUploadedAttachment {
    fn from(value: im_core::attachments::UploadedAttachment) -> Self {
        Self {
            attachment_id: value.attachment_id,
            filename: value.filename,
            mime_type: value.mime_type,
            size_bytes: value.size_bytes,
            size: value.size,
            digest_b64u: value.digest_b64u,
            object_uri: value.object_uri,
            object_encryption_mode: value.object_encryption_mode,
            plaintext_size_bytes: value.plaintext_size_bytes,
        }
    }
}

impl From<im_core::attachments::AttachmentSendResult> for DartAttachmentSendResult {
    fn from(value: im_core::attachments::AttachmentSendResult) -> Self {
        Self {
            message: value.message.into(),
            target_kind: value.target_kind,
            target_did: value.target_did,
            attachment: value.attachment.into(),
            // `AttachmentSendResult.manifest` is already redacted by im-core for
            // E2EE attachments; keep exposing only that public projection.
            manifest_json: value.manifest.to_string(),
        }
    }
}

impl From<im_core::attachments::DownloadedAttachment> for DartDownloadedAttachment {
    fn from(value: im_core::attachments::DownloadedAttachment) -> Self {
        Self {
            attachment_id: value.attachment_id,
            filename: value.filename,
            mime_type: value.mime_type,
            size_bytes: value.size_bytes,
            destination: value.destination.into(),
            warnings: value.warnings,
        }
    }
}

impl From<im_core::attachments::DownloadedAttachmentDestination>
    for DartDownloadedAttachmentDestination
{
    fn from(value: im_core::attachments::DownloadedAttachmentDestination) -> Self {
        match value {
            im_core::attachments::DownloadedAttachmentDestination::LocalFile(path) => {
                Self::LocalFile {
                    path: path.display().to_string(),
                }
            }
            im_core::attachments::DownloadedAttachmentDestination::Memory(bytes) => {
                Self::Memory { bytes }
            }
        }
    }
}

fn delivery_state_to_string(value: im_core::messages::DeliveryState) -> String {
    match value {
        im_core::messages::DeliveryState::Accepted => "accepted".to_string(),
        im_core::messages::DeliveryState::Sent => "sent".to_string(),
        im_core::messages::DeliveryState::StoredLocally => "stored_locally".to_string(),
        im_core::messages::DeliveryState::Failed { reason } => format!("failed:{reason}"),
    }
}

impl From<im_core::secure::DirectSecureState> for DartDirectSecureState {
    fn from(value: im_core::secure::DirectSecureState) -> Self {
        match value {
            im_core::secure::DirectSecureState::Ready => Self::Ready,
            im_core::secure::DirectSecureState::Preparing => Self::Preparing,
            im_core::secure::DirectSecureState::WaitingForPeer => Self::WaitingForPeer,
            im_core::secure::DirectSecureState::NeedsRepair => Self::NeedsRepair,
            im_core::secure::DirectSecureState::Unavailable => Self::Unavailable,
            im_core::secure::DirectSecureState::Unknown => Self::Unknown,
        }
    }
}

impl From<im_core::secure::DirectSecureStatus> for DartDirectSecureStatus {
    fn from(value: im_core::secure::DirectSecureStatus) -> Self {
        Self {
            peer: value.peer.as_str().to_string(),
            resolved_peer: value.resolved_peer.map(|peer| peer.as_str().to_string()),
            state: value.state.into(),
            can_send_secure: value.can_send_secure,
            pending_outbox_count: value.pending_outbox_count,
            problem: value.problem.map(Into::into),
            warnings: value.warnings,
        }
    }
}

impl From<im_core::secure::DirectSecurePrepareResult> for DartDirectSecurePrepareResult {
    fn from(value: im_core::secure::DirectSecurePrepareResult) -> Self {
        Self {
            peer: value.peer.as_str().to_string(),
            state: value.state.into(),
            can_send_secure: value.can_send_secure,
            warnings: value.warnings,
        }
    }
}

impl From<im_core::secure::DirectSecureRepairResult> for DartDirectSecureRepairResult {
    fn from(value: im_core::secure::DirectSecureRepairResult) -> Self {
        Self {
            peer: value.peer.as_str().to_string(),
            state: value.state.into(),
            repaired: value.repaired,
            problem: value.problem.map(Into::into),
            warnings: value.warnings,
        }
    }
}

impl From<im_core::secure::GroupSecureState> for DartGroupSecureState {
    fn from(value: im_core::secure::GroupSecureState) -> Self {
        match value {
            im_core::secure::GroupSecureState::Ready => Self::Ready,
            im_core::secure::GroupSecureState::Syncing => Self::Syncing,
            im_core::secure::GroupSecureState::NeedsRepair => Self::NeedsRepair,
            im_core::secure::GroupSecureState::WaitingForMembershipUpdate => {
                Self::WaitingForMembershipUpdate
            }
            im_core::secure::GroupSecureState::MissingLocalState => Self::MissingLocalState,
            im_core::secure::GroupSecureState::Unavailable => Self::Unavailable,
            im_core::secure::GroupSecureState::Unknown => Self::Unknown,
        }
    }
}

impl From<im_core::secure::GroupSecureLocalReadiness> for DartGroupSecureLocalReadiness {
    fn from(value: im_core::secure::GroupSecureLocalReadiness) -> Self {
        Self {
            has_local_state: value.has_local_state,
            has_active_membership: value.has_active_membership,
        }
    }
}

impl From<im_core::secure::GroupSecurePendingWork> for DartGroupSecurePendingWork {
    fn from(value: im_core::secure::GroupSecurePendingWork) -> Self {
        Self {
            pending_notices: value.pending_notices,
            pending_commits: value.pending_commits,
        }
    }
}

impl From<im_core::secure::GroupSecureStatus> for DartGroupSecureStatus {
    fn from(value: im_core::secure::GroupSecureStatus) -> Self {
        Self {
            group: value.group.as_str().to_string(),
            state: value.state.into(),
            can_send_secure: value.can_send_secure,
            local_readiness: value.local_readiness.into(),
            pending_work: value.pending_work.into(),
            problem: value.problem.map(Into::into),
            warnings: value.warnings,
        }
    }
}

impl From<im_core::secure::GroupSecurePrepareResult> for DartGroupSecurePrepareResult {
    fn from(value: im_core::secure::GroupSecurePrepareResult) -> Self {
        Self {
            group: value.group.as_str().to_string(),
            state: value.state.into(),
            can_send_secure: value.can_send_secure,
            warnings: value.warnings,
        }
    }
}

impl From<im_core::secure::GroupSecureRepairResult> for DartGroupSecureRepairResult {
    fn from(value: im_core::secure::GroupSecureRepairResult) -> Self {
        Self {
            group: value.group.as_str().to_string(),
            state: value.state.into(),
            repaired: value.repaired,
            problem: value.problem.map(Into::into),
            warnings: value.warnings,
        }
    }
}

impl From<im_core::secure::SecureOutboxStatus> for DartSecureOutboxStatus {
    fn from(value: im_core::secure::SecureOutboxStatus) -> Self {
        match value {
            im_core::secure::SecureOutboxStatus::Queued => Self::Queued,
            im_core::secure::SecureOutboxStatus::Sending => Self::Sending,
            im_core::secure::SecureOutboxStatus::Failed => Self::Failed,
            im_core::secure::SecureOutboxStatus::Sent => Self::Sent,
            im_core::secure::SecureOutboxStatus::Dropped => Self::Dropped,
        }
    }
}

impl From<im_core::secure::SecureOutboxEntry> for DartSecureOutboxEntry {
    fn from(value: im_core::secure::SecureOutboxEntry) -> Self {
        Self {
            id: value.id.as_str().to_string(),
            target: value.target.into(),
            message_kind: value.message_kind,
            status: value.status.into(),
            attempt_count: value.attempt_count,
            last_error: value.last_error.map(Into::into),
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

impl From<im_core::secure::SecureOutboxResult> for DartSecureOutboxResult {
    fn from(value: im_core::secure::SecureOutboxResult) -> Self {
        Self {
            id: value.id.as_str().to_string(),
            status: value.status.into(),
            delivery: value.delivery.map(Into::into),
            warnings: value.warnings,
        }
    }
}

impl From<im_core::secure::SecureDelivery> for DartSecureDelivery {
    fn from(value: im_core::secure::SecureDelivery) -> Self {
        Self {
            message_id: value.message_id.map(|id| id.as_str().to_string()),
            state: delivery_state_to_string(value.state),
        }
    }
}

impl From<im_core::secure::SecureProblem> for DartSecureProblem {
    fn from(value: im_core::secure::SecureProblem) -> Self {
        Self {
            code: value.code.into(),
            message: value.message,
            retryable: value.retryable,
        }
    }
}

impl From<im_core::secure::SecureProblemCode> for DartSecureProblemCode {
    fn from(value: im_core::secure::SecureProblemCode) -> Self {
        match value {
            im_core::secure::SecureProblemCode::IdentityNotReady => Self::IdentityNotReady,
            im_core::secure::SecureProblemCode::PeerNotFound => Self::PeerNotFound,
            im_core::secure::SecureProblemCode::PeerKeysUnavailable => Self::PeerKeysUnavailable,
            im_core::secure::SecureProblemCode::SessionNeedsRepair => Self::SessionNeedsRepair,
            im_core::secure::SecureProblemCode::GroupStateUnavailable => {
                Self::GroupStateUnavailable
            }
            im_core::secure::SecureProblemCode::LocalStateUnavailable => {
                Self::LocalStateUnavailable
            }
            im_core::secure::SecureProblemCode::TransportUnavailable => Self::TransportUnavailable,
            im_core::secure::SecureProblemCode::Unsupported => Self::Unsupported,
            im_core::secure::SecureProblemCode::Unknown => Self::Unknown,
        }
    }
}

impl From<im_core::messages::MarkReadResult> for DartMarkReadResult {
    fn from(value: im_core::messages::MarkReadResult) -> Self {
        Self {
            updated_count: value.updated_count,
            message_ids: value
                .message_ids
                .into_iter()
                .map(|id| id.as_str().to_string())
                .collect(),
            warnings: value.warnings,
        }
    }
}

impl From<im_core::groups::GroupSummary> for DartGroupSummary {
    fn from(value: im_core::groups::GroupSummary) -> Self {
        Self {
            id: value.id,
            did: value.did.as_str().to_string(),
            name: value.name,
            my_role: value.my_role,
            membership_status: value.membership_status,
            member_count: value.member_count,
            last_message_at: value.last_message_at,
        }
    }
}

impl From<im_core::groups::GroupSnapshot> for DartGroupSnapshot {
    fn from(value: im_core::groups::GroupSnapshot) -> Self {
        Self {
            id: value.id,
            did: value.did.as_str().to_string(),
            name: value.name,
            description: value.description,
            my_role: value.my_role,
            membership_status: value.membership_status,
            member_count: value.member_count,
            last_message_at: value.last_message_at,
        }
    }
}

impl From<im_core::groups::GroupMember> for DartGroupMember {
    fn from(value: im_core::groups::GroupMember) -> Self {
        Self {
            did: value.did.map(|did| did.as_str().to_string()),
            handle: value.handle.map(|handle| handle.as_str().to_string()),
            role: value.role,
            status: value.status,
            joined_at: value.joined_at,
        }
    }
}

impl From<im_core::groups::GroupReadResult> for DartGroupReadResult {
    fn from(value: im_core::groups::GroupReadResult) -> Self {
        Self {
            group: value.group.map(Into::into),
            groups: value.groups.into_iter().map(Into::into).collect(),
            members: value.members.into_iter().map(Into::into).collect(),
            messages: value.messages.into(),
            total: value.total,
            source: value.source,
            warnings: value.warnings,
        }
    }
}

impl From<im_core::realtime::RealtimeStatus> for DartRealtimeStatus {
    fn from(value: im_core::realtime::RealtimeStatus) -> Self {
        Self {
            connected: value.connected,
            state: realtime_state_to_string(value.state),
            subscriptions: value
                .subscriptions
                .into_iter()
                .map(realtime_subscription_to_string)
                .collect(),
            last_error: value.last_error,
            warnings: Vec::new(),
        }
    }
}

pub(crate) fn realtime_event_to_dart(value: im_core::realtime::ImEvent) -> DartRealtimeEvent {
    use im_core::realtime::{GroupUpdateKind, HostNotificationKind, ImEvent, MessageUpdateKind};

    let empty = || DartRealtimeEvent {
        kind: String::new(),
        state: None,
        reason: None,
        message: None,
        message_id: None,
        thread_kind: None,
        thread_id: None,
        update_kind: None,
        group: None,
        notification_id: None,
        title: None,
        body: None,
        source: None,
        host_kind: None,
        content_type: None,
        notification_type: None,
    };

    match value {
        ImEvent::ConnectionStateChanged(event) => {
            let mut out = empty();
            out.kind = "connection_state_changed".to_string();
            out.state = Some(realtime_state_to_string(event.state));
            out.reason = event.reason;
            out
        }
        ImEvent::MessageReceived(event) => {
            let mut out = empty();
            out.kind = "message_received".to_string();
            out.message = Some(event.message.into());
            out
        }
        ImEvent::MessageUpdated(event) => {
            let (thread_kind, thread_id) = thread_ref_parts(event.thread);
            let mut out = empty();
            out.kind = "message_updated".to_string();
            out.message_id = Some(event.message_id.as_str().to_string());
            out.thread_kind = Some(thread_kind);
            out.thread_id = Some(thread_id);
            out.update_kind = Some(
                match event.update_kind {
                    MessageUpdateKind::Read => "read",
                    MessageUpdateKind::DeliveryStateChanged => "delivery_state_changed",
                    MessageUpdateKind::Unknown => "unknown",
                }
                .to_string(),
            );
            out
        }
        ImEvent::GroupUpdated(event) => {
            let mut out = empty();
            out.kind = "group_updated".to_string();
            out.group = Some(event.group.as_str().to_string());
            out.update_kind = Some(
                match event.update_kind {
                    GroupUpdateKind::Created => "created",
                    GroupUpdateKind::Updated => "updated",
                    GroupUpdateKind::MemberAdded => "member_added",
                    GroupUpdateKind::MemberRemoved => "member_removed",
                    GroupUpdateKind::MessageAdded => "message_added",
                    GroupUpdateKind::Unknown => "unknown",
                }
                .to_string(),
            );
            out
        }
        ImEvent::LocalNotification(event) => {
            let mut out = empty();
            out.kind = "local_notification".to_string();
            out.notification_id = event.notification_id;
            out.title = event.title;
            out.body = event.body;
            out.source = event.source;
            out
        }
        ImEvent::HostNotification(event) => {
            let mut out = empty();
            out.kind = "host_notification".to_string();
            out.host_kind = Some(
                match event.event_type {
                    HostNotificationKind::DirectMessage => "direct_message",
                    HostNotificationKind::GroupMessage => "group_message",
                    HostNotificationKind::GroupState => "group_state",
                    HostNotificationKind::Mail => "mail",
                    HostNotificationKind::Unknown => "unknown",
                }
                .to_string(),
            );
            out.title = event.title;
            out.body = event.body;
            if let Some(thread) = event.thread {
                let (thread_kind, thread_id) = thread_ref_parts(thread);
                out.thread_kind = Some(thread_kind);
                out.thread_id = Some(thread_id);
            }
            out
        }
        ImEvent::UnknownNotification(event) => {
            let mut out = empty();
            out.kind = "unknown_notification".to_string();
            out.reason = Some(event.reason);
            out.content_type = event.content_type;
            out.notification_type = event.notification_type;
            out
        }
    }
}

pub(crate) fn realtime_state_to_string(
    value: im_core::realtime::RealtimeConnectionState,
) -> String {
    match value {
        im_core::realtime::RealtimeConnectionState::Disconnected => "disconnected".to_string(),
        im_core::realtime::RealtimeConnectionState::Connecting => "connecting".to_string(),
        im_core::realtime::RealtimeConnectionState::Connected => "connected".to_string(),
        im_core::realtime::RealtimeConnectionState::Reconnecting => "reconnecting".to_string(),
        im_core::realtime::RealtimeConnectionState::Closed => "closed".to_string(),
    }
}

fn realtime_subscription_to_string(value: im_core::realtime::RealtimeSubscription) -> String {
    match value {
        im_core::realtime::RealtimeSubscription::Messages => "messages".to_string(),
        im_core::realtime::RealtimeSubscription::Groups => "groups".to_string(),
        im_core::realtime::RealtimeSubscription::Notifications => "notifications".to_string(),
        im_core::realtime::RealtimeSubscription::HostNotifications => {
            "host_notifications".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::realtime_event_to_dart;
    use im_core::{
        ids::{GroupRef, MessageId, PeerRef, ThreadId},
        messages::{
            Message, MessageBodyView, MessageDirection, MessageKind, MessageMetadata, ThreadRef,
        },
        realtime::{
            ConnectionStateChanged, GroupUpdateKind, GroupUpdatedEvent, HostNotificationEvent,
            HostNotificationKind, ImEvent, LocalNotificationEvent, MessageReceivedEvent,
            MessageUpdateKind, MessageUpdatedEvent, RealtimeConnectionState,
            UnknownNotificationEvent,
        },
    };

    #[test]
    fn realtime_event_mapping_preserves_connection_and_message_events() {
        let event =
            realtime_event_to_dart(ImEvent::ConnectionStateChanged(ConnectionStateChanged {
                state: RealtimeConnectionState::Connected,
                reason: Some("ready".to_string()),
            }));
        assert_eq!(event.kind, "connection_state_changed");
        assert_eq!(event.state.as_deref(), Some("connected"));
        assert_eq!(event.reason.as_deref(), Some("ready"));

        let event = realtime_event_to_dart(ImEvent::MessageReceived(MessageReceivedEvent {
            message: Message {
                id: MessageId::parse("msg-dart-map-1").unwrap(),
                thread: ThreadRef::Direct(PeerRef::parse("did:example:alice", "").unwrap()),
                direction: MessageDirection::Incoming,
                sender: PeerRef::parse("did:example:bob", "").unwrap(),
                receiver: Some(PeerRef::parse("did:example:alice", "").unwrap()),
                group: None,
                body: MessageBodyView::Text {
                    text: "hello".to_string(),
                    kind: MessageKind::Text,
                },
                sent_at: None,
                received_at: None,
                metadata: MessageMetadata::default(),
            },
            attachment_summary: None,
            download_action: None,
            warnings: Vec::new(),
        }));
        assert_eq!(event.kind, "message_received");
        let message = event.message.expect("message payload");
        assert_eq!(message.id, "msg-dart-map-1");
        assert_eq!(message.thread_kind, "direct");
        assert_eq!(message.body.text.as_deref(), Some("hello"));
    }

    #[test]
    fn realtime_event_mapping_preserves_group_host_local_and_unknown_events() {
        let group = realtime_event_to_dart(ImEvent::GroupUpdated(GroupUpdatedEvent {
            group: GroupRef::parse("did:example:group").unwrap(),
            update_kind: GroupUpdateKind::MessageAdded,
        }));
        assert_eq!(group.kind, "group_updated");
        assert_eq!(group.group.as_deref(), Some("did:example:group"));
        assert_eq!(group.update_kind.as_deref(), Some("message_added"));

        let message_update = realtime_event_to_dart(ImEvent::MessageUpdated(MessageUpdatedEvent {
            message_id: MessageId::parse("msg-dart-map-2").unwrap(),
            thread: ThreadRef::Thread(ThreadId::parse("thread-1").unwrap()),
            update_kind: MessageUpdateKind::DeliveryStateChanged,
        }));
        assert_eq!(message_update.kind, "message_updated");
        assert_eq!(message_update.message_id.as_deref(), Some("msg-dart-map-2"));
        assert_eq!(message_update.thread_kind.as_deref(), Some("thread"));
        assert_eq!(message_update.thread_id.as_deref(), Some("thread-1"));
        assert_eq!(
            message_update.update_kind.as_deref(),
            Some("delivery_state_changed")
        );

        let local = realtime_event_to_dart(ImEvent::LocalNotification(LocalNotificationEvent {
            notification_id: Some("local-1".to_string()),
            title: Some("Title".to_string()),
            body: Some("Body".to_string()),
            source: Some("sdk".to_string()),
        }));
        assert_eq!(local.kind, "local_notification");
        assert_eq!(local.notification_id.as_deref(), Some("local-1"));
        assert_eq!(local.source.as_deref(), Some("sdk"));

        let host = realtime_event_to_dart(ImEvent::HostNotification(HostNotificationEvent {
            event_type: HostNotificationKind::GroupState,
            title: Some("Host".to_string()),
            body: None,
            thread: Some(ThreadRef::Group(
                GroupRef::parse("did:example:group").unwrap(),
            )),
        }));
        assert_eq!(host.kind, "host_notification");
        assert_eq!(host.host_kind.as_deref(), Some("group_state"));
        assert_eq!(host.thread_kind.as_deref(), Some("group"));

        let unknown =
            realtime_event_to_dart(ImEvent::UnknownNotification(UnknownNotificationEvent {
                content_type: Some("application/json".to_string()),
                notification_type: Some("custom.event".to_string()),
                reason: "unsupported notification".to_string(),
            }));
        assert_eq!(unknown.kind, "unknown_notification");
        assert_eq!(unknown.content_type.as_deref(), Some("application/json"));
        assert_eq!(unknown.notification_type.as_deref(), Some("custom.event"));
        assert_eq!(unknown.reason.as_deref(), Some("unsupported notification"));
    }
}
