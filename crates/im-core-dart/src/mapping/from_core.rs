use crate::dto::{
    auth::{DartAuthScope, DartAuthStatus, DartSessionBundle, DartSessionUpdate},
    directory::{DartDirectoryResolution, DartRelationStatus},
    group::{DartGroupMember, DartGroupReadResult, DartGroupSnapshot, DartGroupSummary},
    identity::DartIdentitySummary,
    message::{
        DartConversation, DartConversationPage, DartMarkReadResult, DartMessage,
        DartMessageBodyView, DartMessageDirection, DartMessageMetadata,
        DartMessageMetadataAttribute, DartMessagePage, DartSendMessageResult,
    },
    profile::DartUserProfile,
    realtime::DartRealtimeStatus,
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
        }
    }
}

impl From<im_core::identity::Profile> for DartUserProfile {
    fn from(value: im_core::identity::Profile) -> Self {
        Self {
            subject: value.subject.as_str().to_string(),
            handle: value.handle.map(|handle| handle.as_str().to_string()),
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
                unsupported_content_type: None,
            },
            im_core::messages::MessageBodyView::Unsupported { content_type } => Self {
                text: None,
                kind: None,
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

fn delivery_state_to_string(value: im_core::messages::DeliveryState) -> String {
    match value {
        im_core::messages::DeliveryState::Accepted => "accepted".to_string(),
        im_core::messages::DeliveryState::Sent => "sent".to_string(),
        im_core::messages::DeliveryState::StoredLocally => "stored_locally".to_string(),
        im_core::messages::DeliveryState::Failed { reason } => format!("failed:{reason}"),
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

fn realtime_state_to_string(value: im_core::realtime::RealtimeConnectionState) -> String {
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
