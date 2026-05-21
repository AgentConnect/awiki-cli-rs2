#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ConversationRecord {
    pub(crate) owner_did: String,
    pub(crate) thread_id: String,
    pub(crate) message_count: i64,
    pub(crate) unread_count: i64,
    pub(crate) last_message_at: String,
    pub(crate) last_content: String,
}
