use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImEvent {
    ConnectionStateChanged(ConnectionStateChanged),
    MessageReceived(MessageReceivedEvent),
    MessageUpdated(MessageUpdatedEvent),
    GroupUpdated(GroupUpdatedEvent),
    LocalNotification(LocalNotificationEvent),
    HostNotification(HostNotificationEvent),
    UnknownNotification(UnknownNotificationEvent),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionStateChanged {
    pub state: super::RealtimeConnectionState,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageReceivedEvent {
    pub message: crate::messages::Message,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageUpdatedEvent {
    pub message_id: crate::ids::MessageId,
    pub thread: crate::messages::ThreadRef,
    pub update_kind: MessageUpdateKind,
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
}
