use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImEvent {
    ConnectionStateChanged(ConnectionStateChanged),
    MessageReceived(MessageReceivedEvent),
    MessageUpdated(MessageUpdatedEvent),
    GroupUpdated(GroupUpdatedEvent),
    SystemNotificationChanged(SystemNotificationChangedEvent),
    LocalNotification(LocalNotificationEvent),
    HostNotification(HostNotificationEvent),
    UnknownNotification(UnknownNotificationEvent),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemNotificationChangedEvent {
    pub notification: crate::system_notifications::SystemNotificationSnapshot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync: Option<RealtimeSyncHint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionStateChanged {
    pub state: super::RealtimeConnectionState,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageReceivedEvent {
    pub message: crate::messages::Message,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachment_summary: Option<AttachmentMessageSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_action: Option<AttachmentDownloadAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync: Option<RealtimeSyncHint>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentMessageSummary {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachment_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentDownloadAction {
    pub thread: crate::messages::ThreadRef,
    pub message_id: crate::ids::MessageId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachment_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageUpdatedEvent {
    pub message_id: crate::ids::MessageId,
    pub thread: crate::messages::ThreadRef,
    pub update_kind: MessageUpdateKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync: Option<RealtimeSyncHint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageUpdateKind {
    Read,
    DeliveryStateChanged,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupUpdatedEvent {
    pub group: crate::ids::GroupRef,
    pub update_kind: GroupUpdateKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_event_seq: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_state_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_did: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_did: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_handle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_subject_did: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle_binding_generation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub membership_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync: Option<RealtimeSyncHint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GroupUpdateKind {
    Created,
    Updated,
    MemberAdded,
    MemberRemoved,
    MessageAdded,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalNotificationEvent {
    pub notification_id: Option<String>,
    pub title: Option<String>,
    pub body: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostNotificationEvent {
    pub event_type: HostNotificationKind,
    pub title: Option<String>,
    pub body: Option<String>,
    pub thread: Option<crate::messages::ThreadRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HostNotificationKind {
    DirectMessage,
    GroupMessage,
    GroupState,
    Mail,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnknownNotificationEvent {
    pub content_type: Option<String>,
    pub notification_type: Option<String>,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync: Option<RealtimeSyncHint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RealtimeSyncHint {
    /// Legacy v1 transport metadata. This stays inside Core and is not exposed
    /// through the application SDK boundary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    /// Legacy v1 sequence, or the v2 account scan high-water hint. A hint never
    /// advances a durable sync cursor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_seq: Option<String>,
    /// Legacy v1 transport metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_type: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub domains: BTreeSet<SyncDomain>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub sync_dirty: bool,
    pub gap_detected: bool,
    pub has_unknown_domain: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncDomain {
    Message,
    Profile,
    AgentInventory,
    AgentStatus,
    DeviceRegistry,
}
