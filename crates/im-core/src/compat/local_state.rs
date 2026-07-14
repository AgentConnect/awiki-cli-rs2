//! Migration-only local state helpers for `awiki-cli` wrappers.

#[cfg(feature = "sqlite")]
use std::collections::BTreeMap;

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerInvariantViolation {
    pub table: String,
    pub invariant: String,
    pub row_count: i64,
}

#[cfg(feature = "sqlite")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalInvariantViolation {
    pub table: String,
    pub invariant: String,
    pub row_count: i64,
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
#[doc(hidden)]
pub fn ensure_identity_owned_schema(connection: &rusqlite::Connection) -> crate::ImResult<()> {
    crate::internal::local_state::schema::ensure_schema(connection)
}

#[cfg(feature = "sqlite")]
#[doc(hidden)]
pub fn identity_owned_owner_invariants(
    connection: &rusqlite::Connection,
) -> crate::ImResult<Vec<OwnerInvariantViolation>> {
    crate::internal::local_state::schema::identity_owned_owner_invariants(
        connection,
        crate::internal::local_state::schema::IdentityOwnedSchemaTableMode::Final,
    )
    .map(|violations| {
        violations
            .into_iter()
            .map(|violation| OwnerInvariantViolation {
                table: violation.table.to_string(),
                invariant: violation.invariant.to_string(),
                row_count: violation.row_count,
            })
            .collect()
    })
}

#[cfg(feature = "sqlite")]
#[doc(hidden)]
pub fn canonical_conversation_invariants(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
) -> crate::ImResult<Vec<CanonicalInvariantViolation>> {
    crate::internal::local_state::canonical_invariants::check(connection, owner_identity_id).map(
        |violations| {
            violations
                .into_iter()
                .map(|violation| CanonicalInvariantViolation {
                    table: violation.table.to_owned(),
                    invariant: violation.invariant.to_owned(),
                    row_count: violation.row_count,
                })
                .collect()
        },
    )
}

#[cfg(feature = "sqlite")]
#[doc(hidden)]
pub fn record_identity_did_history_transition<S: AsRef<str>>(
    connection: &mut rusqlite::Connection,
    owner_identity_id: &str,
    current_did: &str,
    previous_dids: &[S],
) -> crate::ImResult<BTreeMap<String, i64>> {
    crate::internal::local_state::schema::record_identity_did_history_transition(
        connection,
        owner_identity_id,
        current_did,
        previous_dids,
    )
}

#[cfg(feature = "sqlite")]
#[doc(hidden)]
pub fn list_email_notifications_for_test(
    sqlite_path: &std::path::Path,
    owner_identity_id: &str,
    owner_did: &str,
    limit: crate::ids::PageLimit,
) -> crate::ImResult<crate::ids::Page<crate::email::EmailNotification>> {
    crate::internal::local_state::email::list_mail_notifications(
        sqlite_path,
        owner_identity_id,
        owner_did,
        limit,
    )
}

#[cfg(feature = "sqlite")]
#[doc(hidden)]
pub fn plan_replace_did_local_state_rebind(
    sqlite_path: &std::path::Path,
    old_owner_did: &str,
    new_owner_did: &str,
) -> crate::ImResult<(BTreeMap<String, i64>, BTreeMap<String, i64>)> {
    let affected = crate::internal::identity_replace_did_plan::plan_replace_did_local_state_rebind(
        sqlite_path,
        old_owner_did,
        new_owner_did,
    )?;
    Ok((affected.store_rebind_counts, affected.e2ee_cleanup_counts))
}

#[cfg(feature = "sqlite")]
impl From<MessageRecord> for crate::internal::local_state::messages::MessageRecord {
    fn from(record: MessageRecord) -> Self {
        Self {
            msg_id: record.msg_id,
            owner_identity_id: record.owner_identity_id,
            owner_did: record.owner_did,
            conversation_id: String::new(),
            wire_thread_kind: String::new(),
            wire_thread_ref: String::new(),
            wire_identity_resolution_state: String::new(),
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
            mentions_current_user: false,
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
            membership_id: String::new(),
            peer_persona_id: String::new(),
            member_did: record.member_did.clone(),
            member_credential_did: record.member_did.clone(),
            anchor_kind: "did".to_owned(),
            anchor_value: record.member_did.clone(),
            member_handle: record.member_handle,
            handle_binding_generation: String::new(),
            membership_epoch: String::new(),
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
