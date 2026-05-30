use super::sql::{
    bool_from_row, bool_ptr_from_row, float64_ptr_from_row, int64_ptr_from_row, int_from_row,
    string_from_row, RowMap,
};
use crate::internal::identity_recover_local_state::helpers::{
    default_string, make_thread_id, normalize_owner_did,
};
use std::collections::BTreeSet;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

#[derive(Debug, Clone)]
pub(super) struct MessageRecord {
    pub(super) msg_id: String,
    pub(super) owner_identity_id: String,
    pub(super) owner_did: String,
    pub(super) conversation_id: String,
    pub(super) thread_id: String,
    pub(super) direction: i64,
    pub(super) sender_did: String,
    pub(super) receiver_did: String,
    pub(super) group_id: String,
    pub(super) group_did: String,
    pub(super) content_type: String,
    pub(super) content: String,
    pub(super) title: String,
    pub(super) server_seq: Option<i64>,
    pub(super) sent_at: String,
    pub(super) stored_at: String,
    pub(super) is_e2ee: bool,
    pub(super) is_read: bool,
    pub(super) sender_name: String,
    pub(super) metadata: String,
    pub(super) credential_name: String,
}

#[derive(Debug, Clone)]
pub(super) struct ContactRecord {
    pub(super) owner_identity_id: String,
    pub(super) owner_did: String,
    pub(super) did: String,
    pub(super) name: String,
    pub(super) handle: String,
    pub(super) nick_name: String,
    pub(super) bio: String,
    pub(super) profile_md: String,
    pub(super) tags: String,
    pub(super) relationship: String,
    pub(super) source_type: String,
    pub(super) source_name: String,
    pub(super) source_group_id: String,
    pub(super) connected_at: String,
    pub(super) recommended_reason: String,
    pub(super) followed: Option<bool>,
    pub(super) messaged: Option<bool>,
    pub(super) note: String,
    pub(super) first_seen_at: String,
    pub(super) last_seen_at: String,
    pub(super) metadata: String,
    pub(super) credential_name: String,
}

#[derive(Debug, Clone)]
pub(super) struct ContactHandleBindingRecord {
    pub(super) owner_identity_id: String,
    pub(super) owner_did: String,
    pub(super) handle: String,
    pub(super) did: String,
    pub(super) is_current: bool,
    pub(super) first_seen_at: String,
    pub(super) last_seen_at: String,
    pub(super) source_type: String,
    pub(super) source_group_id: String,
    pub(super) metadata: String,
    pub(super) credential_name: String,
}

#[derive(Debug, Clone)]
pub(super) struct RelationshipEventRecord {
    pub(super) event_id: String,
    pub(super) owner_identity_id: String,
    pub(super) owner_did: String,
    pub(super) target_did: String,
    pub(super) target_handle: String,
    pub(super) event_type: String,
    pub(super) source_type: String,
    pub(super) source_name: String,
    pub(super) source_group_id: String,
    pub(super) reason: String,
    pub(super) score: Option<f64>,
    pub(super) status: String,
    pub(super) created_at: String,
    pub(super) updated_at: String,
    pub(super) metadata: String,
    pub(super) credential_name: String,
}

#[derive(Debug, Clone)]
pub(super) struct GroupRecord {
    pub(super) owner_identity_id: String,
    pub(super) owner_did: String,
    pub(super) group_id: String,
    pub(super) group_did: String,
    pub(super) name: String,
    pub(super) group_mode: String,
    pub(super) slug: String,
    pub(super) description: String,
    pub(super) goal: String,
    pub(super) rules: String,
    pub(super) message_prompt: String,
    pub(super) doc_url: String,
    pub(super) group_owner_did: String,
    pub(super) group_owner_handle: String,
    pub(super) my_role: String,
    pub(super) membership_status: String,
    pub(super) join_enabled: Option<bool>,
    pub(super) join_code: String,
    pub(super) join_code_expires_at: String,
    pub(super) member_count: Option<i64>,
    pub(super) last_synced_seq: Option<i64>,
    pub(super) last_read_seq: Option<i64>,
    pub(super) last_message_at: String,
    pub(super) remote_created_at: String,
    pub(super) remote_updated_at: String,
    pub(super) stored_at: String,
    pub(super) metadata: String,
    pub(super) credential_name: String,
}

#[derive(Debug, Clone)]
pub(super) struct GroupMemberRecord {
    pub(super) owner_identity_id: String,
    pub(super) owner_did: String,
    pub(super) group_id: String,
    pub(super) user_id: String,
    pub(super) member_did: String,
    pub(super) member_handle: String,
    pub(super) profile_url: String,
    pub(super) role: String,
    pub(super) status: String,
    pub(super) joined_at: String,
    pub(super) sent_message_count: Option<i64>,
    pub(super) last_synced_at: String,
    pub(super) metadata: String,
    pub(super) credential_name: String,
}

pub(super) fn normalize_recovered_message_row(
    row: &RowMap,
    old_owner_set: &BTreeSet<String>,
    new_owner_did: &str,
    final_owner_identity_id: &str,
    final_credential_name: &str,
) -> MessageRecord {
    let sender_did = remap_recovered_self_did(
        &string_from_row(row, "sender_did"),
        old_owner_set,
        new_owner_did,
    );
    let receiver_did = remap_recovered_self_did(
        &string_from_row(row, "receiver_did"),
        old_owner_set,
        new_owner_did,
    );
    let group_id = string_from_row(row, "group_id");
    let group_did = string_from_row(row, "group_did");
    let conversation_id =
        if let Some(group_key) = first_non_empty([group_id.as_str(), group_did.as_str()]) {
            make_thread_id(new_owner_did, "", group_key)
        } else {
            let peer_did = first_recovered_peer_did(&sender_did, &receiver_did, new_owner_did);
            make_thread_id(new_owner_did, &peer_did, "")
        };
    MessageRecord {
        msg_id: string_from_row(row, "msg_id"),
        owner_identity_id: final_owner_identity_id.to_string(),
        owner_did: new_owner_did.to_string(),
        conversation_id: conversation_id.clone(),
        thread_id: conversation_id,
        direction: int_from_row(row, "direction"),
        sender_did,
        receiver_did,
        group_id,
        group_did,
        content_type: string_from_row(row, "content_type"),
        content: string_from_row(row, "content"),
        title: string_from_row(row, "title"),
        server_seq: int64_ptr_from_row(row, "server_seq"),
        sent_at: string_from_row(row, "sent_at"),
        stored_at: string_from_row(row, "stored_at"),
        is_e2ee: bool_from_row(row, "is_e2ee"),
        is_read: bool_from_row(row, "is_read"),
        sender_name: string_from_row(row, "sender_name"),
        metadata: string_from_row(row, "metadata"),
        credential_name: final_credential_name.to_string(),
    }
}

pub(super) fn normalize_recovered_contact_row(
    row: &RowMap,
    new_owner_did: &str,
    final_owner_identity_id: &str,
    final_credential_name: &str,
) -> ContactRecord {
    ContactRecord {
        owner_identity_id: final_owner_identity_id.to_string(),
        owner_did: new_owner_did.to_string(),
        did: string_from_row(row, "did"),
        name: string_from_row(row, "name"),
        handle: string_from_row(row, "handle"),
        nick_name: string_from_row(row, "nick_name"),
        bio: string_from_row(row, "bio"),
        profile_md: string_from_row(row, "profile_md"),
        tags: string_from_row(row, "tags"),
        relationship: string_from_row(row, "relationship"),
        source_type: string_from_row(row, "source_type"),
        source_name: string_from_row(row, "source_name"),
        source_group_id: string_from_row(row, "source_group_id"),
        connected_at: string_from_row(row, "connected_at"),
        recommended_reason: string_from_row(row, "recommended_reason"),
        followed: bool_ptr_from_row(row, "followed"),
        messaged: bool_ptr_from_row(row, "messaged"),
        note: string_from_row(row, "note"),
        first_seen_at: string_from_row(row, "first_seen_at"),
        last_seen_at: string_from_row(row, "last_seen_at"),
        metadata: string_from_row(row, "metadata"),
        credential_name: final_credential_name.to_string(),
    }
}

pub(super) fn normalize_recovered_contact_handle_binding_row(
    row: &RowMap,
    new_owner_did: &str,
    final_owner_identity_id: &str,
    final_credential_name: &str,
) -> ContactHandleBindingRecord {
    ContactHandleBindingRecord {
        owner_identity_id: final_owner_identity_id.to_string(),
        owner_did: new_owner_did.to_string(),
        handle: string_from_row(row, "handle"),
        did: string_from_row(row, "did"),
        is_current: false,
        first_seen_at: string_from_row(row, "first_seen_at"),
        last_seen_at: string_from_row(row, "last_seen_at"),
        source_type: string_from_row(row, "source_type"),
        source_group_id: string_from_row(row, "source_group_id"),
        metadata: string_from_row(row, "metadata"),
        credential_name: final_credential_name.to_string(),
    }
}

pub(super) fn normalize_recovered_relationship_event_row(
    row: &RowMap,
    new_owner_did: &str,
    final_owner_identity_id: &str,
    final_credential_name: &str,
) -> RelationshipEventRecord {
    RelationshipEventRecord {
        event_id: string_from_row(row, "event_id"),
        owner_identity_id: final_owner_identity_id.to_string(),
        owner_did: new_owner_did.to_string(),
        target_did: string_from_row(row, "target_did"),
        target_handle: string_from_row(row, "target_handle"),
        event_type: string_from_row(row, "event_type"),
        source_type: string_from_row(row, "source_type"),
        source_name: string_from_row(row, "source_name"),
        source_group_id: string_from_row(row, "source_group_id"),
        reason: string_from_row(row, "reason"),
        score: float64_ptr_from_row(row, "score"),
        status: string_from_row(row, "status"),
        created_at: string_from_row(row, "created_at"),
        updated_at: string_from_row(row, "updated_at"),
        metadata: string_from_row(row, "metadata"),
        credential_name: final_credential_name.to_string(),
    }
}

pub(super) fn normalize_recovered_group_row(
    row: &RowMap,
    old_owner_set: &BTreeSet<String>,
    new_owner_did: &str,
    final_owner_identity_id: &str,
    final_credential_name: &str,
) -> GroupRecord {
    GroupRecord {
        owner_identity_id: final_owner_identity_id.to_string(),
        owner_did: new_owner_did.to_string(),
        group_id: string_from_row(row, "group_id"),
        group_did: string_from_row(row, "group_did"),
        name: string_from_row(row, "name"),
        group_mode: string_from_row(row, "group_mode"),
        slug: string_from_row(row, "slug"),
        description: string_from_row(row, "description"),
        goal: string_from_row(row, "goal"),
        rules: string_from_row(row, "rules"),
        message_prompt: string_from_row(row, "message_prompt"),
        doc_url: string_from_row(row, "doc_url"),
        group_owner_did: remap_recovered_self_did(
            &string_from_row(row, "group_owner_did"),
            old_owner_set,
            new_owner_did,
        ),
        group_owner_handle: string_from_row(row, "group_owner_handle"),
        my_role: string_from_row(row, "my_role"),
        membership_status: string_from_row(row, "membership_status"),
        join_enabled: bool_ptr_from_row(row, "join_enabled"),
        join_code: string_from_row(row, "join_code"),
        join_code_expires_at: string_from_row(row, "join_code_expires_at"),
        member_count: int64_ptr_from_row(row, "member_count"),
        last_synced_seq: int64_ptr_from_row(row, "last_synced_seq"),
        last_read_seq: int64_ptr_from_row(row, "last_read_seq"),
        last_message_at: string_from_row(row, "last_message_at"),
        remote_created_at: string_from_row(row, "remote_created_at"),
        remote_updated_at: string_from_row(row, "remote_updated_at"),
        stored_at: string_from_row(row, "stored_at"),
        metadata: string_from_row(row, "metadata"),
        credential_name: final_credential_name.to_string(),
    }
}

pub(super) fn normalize_recovered_group_member_row(
    row: &RowMap,
    old_owner_set: &BTreeSet<String>,
    new_owner_did: &str,
    final_owner_identity_id: &str,
    final_credential_name: &str,
) -> GroupMemberRecord {
    GroupMemberRecord {
        owner_identity_id: final_owner_identity_id.to_string(),
        owner_did: new_owner_did.to_string(),
        group_id: string_from_row(row, "group_id"),
        user_id: string_from_row(row, "user_id"),
        member_did: remap_recovered_self_did(
            &string_from_row(row, "member_did"),
            old_owner_set,
            new_owner_did,
        ),
        member_handle: string_from_row(row, "member_handle"),
        profile_url: string_from_row(row, "profile_url"),
        role: string_from_row(row, "role"),
        status: string_from_row(row, "status"),
        joined_at: string_from_row(row, "joined_at"),
        sent_message_count: int64_ptr_from_row(row, "sent_message_count"),
        last_synced_at: string_from_row(row, "last_synced_at"),
        metadata: string_from_row(row, "metadata"),
        credential_name: final_credential_name.to_string(),
    }
}

pub(super) fn merge_recovered_message(existing: &RowMap, incoming: MessageRecord) -> MessageRecord {
    MessageRecord {
        conversation_id: default_string(
            incoming.conversation_id.clone(),
            &string_from_row(existing, "conversation_id"),
        ),
        thread_id: default_string(
            incoming.thread_id.clone(),
            &string_from_row(existing, "thread_id"),
        ),
        direction: incoming.direction,
        sender_did: choose_later_non_empty(
            &string_from_row(existing, "sender_did"),
            incoming.sender_did.clone(),
        ),
        receiver_did: choose_later_non_empty(
            &string_from_row(existing, "receiver_did"),
            incoming.receiver_did.clone(),
        ),
        group_id: choose_later_non_empty(
            &string_from_row(existing, "group_id"),
            incoming.group_id.clone(),
        ),
        group_did: choose_later_non_empty(
            &string_from_row(existing, "group_did"),
            incoming.group_did.clone(),
        ),
        content_type: choose_later_non_empty(
            &string_from_row(existing, "content_type"),
            incoming.content_type.clone(),
        ),
        content: choose_later_non_empty(
            &string_from_row(existing, "content"),
            incoming.content.clone(),
        ),
        title: choose_later_non_empty(&string_from_row(existing, "title"), incoming.title.clone()),
        server_seq: max_int64_ptr(
            int64_ptr_from_row(existing, "server_seq"),
            incoming.server_seq,
        ),
        sent_at: later_time_string(
            &string_from_row(existing, "sent_at"),
            incoming.sent_at.clone(),
        ),
        stored_at: later_time_string(
            &string_from_row(existing, "stored_at"),
            incoming.stored_at.clone(),
        ),
        is_e2ee: bool_from_row(existing, "is_e2ee") || incoming.is_e2ee,
        is_read: bool_from_row(existing, "is_read") || incoming.is_read,
        sender_name: choose_later_non_empty(
            &string_from_row(existing, "sender_name"),
            incoming.sender_name.clone(),
        ),
        metadata: choose_later_non_empty(
            &string_from_row(existing, "metadata"),
            incoming.metadata.clone(),
        ),
        owner_identity_id: choose_later_non_empty(
            &string_from_row(existing, "owner_identity_id"),
            incoming.owner_identity_id.clone(),
        ),
        credential_name: choose_later_non_empty(
            &string_from_row(existing, "credential_name"),
            incoming.credential_name.clone(),
        ),
        ..incoming
    }
}

pub(super) fn merge_recovered_contact(existing: &RowMap, incoming: ContactRecord) -> ContactRecord {
    ContactRecord {
        name: choose_later_non_empty(&string_from_row(existing, "name"), incoming.name.clone()),
        handle: choose_later_non_empty(
            &string_from_row(existing, "handle"),
            incoming.handle.clone(),
        ),
        nick_name: choose_later_non_empty(
            &string_from_row(existing, "nick_name"),
            incoming.nick_name.clone(),
        ),
        bio: choose_later_non_empty(&string_from_row(existing, "bio"), incoming.bio.clone()),
        profile_md: choose_later_non_empty(
            &string_from_row(existing, "profile_md"),
            incoming.profile_md.clone(),
        ),
        tags: choose_later_non_empty(&string_from_row(existing, "tags"), incoming.tags.clone()),
        relationship: choose_later_non_empty(
            &string_from_row(existing, "relationship"),
            incoming.relationship.clone(),
        ),
        source_type: choose_later_non_empty(
            &string_from_row(existing, "source_type"),
            incoming.source_type.clone(),
        ),
        source_name: choose_later_non_empty(
            &string_from_row(existing, "source_name"),
            incoming.source_name.clone(),
        ),
        source_group_id: choose_later_non_empty(
            &string_from_row(existing, "source_group_id"),
            incoming.source_group_id.clone(),
        ),
        connected_at: later_time_string(
            &string_from_row(existing, "connected_at"),
            incoming.connected_at.clone(),
        ),
        recommended_reason: choose_later_non_empty(
            &string_from_row(existing, "recommended_reason"),
            incoming.recommended_reason.clone(),
        ),
        followed: Some(bool_from_row(existing, "followed") || bool_from_ptr(incoming.followed)),
        messaged: Some(bool_from_row(existing, "messaged") || bool_from_ptr(incoming.messaged)),
        note: choose_later_non_empty(&string_from_row(existing, "note"), incoming.note.clone()),
        first_seen_at: earlier_time_string(
            &string_from_row(existing, "first_seen_at"),
            incoming.first_seen_at.clone(),
        ),
        last_seen_at: later_time_string(
            &string_from_row(existing, "last_seen_at"),
            incoming.last_seen_at.clone(),
        ),
        metadata: choose_later_non_empty(
            &string_from_row(existing, "metadata"),
            incoming.metadata.clone(),
        ),
        owner_identity_id: choose_later_non_empty(
            &string_from_row(existing, "owner_identity_id"),
            incoming.owner_identity_id.clone(),
        ),
        credential_name: choose_later_non_empty(
            &string_from_row(existing, "credential_name"),
            incoming.credential_name.clone(),
        ),
        ..incoming
    }
}

pub(super) fn merge_recovered_contact_handle_binding(
    existing: &RowMap,
    incoming: ContactHandleBindingRecord,
) -> ContactHandleBindingRecord {
    ContactHandleBindingRecord {
        is_current: false,
        first_seen_at: earlier_time_string(
            &string_from_row(existing, "first_seen_at"),
            incoming.first_seen_at.clone(),
        ),
        last_seen_at: later_time_string(
            &string_from_row(existing, "last_seen_at"),
            incoming.last_seen_at.clone(),
        ),
        source_type: choose_later_non_empty(
            &string_from_row(existing, "source_type"),
            incoming.source_type.clone(),
        ),
        source_group_id: choose_later_non_empty(
            &string_from_row(existing, "source_group_id"),
            incoming.source_group_id.clone(),
        ),
        metadata: choose_later_non_empty(
            &string_from_row(existing, "metadata"),
            incoming.metadata.clone(),
        ),
        owner_identity_id: choose_later_non_empty(
            &string_from_row(existing, "owner_identity_id"),
            incoming.owner_identity_id.clone(),
        ),
        credential_name: choose_later_non_empty(
            &string_from_row(existing, "credential_name"),
            incoming.credential_name.clone(),
        ),
        ..incoming
    }
}

pub(super) fn merge_recovered_relationship_event(
    existing: &RowMap,
    incoming: RelationshipEventRecord,
) -> RelationshipEventRecord {
    let score = if incoming.score.is_none() {
        float64_ptr_from_row(existing, "score")
    } else {
        incoming.score
    };
    RelationshipEventRecord {
        target_did: choose_later_non_empty(
            &string_from_row(existing, "target_did"),
            incoming.target_did.clone(),
        ),
        target_handle: choose_later_non_empty(
            &string_from_row(existing, "target_handle"),
            incoming.target_handle.clone(),
        ),
        event_type: choose_later_non_empty(
            &string_from_row(existing, "event_type"),
            incoming.event_type.clone(),
        ),
        source_type: choose_later_non_empty(
            &string_from_row(existing, "source_type"),
            incoming.source_type.clone(),
        ),
        source_name: choose_later_non_empty(
            &string_from_row(existing, "source_name"),
            incoming.source_name.clone(),
        ),
        source_group_id: choose_later_non_empty(
            &string_from_row(existing, "source_group_id"),
            incoming.source_group_id.clone(),
        ),
        reason: choose_later_non_empty(
            &string_from_row(existing, "reason"),
            incoming.reason.clone(),
        ),
        score,
        status: choose_later_non_empty(
            &string_from_row(existing, "status"),
            incoming.status.clone(),
        ),
        created_at: earlier_time_string(
            &string_from_row(existing, "created_at"),
            incoming.created_at.clone(),
        ),
        updated_at: later_time_string(
            &string_from_row(existing, "updated_at"),
            incoming.updated_at.clone(),
        ),
        metadata: choose_later_non_empty(
            &string_from_row(existing, "metadata"),
            incoming.metadata.clone(),
        ),
        owner_identity_id: choose_later_non_empty(
            &string_from_row(existing, "owner_identity_id"),
            incoming.owner_identity_id.clone(),
        ),
        credential_name: choose_later_non_empty(
            &string_from_row(existing, "credential_name"),
            incoming.credential_name.clone(),
        ),
        ..incoming
    }
}

pub(super) fn merge_recovered_group(existing: &RowMap, incoming: GroupRecord) -> GroupRecord {
    GroupRecord {
        group_did: choose_later_non_empty(
            &string_from_row(existing, "group_did"),
            incoming.group_did.clone(),
        ),
        name: choose_later_non_empty(&string_from_row(existing, "name"), incoming.name.clone()),
        group_mode: choose_later_non_empty(
            &string_from_row(existing, "group_mode"),
            incoming.group_mode.clone(),
        ),
        slug: choose_later_non_empty(&string_from_row(existing, "slug"), incoming.slug.clone()),
        description: choose_later_non_empty(
            &string_from_row(existing, "description"),
            incoming.description.clone(),
        ),
        goal: choose_later_non_empty(&string_from_row(existing, "goal"), incoming.goal.clone()),
        rules: choose_later_non_empty(&string_from_row(existing, "rules"), incoming.rules.clone()),
        message_prompt: choose_later_non_empty(
            &string_from_row(existing, "message_prompt"),
            incoming.message_prompt.clone(),
        ),
        doc_url: choose_later_non_empty(
            &string_from_row(existing, "doc_url"),
            incoming.doc_url.clone(),
        ),
        group_owner_did: choose_later_non_empty(
            &string_from_row(existing, "group_owner_did"),
            incoming.group_owner_did.clone(),
        ),
        group_owner_handle: choose_later_non_empty(
            &string_from_row(existing, "group_owner_handle"),
            incoming.group_owner_handle.clone(),
        ),
        my_role: choose_later_non_empty(
            &string_from_row(existing, "my_role"),
            incoming.my_role.clone(),
        ),
        membership_status: choose_later_non_empty(
            &string_from_row(existing, "membership_status"),
            incoming.membership_status.clone(),
        ),
        join_enabled: incoming
            .join_enabled
            .or_else(|| bool_ptr_from_row(existing, "join_enabled")),
        join_code: choose_later_non_empty(
            &string_from_row(existing, "join_code"),
            incoming.join_code.clone(),
        ),
        join_code_expires_at: later_time_string(
            &string_from_row(existing, "join_code_expires_at"),
            incoming.join_code_expires_at.clone(),
        ),
        member_count: max_int64_ptr(
            int64_ptr_from_row(existing, "member_count"),
            incoming.member_count,
        ),
        last_synced_seq: max_int64_ptr(
            int64_ptr_from_row(existing, "last_synced_seq"),
            incoming.last_synced_seq,
        ),
        last_read_seq: max_int64_ptr(
            int64_ptr_from_row(existing, "last_read_seq"),
            incoming.last_read_seq,
        ),
        last_message_at: later_time_string(
            &string_from_row(existing, "last_message_at"),
            incoming.last_message_at.clone(),
        ),
        remote_created_at: earlier_time_string(
            &string_from_row(existing, "remote_created_at"),
            incoming.remote_created_at.clone(),
        ),
        remote_updated_at: later_time_string(
            &string_from_row(existing, "remote_updated_at"),
            incoming.remote_updated_at.clone(),
        ),
        stored_at: later_time_string(
            &string_from_row(existing, "stored_at"),
            incoming.stored_at.clone(),
        ),
        metadata: choose_later_non_empty(
            &string_from_row(existing, "metadata"),
            incoming.metadata.clone(),
        ),
        owner_identity_id: choose_later_non_empty(
            &string_from_row(existing, "owner_identity_id"),
            incoming.owner_identity_id.clone(),
        ),
        credential_name: choose_later_non_empty(
            &string_from_row(existing, "credential_name"),
            incoming.credential_name.clone(),
        ),
        ..incoming
    }
}

pub(super) fn merge_recovered_group_member(
    existing: &RowMap,
    incoming: GroupMemberRecord,
) -> GroupMemberRecord {
    GroupMemberRecord {
        member_did: choose_later_non_empty(
            &string_from_row(existing, "member_did"),
            incoming.member_did.clone(),
        ),
        member_handle: choose_later_non_empty(
            &string_from_row(existing, "member_handle"),
            incoming.member_handle.clone(),
        ),
        profile_url: choose_later_non_empty(
            &string_from_row(existing, "profile_url"),
            incoming.profile_url.clone(),
        ),
        role: choose_later_non_empty(&string_from_row(existing, "role"), incoming.role.clone()),
        status: choose_later_non_empty(
            &string_from_row(existing, "status"),
            incoming.status.clone(),
        ),
        joined_at: earlier_time_string(
            &string_from_row(existing, "joined_at"),
            incoming.joined_at.clone(),
        ),
        sent_message_count: max_int64_ptr(
            int64_ptr_from_row(existing, "sent_message_count"),
            incoming.sent_message_count,
        ),
        last_synced_at: later_time_string(
            &string_from_row(existing, "last_synced_at"),
            incoming.last_synced_at.clone(),
        ),
        metadata: choose_later_non_empty(
            &string_from_row(existing, "metadata"),
            incoming.metadata.clone(),
        ),
        owner_identity_id: choose_later_non_empty(
            &string_from_row(existing, "owner_identity_id"),
            incoming.owner_identity_id.clone(),
        ),
        credential_name: choose_later_non_empty(
            &string_from_row(existing, "credential_name"),
            incoming.credential_name.clone(),
        ),
        ..incoming
    }
}

pub(super) fn normalize_recover_owner_dids<S: AsRef<str>>(
    old_owner_dids: &[S],
    new_owner_did: &str,
) -> Vec<String> {
    let mut normalized = Vec::new();
    let mut seen = BTreeSet::new();
    let new_owner_did = normalize_owner_did(new_owner_did);
    for owner in old_owner_dids {
        let owner = normalize_owner_did(owner.as_ref());
        if owner.is_empty() || owner == new_owner_did || !seen.insert(owner.clone()) {
            continue;
        }
        normalized.push(owner);
    }
    normalized
}

fn remap_recovered_self_did(
    value: &str,
    old_owner_set: &BTreeSet<String>,
    new_owner_did: &str,
) -> String {
    let value = value.trim();
    if old_owner_set.contains(value) {
        new_owner_did.to_string()
    } else {
        value.to_string()
    }
}

fn first_recovered_peer_did(sender_did: &str, receiver_did: &str, owner_did: &str) -> String {
    if !sender_did.trim().is_empty() && sender_did != owner_did {
        return sender_did.to_string();
    }
    if !receiver_did.trim().is_empty() && receiver_did != owner_did {
        return receiver_did.to_string();
    }
    first_non_empty([sender_did, receiver_did])
        .unwrap_or_default()
        .to_string()
}

fn choose_later_non_empty(existing: &str, incoming: String) -> String {
    if !incoming.trim().is_empty() {
        incoming
    } else {
        existing.to_string()
    }
}

fn earlier_time_string(existing: &str, incoming: String) -> String {
    match compare_time_strings(existing, &incoming) {
        1 => incoming,
        -1 | 0 => default_string(existing.to_string(), &incoming),
        _ => default_string(incoming, existing),
    }
}

fn later_time_string(existing: &str, incoming: String) -> String {
    match compare_time_strings(existing, &incoming) {
        -1 => incoming,
        1 | 0 => default_string(existing.to_string(), &incoming),
        _ => default_string(incoming, existing),
    }
}

fn compare_time_strings(left: &str, right: &str) -> i8 {
    let left = left.trim();
    let right = right.trim();
    match (left.is_empty(), right.is_empty()) {
        (true, true) => return 0,
        (true, false) => return -1,
        (false, true) => return 1,
        (false, false) => {}
    }
    if let (Ok(left_time), Ok(right_time)) = (
        OffsetDateTime::parse(left, &Rfc3339),
        OffsetDateTime::parse(right, &Rfc3339),
    ) {
        return match left_time.cmp(&right_time) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        };
    }
    match left.cmp(right) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

fn max_int64_ptr(left: Option<i64>, right: Option<i64>) -> Option<i64> {
    match (left, right) {
        (None, right) => right,
        (left, None) => left,
        (Some(left), Some(right)) if left >= right => Some(left),
        (Some(_), Some(right)) => Some(right),
    }
}

fn bool_from_ptr(value: Option<bool>) -> bool {
    value.unwrap_or(false)
}

fn first_non_empty<'a, I>(values: I) -> Option<&'a str>
where
    I: IntoIterator<Item = &'a str>,
{
    values.into_iter().find(|value| !value.trim().is_empty())
}
