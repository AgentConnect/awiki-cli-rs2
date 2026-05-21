//! Migration-only local state helpers for `awiki-cli` wrappers.

#[cfg(feature = "sqlite")]
pub const SCHEMA_VERSION: i64 = crate::internal::local_state::schema::SCHEMA_VERSION;

#[cfg(feature = "sqlite")]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MessageRecord {
    pub msg_id: String,
    pub owner_identity_id: String,
    pub owner_did: String,
    pub thread_id: String,
    pub direction: i64,
    pub sender_did: String,
    pub receiver_did: String,
    pub group_id: String,
    pub group_did: String,
    pub content_type: String,
    pub content: String,
    pub title: String,
    pub server_seq: Option<i64>,
    pub sent_at: String,
    pub stored_at: String,
    pub is_e2ee: bool,
    pub is_read: bool,
    pub sender_name: String,
    pub metadata: String,
    pub credential_name: String,
}

#[cfg(feature = "sqlite")]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContactRecord {
    pub owner_identity_id: String,
    pub owner_did: String,
    pub did: String,
    pub name: String,
    pub handle: String,
    pub nick_name: String,
    pub bio: String,
    pub profile_md: String,
    pub tags: String,
    pub relationship: String,
    pub source_type: String,
    pub source_name: String,
    pub source_group_id: String,
    pub connected_at: String,
    pub recommended_reason: String,
    pub followed: Option<bool>,
    pub messaged: Option<bool>,
    pub note: String,
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub metadata: String,
    pub credential_name: String,
}

#[cfg(feature = "sqlite")]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupRecord {
    pub owner_identity_id: String,
    pub owner_did: String,
    pub group_id: String,
    pub group_did: String,
    pub name: String,
    pub group_mode: String,
    pub slug: String,
    pub description: String,
    pub goal: String,
    pub rules: String,
    pub message_prompt: String,
    pub doc_url: String,
    pub group_owner_did: String,
    pub group_owner_handle: String,
    pub my_role: String,
    pub membership_status: String,
    pub join_enabled: Option<bool>,
    pub join_code: String,
    pub join_code_expires_at: String,
    pub member_count: Option<i64>,
    pub last_synced_seq: Option<i64>,
    pub last_read_seq: Option<i64>,
    pub last_message_at: String,
    pub remote_created_at: String,
    pub remote_updated_at: String,
    pub stored_at: String,
    pub metadata: String,
    pub credential_name: String,
}

#[cfg(feature = "sqlite")]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupMemberRecord {
    pub owner_identity_id: String,
    pub owner_did: String,
    pub group_id: String,
    pub user_id: String,
    pub member_did: String,
    pub member_handle: String,
    pub profile_url: String,
    pub role: String,
    pub status: String,
    pub joined_at: String,
    pub sent_message_count: Option<i64>,
    pub last_synced_at: String,
    pub metadata: String,
    pub credential_name: String,
}

#[cfg(feature = "sqlite")]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConversationRecord {
    pub owner_did: String,
    pub thread_id: String,
    pub message_count: i64,
    pub unread_count: i64,
    pub last_message_at: String,
    pub last_content: String,
}

#[cfg(feature = "sqlite")]
#[doc(hidden)]
pub fn ensure_schema(connection: &rusqlite::Connection) -> crate::ImResult<()> {
    crate::internal::local_state::schema::ensure_schema(connection)
}

#[cfg(feature = "sqlite")]
#[doc(hidden)]
pub fn current_schema_version(connection: &rusqlite::Connection) -> crate::ImResult<i64> {
    crate::internal::local_state::schema::current_schema_version(connection)
}

#[cfg(feature = "sqlite")]
impl From<MessageRecord> for crate::internal::local_state::messages::MessageRecord {
    fn from(record: MessageRecord) -> Self {
        Self {
            msg_id: record.msg_id,
            owner_identity_id: record.owner_identity_id,
            owner_did: record.owner_did,
            thread_id: record.thread_id,
            direction: record.direction,
            sender_did: record.sender_did,
            receiver_did: record.receiver_did,
            group_id: record.group_id,
            group_did: record.group_did,
            content_type: record.content_type,
            content: record.content,
            title: record.title,
            server_seq: record.server_seq,
            sent_at: record.sent_at,
            stored_at: record.stored_at,
            is_e2ee: record.is_e2ee,
            is_read: record.is_read,
            sender_name: record.sender_name,
            metadata: record.metadata,
            credential_name: record.credential_name,
        }
    }
}

#[cfg(feature = "sqlite")]
impl From<ContactRecord> for crate::internal::local_state::contacts::ContactRecord {
    fn from(record: ContactRecord) -> Self {
        Self {
            owner_did: record.owner_did,
            owner_identity_id: record.owner_identity_id,
            did: record.did,
            name: record.name,
            handle: record.handle,
            nick_name: record.nick_name,
            bio: record.bio,
            profile_md: record.profile_md,
            tags: record.tags,
            relationship: record.relationship,
            source_type: record.source_type,
            source_name: record.source_name,
            source_group_id: record.source_group_id,
            connected_at: record.connected_at,
            recommended_reason: record.recommended_reason,
            followed: record.followed,
            messaged: record.messaged,
            note: record.note,
            first_seen_at: record.first_seen_at,
            last_seen_at: record.last_seen_at,
            metadata: record.metadata,
            credential_name: record.credential_name,
        }
    }
}

#[cfg(feature = "sqlite")]
impl From<GroupRecord> for crate::internal::local_state::groups::GroupRecord {
    fn from(record: GroupRecord) -> Self {
        Self {
            owner_did: record.owner_did,
            owner_identity_id: record.owner_identity_id,
            group_id: record.group_id,
            group_did: record.group_did,
            name: record.name,
            group_mode: record.group_mode,
            slug: record.slug,
            description: record.description,
            goal: record.goal,
            rules: record.rules,
            message_prompt: record.message_prompt,
            doc_url: record.doc_url,
            group_owner_did: record.group_owner_did,
            group_owner_handle: record.group_owner_handle,
            my_role: record.my_role,
            membership_status: record.membership_status,
            join_enabled: record.join_enabled,
            join_code: record.join_code,
            join_code_expires_at: record.join_code_expires_at,
            member_count: record.member_count,
            last_synced_seq: record.last_synced_seq,
            last_read_seq: record.last_read_seq,
            last_message_at: record.last_message_at,
            remote_created_at: record.remote_created_at,
            remote_updated_at: record.remote_updated_at,
            stored_at: record.stored_at,
            metadata: record.metadata,
            credential_name: record.credential_name,
        }
    }
}

#[cfg(feature = "sqlite")]
impl From<GroupMemberRecord> for crate::internal::local_state::groups::GroupMemberRecord {
    fn from(record: GroupMemberRecord) -> Self {
        Self {
            owner_did: record.owner_did,
            owner_identity_id: record.owner_identity_id,
            group_id: record.group_id,
            user_id: record.user_id,
            member_did: record.member_did,
            member_handle: record.member_handle,
            profile_url: record.profile_url,
            role: record.role,
            status: record.status,
            joined_at: record.joined_at,
            sent_message_count: record.sent_message_count,
            last_synced_at: record.last_synced_at,
            metadata: record.metadata,
            credential_name: record.credential_name,
        }
    }
}
