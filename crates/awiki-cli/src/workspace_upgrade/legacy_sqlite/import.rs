use super::types::{ImportReport, LegacyScan};
use super::{
    current_schema_version, ensure_schema,
    helpers::{
        bool_to_int, default_bool_value, default_int64_ptr, default_string, generate_id,
        make_thread_id, normalize_credential_name, normalize_metadata, normalize_optional_bool,
        normalize_optional_float64, normalize_optional_int64, normalize_optional_string,
        normalize_owner_did, now_utc,
    },
    open_read_only,
    query::query_rows,
};
use super::{StoreError, StoreResult};
use crate::workspace_config::Paths;
use rusqlite::{params, Connection, Transaction};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const IMPORT_TABLES: &[(&str, Importer)] = &[
    ("messages", import_messages),
    ("e2ee_outbox", import_e2ee_outbox),
    ("contacts", import_contacts),
    ("groups", import_groups),
    ("group_members", import_group_members),
    ("relationship_events", import_relationship_events),
];

type Importer = fn(&Connection, &mut Connection, &LegacyOwnerLookup) -> StoreResult<usize>;

#[derive(Debug, Clone, Default)]
pub struct LegacyOwnerLookup {
    owner_by_credential: BTreeMap<String, LegacyOwnerScope>,
    owner_by_did: BTreeMap<String, LegacyOwnerScope>,
    default_owner: Option<LegacyOwnerScope>,
}

impl LegacyOwnerLookup {
    pub fn from_entries<I>(entries: I) -> Self
    where
        I: IntoIterator<Item = (String, String, bool)>,
    {
        Self::from_identity_entries(
            entries
                .into_iter()
                .map(|(name, did, is_default)| (name.clone(), name, did, is_default)),
        )
    }

    pub fn from_identity_entries<I>(entries: I) -> Self
    where
        I: IntoIterator<Item = (String, String, String, bool)>,
    {
        let entries = entries.into_iter().collect::<Vec<_>>();
        let mut lookup = Self::default();
        for (identity_id, name, did, is_default) in &entries {
            let Some(scope) = LegacyOwnerScope::new(identity_id, did, Some(name)) else {
                continue;
            };
            lookup
                .owner_by_credential
                .insert(name.trim().to_string(), scope.clone());
            lookup
                .owner_by_did
                .insert(scope.owner_did.clone(), scope.clone());
            if *is_default {
                lookup.default_owner = Some(scope.clone());
            }
        }
        if lookup.default_owner.is_none() && entries.len() == 1 {
            let (identity_id, name, did, _) = &entries[0];
            lookup.default_owner = LegacyOwnerScope::new(identity_id, did, Some(name));
        }
        lookup
    }

    fn infer_owner_scope(&self, row: &Value) -> StoreResult<LegacyOwnerScope> {
        let owner = string_from_row(row, "owner_did");
        if !owner.trim().is_empty() {
            if let Some(scope) = self.owner_by_did.get(owner.trim()) {
                return Ok(scope.clone());
            }
            return Err(StoreError::Invalid(
                "legacy row owner_did could not be resolved to owner_identity_id".to_string(),
            ));
        }
        let credential = string_from_row(row, "credential_name");
        if !credential.is_empty() {
            if let Some(scope) = self.owner_by_credential.get(credential.trim()) {
                return Ok(scope.clone());
            }
            return Err(StoreError::Invalid(
                "legacy row credential_name could not be resolved to owner_identity_id".to_string(),
            ));
        }
        self.default_owner.clone().ok_or_else(|| {
            StoreError::Invalid("legacy row owner_identity_id could not be resolved".to_string())
        })
    }

    fn has_default_owner(&self) -> bool {
        self.default_owner.is_some()
    }
}

#[derive(Debug, Clone)]
struct LegacyOwnerScope {
    owner_identity_id: String,
    owner_did: String,
    credential_name: String,
}

impl LegacyOwnerScope {
    fn new(identity_id: &str, did: &str, credential_name: Option<&str>) -> Option<Self> {
        let owner_identity_id = identity_id.trim();
        let owner_did = did.trim();
        if owner_identity_id.is_empty() || owner_did.is_empty() {
            return None;
        }
        Some(Self {
            owner_identity_id: owner_identity_id.to_string(),
            owner_did: owner_did.to_string(),
            credential_name: credential_name.unwrap_or_default().trim().to_string(),
        })
    }

    fn credential_name_for_row(&self, row_credential_name: &str) -> String {
        let row_credential_name = normalize_credential_name(row_credential_name);
        if row_credential_name.is_empty() {
            self.credential_name.clone()
        } else {
            row_credential_name
        }
    }
}

pub fn scan_legacy_database(paths: &Paths) -> StoreResult<LegacyScan> {
    let legacy_path = legacy_database_path(&paths.legacy_data_dir);
    let mut scan = LegacyScan {
        path: legacy_path.to_string_lossy().into_owned(),
        exists: false,
        schema_version: 0,
        tables: Vec::new(),
    };
    if !legacy_path.exists() {
        return Ok(scan);
    }
    let connection = match open_read_only(&scan.path) {
        Ok(connection) => connection,
        Err(err) if is_missing_database_error(&err) => return Ok(scan),
        Err(err) => return Err(err),
    };
    scan.exists = true;
    scan.schema_version = current_schema_version(&connection)?;
    let rows = query_rows(
        &connection,
        "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name",
    )?;
    scan.tables = rows
        .iter()
        .filter_map(|row| row.get("name").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect();
    Ok(scan)
}

pub fn import_legacy_database(
    target: &mut Connection,
    paths: &Paths,
    owners: &LegacyOwnerLookup,
) -> StoreResult<ImportReport> {
    let scan = scan_legacy_database(paths)?;
    if !scan.exists {
        return Err(StoreError::LegacyDatabaseNotFound);
    }
    let source = open_read_only(&scan.path)?;
    ensure_schema(target)?;
    if scan.schema_version > 0 && scan.schema_version < 6 && !owners.has_default_owner() {
        return Err(StoreError::UnsupportedLegacySchema(
            "unsupported legacy sqlite schema version: legacy schema < 6 requires at least one imported identity so owner_did can be inferred".to_string(),
        ));
    }
    let mut report = ImportReport {
        source_path: scan.path.clone(),
        source_schema_version: scan.schema_version,
        imported_rows: BTreeMap::new(),
        skipped_tables: Vec::new(),
        warnings: Vec::new(),
    };
    for (table, importer) in IMPORT_TABLES {
        match importer(&source, target, owners) {
            Ok(count) => {
                report.imported_rows.insert((*table).to_string(), count);
            }
            Err(err) if is_missing_table_error(&err) => {
                report.skipped_tables.push((*table).to_string());
            }
            Err(err) => {
                return Err(StoreError::Invalid(format!("import {table}: {err}")));
            }
        }
    }
    report.skipped_tables.sort();
    Ok(report)
}

fn legacy_database_path(raw: &str) -> PathBuf {
    let path = Path::new(raw.trim());
    if raw.trim().to_ascii_lowercase().ends_with(".db") {
        return path.to_path_buf();
    }
    Path::new(raw).join("database").join("awiki.db")
}

fn import_messages(
    source: &Connection,
    target: &mut Connection,
    owners: &LegacyOwnerLookup,
) -> StoreResult<usize> {
    let rows = query_rows(source, "SELECT * FROM messages")?;
    let mut count = 0;
    for row in rows {
        let msg_id = string_from_row(&row, "msg_id");
        if msg_id.is_empty() {
            continue;
        }
        let owner = owners.infer_owner_scope(&row)?;
        let mut thread_id = string_from_row(&row, "thread_id");
        if thread_id.is_empty() {
            thread_id = make_thread_id(
                &owner.owner_did,
                &string_from_row(&row, "sender_did"),
                &string_from_row(&row, "group_id"),
            );
        }
        let credential_name =
            owner.credential_name_for_row(&string_from_row(&row, "credential_name"));
        store_message(
            target,
            MessageImport {
                msg_id,
                owner,
                thread_id,
                direction: int_from_row(&row, "direction"),
                sender_did: string_from_row(&row, "sender_did"),
                receiver_did: string_from_row(&row, "receiver_did"),
                group_id: string_from_row(&row, "group_id"),
                group_did: string_from_row(&row, "group_did"),
                content_type: default_string(string_from_row(&row, "content_type"), "text"),
                content: string_from_row(&row, "content"),
                title: string_from_row(&row, "title"),
                server_seq: int64_ptr_from_row(&row, "server_seq"),
                sent_at: string_from_row(&row, "sent_at"),
                stored_at: string_from_row(&row, "stored_at"),
                is_e2ee: bool_from_row(&row, "is_e2ee"),
                is_read: bool_from_row(&row, "is_read"),
                sender_name: string_from_row(&row, "sender_name"),
                metadata: metadata_from_row(&row, "metadata"),
                credential_name,
            },
        )?;
        count += 1;
    }
    Ok(count)
}

fn import_e2ee_outbox(
    source: &Connection,
    target: &mut Connection,
    owners: &LegacyOwnerLookup,
) -> StoreResult<usize> {
    let rows = query_rows(source, "SELECT * FROM e2ee_outbox")?;
    let mut count = 0;
    for row in rows {
        let owner = owners.infer_owner_scope(&row)?;
        let credential_name =
            owner.credential_name_for_row(&string_from_row(&row, "credential_name"));
        queue_e2ee_outbox(
            target,
            E2EEOutboxImport {
                outbox_id: string_from_row(&row, "outbox_id"),
                owner,
                peer_did: string_from_row(&row, "peer_did"),
                session_id: string_from_row(&row, "session_id"),
                original_type: default_string(string_from_row(&row, "original_type"), "text"),
                plaintext: string_from_row(&row, "plaintext"),
                local_status: default_string(string_from_row(&row, "local_status"), "queued"),
                attempt_count: int_from_row(&row, "attempt_count"),
                sent_msg_id: string_from_row(&row, "sent_msg_id"),
                sent_server_seq: int64_ptr_from_row(&row, "sent_server_seq"),
                last_error_code: string_from_row(&row, "last_error_code"),
                retry_hint: string_from_row(&row, "retry_hint"),
                failed_msg_id: string_from_row(&row, "failed_msg_id"),
                failed_server_seq: int64_ptr_from_row(&row, "failed_server_seq"),
                metadata: metadata_from_row(&row, "metadata"),
                last_attempt_at: string_from_row(&row, "last_attempt_at"),
                created_at: string_from_row(&row, "created_at"),
                updated_at: string_from_row(&row, "updated_at"),
                credential_name,
            },
        )?;
        count += 1;
    }
    Ok(count)
}

fn import_contacts(
    source: &Connection,
    target: &mut Connection,
    owners: &LegacyOwnerLookup,
) -> StoreResult<usize> {
    let rows = query_rows(source, "SELECT * FROM contacts")?;
    let mut count = 0;
    for row in rows {
        let did = string_from_row(&row, "did");
        if did.is_empty() {
            continue;
        }
        let owner = owners.infer_owner_scope(&row)?;
        let credential_name =
            owner.credential_name_for_row(&string_from_row(&row, "credential_name"));
        upsert_contact(
            target,
            ContactImport {
                owner,
                did,
                name: string_from_row(&row, "name"),
                handle: string_from_row(&row, "handle"),
                nick_name: string_from_row(&row, "nick_name"),
                bio: string_from_row(&row, "bio"),
                profile_md: string_from_row(&row, "profile_md"),
                tags: string_from_row(&row, "tags"),
                relationship: string_from_row(&row, "relationship"),
                source_type: string_from_row(&row, "source_type"),
                source_name: string_from_row(&row, "source_name"),
                source_group_id: string_from_row(&row, "source_group_id"),
                connected_at: string_from_row(&row, "connected_at"),
                recommended_reason: string_from_row(&row, "recommended_reason"),
                followed: bool_ptr_from_row(&row, "followed"),
                messaged: bool_ptr_from_row(&row, "messaged"),
                note: string_from_row(&row, "note"),
                first_seen_at: string_from_row(&row, "first_seen_at"),
                last_seen_at: string_from_row(&row, "last_seen_at"),
                metadata: metadata_from_row(&row, "metadata"),
                credential_name,
            },
        )?;
        count += 1;
    }
    Ok(count)
}

fn import_groups(
    source: &Connection,
    target: &mut Connection,
    owners: &LegacyOwnerLookup,
) -> StoreResult<usize> {
    let rows = query_rows(source, "SELECT * FROM groups")?;
    let mut count = 0;
    for row in rows {
        let group_id = string_from_row(&row, "group_id");
        if group_id.is_empty() {
            continue;
        }
        let owner = owners.infer_owner_scope(&row)?;
        let credential_name =
            owner.credential_name_for_row(&string_from_row(&row, "credential_name"));
        upsert_group(
            target,
            GroupImport {
                owner,
                group_id,
                group_did: string_from_row(&row, "group_did"),
                name: string_from_row(&row, "name"),
                group_mode: default_string(string_from_row(&row, "group_mode"), "general"),
                slug: string_from_row(&row, "slug"),
                description: string_from_row(&row, "description"),
                goal: string_from_row(&row, "goal"),
                rules: string_from_row(&row, "rules"),
                message_prompt: string_from_row(&row, "message_prompt"),
                doc_url: string_from_row(&row, "doc_url"),
                group_owner_did: string_from_row(&row, "group_owner_did"),
                group_owner_handle: string_from_row(&row, "group_owner_handle"),
                my_role: string_from_row(&row, "my_role"),
                membership_status: default_string(
                    string_from_row(&row, "membership_status"),
                    "active",
                ),
                join_enabled: bool_ptr_from_row(&row, "join_enabled"),
                join_code: string_from_row(&row, "join_code"),
                join_code_expires_at: string_from_row(&row, "join_code_expires_at"),
                member_count: int64_ptr_from_row(&row, "member_count"),
                last_synced_seq: int64_ptr_from_row(&row, "last_synced_seq"),
                last_read_seq: int64_ptr_from_row(&row, "last_read_seq"),
                last_message_at: string_from_row(&row, "last_message_at"),
                remote_created_at: string_from_row(&row, "remote_created_at"),
                remote_updated_at: string_from_row(&row, "remote_updated_at"),
                stored_at: string_from_row(&row, "stored_at"),
                metadata: metadata_from_row(&row, "metadata"),
                credential_name,
            },
        )?;
        count += 1;
    }
    Ok(count)
}

fn import_group_members(
    source: &Connection,
    target: &mut Connection,
    owners: &LegacyOwnerLookup,
) -> StoreResult<usize> {
    let rows = query_rows(source, "SELECT * FROM group_members")?;
    let mut count = 0;
    for row in rows {
        let group_id = string_from_row(&row, "group_id");
        let user_id = string_from_row(&row, "user_id");
        if group_id.is_empty() || user_id.is_empty() {
            continue;
        }
        let owner = owners.infer_owner_scope(&row)?;
        let credential_name =
            owner.credential_name_for_row(&string_from_row(&row, "credential_name"));
        upsert_group_member(
            target,
            GroupMemberImport {
                owner,
                group_id,
                user_id,
                member_did: string_from_row(&row, "member_did"),
                member_handle: string_from_row(&row, "member_handle"),
                profile_url: string_from_row(&row, "profile_url"),
                role: string_from_row(&row, "role"),
                status: default_string(string_from_row(&row, "status"), "active"),
                joined_at: string_from_row(&row, "joined_at"),
                sent_message_count: int64_ptr_from_row(&row, "sent_message_count"),
                last_synced_at: string_from_row(&row, "last_synced_at"),
                metadata: metadata_from_row(&row, "metadata"),
                credential_name,
            },
        )?;
        count += 1;
    }
    Ok(count)
}

fn import_relationship_events(
    source: &Connection,
    target: &mut Connection,
    owners: &LegacyOwnerLookup,
) -> StoreResult<usize> {
    let rows = query_rows(source, "SELECT * FROM relationship_events")?;
    let mut count = 0;
    for row in rows {
        let target_did = string_from_row(&row, "target_did");
        let event_type = string_from_row(&row, "event_type");
        if target_did.is_empty() || event_type.is_empty() {
            continue;
        }
        let owner = owners.infer_owner_scope(&row)?;
        let credential_name =
            owner.credential_name_for_row(&string_from_row(&row, "credential_name"));
        append_relationship_event(
            target,
            RelationshipEventImport {
                event_id: string_from_row(&row, "event_id"),
                owner,
                target_did,
                target_handle: string_from_row(&row, "target_handle"),
                event_type,
                source_type: string_from_row(&row, "source_type"),
                source_name: string_from_row(&row, "source_name"),
                source_group_id: string_from_row(&row, "source_group_id"),
                reason: string_from_row(&row, "reason"),
                score: float64_ptr_from_row(&row, "score"),
                status: default_string(string_from_row(&row, "status"), "pending"),
                created_at: string_from_row(&row, "created_at"),
                updated_at: string_from_row(&row, "updated_at"),
                metadata: metadata_from_row(&row, "metadata"),
                credential_name,
            },
        )?;
        count += 1;
    }
    Ok(count)
}

#[derive(Debug)]
struct MessageImport {
    msg_id: String,
    owner: LegacyOwnerScope,
    thread_id: String,
    direction: i64,
    sender_did: String,
    receiver_did: String,
    group_id: String,
    group_did: String,
    content_type: String,
    content: String,
    title: String,
    server_seq: Option<i64>,
    sent_at: String,
    stored_at: String,
    is_e2ee: bool,
    is_read: bool,
    sender_name: String,
    metadata: String,
    credential_name: String,
}

fn store_message(target: &Connection, record: MessageImport) -> StoreResult<()> {
    if record.msg_id.trim().is_empty() {
        return Err(StoreError::Invalid("msg_id is required".to_string()));
    }
    if record.thread_id.trim().is_empty() {
        return Err(StoreError::Invalid("thread_id is required".to_string()));
    }
    let now = now_utc();
    let owner_identity_id = record.owner.owner_identity_id;
    let owner_did = record.owner.owner_did;
    let conversation_id = conversation_id_from_legacy_thread(
        &record.thread_id,
        &owner_did,
        &record.sender_did,
        &record.group_id,
    );
    target.execute(
        r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction, sender_did, receiver_did, group_id, group_did,
     content_type, content, title, server_seq, sent_at, stored_at, is_e2ee, is_read,
     sender_name, metadata, credential_name)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)
ON CONFLICT(owner_identity_id, msg_id)
DO UPDATE SET
    owner_did = excluded.owner_did,
    conversation_id = excluded.conversation_id,
    thread_id = excluded.thread_id,
    direction = excluded.direction,
    sender_did = excluded.sender_did,
    receiver_did = excluded.receiver_did,
    group_id = excluded.group_id,
    group_did = excluded.group_did,
    content_type = CASE
        WHEN excluded.content_type IN ('application/anp-direct-init+json', 'application/anp-direct-cipher+json')
             AND messages.content_type NOT IN ('application/anp-direct-init+json', 'application/anp-direct-cipher+json')
        THEN messages.content_type
        ELSE excluded.content_type
    END,
    content = CASE
        WHEN excluded.content_type IN ('application/anp-direct-init+json', 'application/anp-direct-cipher+json')
             AND messages.content_type NOT IN ('application/anp-direct-init+json', 'application/anp-direct-cipher+json')
        THEN messages.content
        ELSE excluded.content
    END,
    title = excluded.title,
    server_seq = COALESCE(excluded.server_seq, messages.server_seq),
    sent_at = COALESCE(excluded.sent_at, messages.sent_at),
    is_e2ee = CASE WHEN excluded.is_e2ee = 1 OR messages.is_e2ee = 1 THEN 1 ELSE 0 END,
    is_read = CASE WHEN excluded.is_read = 1 OR messages.is_read = 1 THEN 1 ELSE 0 END,
    sender_name = COALESCE(excluded.sender_name, messages.sender_name),
    metadata = CASE
        WHEN excluded.content_type IN ('application/anp-direct-init+json', 'application/anp-direct-cipher+json')
             AND messages.content_type NOT IN ('application/anp-direct-init+json', 'application/anp-direct-cipher+json')
        THEN messages.metadata
        ELSE COALESCE(excluded.metadata, messages.metadata)
    END,
    credential_name = COALESCE(excluded.credential_name, messages.credential_name)"#,
        params![
            record.msg_id,
            owner_identity_id,
            owner_did,
            conversation_id,
            record.thread_id,
            record.direction,
            normalize_optional_string(&record.sender_did),
            normalize_optional_string(&record.receiver_did),
            normalize_optional_string(&record.group_id),
            normalize_optional_string(&record.group_did),
            default_string(record.content_type, "text"),
            record.content,
            normalize_optional_string(&record.title),
            normalize_optional_int64(record.server_seq),
            normalize_optional_string(&record.sent_at),
            default_string(record.stored_at, &now),
            bool_to_int(record.is_e2ee),
            bool_to_int(record.is_read),
            normalize_optional_string(&record.sender_name),
            normalize_metadata(&record.metadata),
            normalize_credential_name(&record.credential_name),
        ],
    )?;
    Ok(())
}

fn conversation_id_from_legacy_thread(
    thread_id: &str,
    owner_did: &str,
    sender_did: &str,
    group_id: &str,
) -> String {
    let group_id = group_id.trim();
    if !group_id.is_empty() {
        return format!("group:{group_id}");
    }

    let thread_id = thread_id.trim();
    let owner_did = owner_did.trim();
    if let Some(rest) = thread_id.strip_prefix("group:") {
        let group = rest.trim();
        if !group.is_empty() {
            return format!("group:{group}");
        }
    }
    if let Some(rest) = thread_id.strip_prefix("dm:") {
        let parts = split_legacy_dm_thread(rest);
        if let Some(peer) = parts
            .iter()
            .find(|part| !part.trim().is_empty() && part.trim() != owner_did)
        {
            return format!("dm:{}", peer.trim());
        }
    }

    let sender_did = sender_did.trim();
    if !sender_did.is_empty() && sender_did != owner_did {
        return format!("dm:{sender_did}");
    }

    let fallback = thread_id.trim_start_matches("dm:").trim();
    if !fallback.is_empty() && fallback != owner_did {
        return format!("dm:{fallback}");
    }
    "dm:unknown".to_string()
}

fn split_legacy_dm_thread(rest: &str) -> Vec<String> {
    let markers = rest
        .match_indices("did:")
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if markers.len() >= 2 {
        return markers
            .iter()
            .enumerate()
            .map(|(position, start)| {
                let end = markers.get(position + 1).copied().unwrap_or(rest.len());
                rest[*start..end].trim_matches(':').to_string()
            })
            .filter(|part| !part.trim().is_empty())
            .collect();
    }
    rest.split(':')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

#[derive(Debug)]
struct E2EEOutboxImport {
    outbox_id: String,
    owner: LegacyOwnerScope,
    peer_did: String,
    session_id: String,
    original_type: String,
    plaintext: String,
    local_status: String,
    attempt_count: i64,
    sent_msg_id: String,
    sent_server_seq: Option<i64>,
    last_error_code: String,
    retry_hint: String,
    failed_msg_id: String,
    failed_server_seq: Option<i64>,
    metadata: String,
    last_attempt_at: String,
    created_at: String,
    updated_at: String,
    credential_name: String,
}

fn queue_e2ee_outbox(target: &Connection, record: E2EEOutboxImport) -> StoreResult<String> {
    let outbox_id = default_string(record.outbox_id, &generate_id());
    let now = now_utc();
    let owner_identity_id = record.owner.owner_identity_id;
    let owner_did = record.owner.owner_did;
    target.execute(
        r#"
INSERT INTO e2ee_outbox
    (outbox_id, owner_identity_id, owner_did, peer_did, session_id, original_type, plaintext, local_status,
     attempt_count, sent_msg_id, sent_server_seq, last_error_code, retry_hint, failed_msg_id,
     failed_server_seq, metadata, last_attempt_at, created_at, updated_at, credential_name)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)
ON CONFLICT(owner_identity_id, outbox_id)
DO UPDATE SET
    owner_did = excluded.owner_did,
    peer_did = excluded.peer_did,
    session_id = COALESCE(excluded.session_id, e2ee_outbox.session_id),
    original_type = excluded.original_type,
    plaintext = excluded.plaintext,
    local_status = excluded.local_status,
    attempt_count = excluded.attempt_count,
    sent_msg_id = COALESCE(excluded.sent_msg_id, e2ee_outbox.sent_msg_id),
    sent_server_seq = COALESCE(excluded.sent_server_seq, e2ee_outbox.sent_server_seq),
    last_error_code = COALESCE(excluded.last_error_code, e2ee_outbox.last_error_code),
    retry_hint = COALESCE(excluded.retry_hint, e2ee_outbox.retry_hint),
    failed_msg_id = COALESCE(excluded.failed_msg_id, e2ee_outbox.failed_msg_id),
    failed_server_seq = COALESCE(excluded.failed_server_seq, e2ee_outbox.failed_server_seq),
    metadata = COALESCE(excluded.metadata, e2ee_outbox.metadata),
    last_attempt_at = COALESCE(excluded.last_attempt_at, e2ee_outbox.last_attempt_at),
    updated_at = excluded.updated_at,
    credential_name = COALESCE(excluded.credential_name, e2ee_outbox.credential_name)"#,
        params![
            outbox_id,
            owner_identity_id,
            owner_did,
            record.peer_did,
            normalize_optional_string(&record.session_id),
            default_string(record.original_type, "text"),
            record.plaintext,
            default_string(record.local_status, "queued"),
            record.attempt_count,
            normalize_optional_string(&record.sent_msg_id),
            normalize_optional_int64(record.sent_server_seq),
            normalize_optional_string(&record.last_error_code),
            normalize_optional_string(&record.retry_hint),
            normalize_optional_string(&record.failed_msg_id),
            normalize_optional_int64(record.failed_server_seq),
            normalize_metadata(&record.metadata),
            normalize_optional_string(&record.last_attempt_at),
            default_string(record.created_at, &now),
            default_string(record.updated_at, &now),
            normalize_credential_name(&record.credential_name),
        ],
    )?;
    Ok(outbox_id)
}

#[derive(Debug)]
struct ContactImport {
    owner: LegacyOwnerScope,
    did: String,
    name: String,
    handle: String,
    nick_name: String,
    bio: String,
    profile_md: String,
    tags: String,
    relationship: String,
    source_type: String,
    source_name: String,
    source_group_id: String,
    connected_at: String,
    recommended_reason: String,
    followed: Option<bool>,
    messaged: Option<bool>,
    note: String,
    first_seen_at: String,
    last_seen_at: String,
    metadata: String,
    credential_name: String,
}

fn upsert_contact(target: &mut Connection, record: ContactImport) -> StoreResult<()> {
    if record.did.trim().is_empty() {
        return Err(StoreError::Invalid("contact did is required".to_string()));
    }
    let owner_identity_id = record.owner.owner_identity_id.clone();
    let owner_did = record.owner.owner_did.clone();
    let handle = record.handle.trim().to_string();
    let tx = target.transaction()?;
    let now = now_utc();
    let existing_by_did = query_contact_did_handle(&tx, &owner_identity_id, &record.did)?;
    let existing_by_handle = if handle.is_empty() {
        Vec::new()
    } else {
        query_contacts_by_handle(&tx, &owner_identity_id, &handle)?
    };
    if !handle.is_empty() && !existing_by_handle.is_empty() && existing_by_handle[0].0 != record.did
    {
        tx.execute(
            "UPDATE contacts SET handle = NULL, last_seen_at = ?1 WHERE owner_identity_id = ?2 AND did = ?3",
            params![now, owner_identity_id, existing_by_handle[0].0],
        )?;
    }
    if !existing_by_did.is_empty() {
        tx.execute(
            r#"
UPDATE contacts
SET name = COALESCE(?1, name),
    handle = COALESCE(?2, handle),
    nick_name = COALESCE(?3, nick_name),
    bio = COALESCE(?4, bio),
    profile_md = COALESCE(?5, profile_md),
    tags = COALESCE(?6, tags),
    relationship = COALESCE(?7, relationship),
    source_type = COALESCE(?8, source_type),
    source_name = COALESCE(?9, source_name),
    source_group_id = COALESCE(?10, source_group_id),
    connected_at = COALESCE(?11, connected_at),
    recommended_reason = COALESCE(?12, recommended_reason),
    followed = COALESCE(?13, followed),
    messaged = COALESCE(?14, messaged),
    note = COALESCE(?15, note),
    first_seen_at = COALESCE(?16, first_seen_at),
    last_seen_at = ?17,
    metadata = COALESCE(?18, metadata),
    owner_identity_id = COALESCE(?19, owner_identity_id),
    credential_name = COALESCE(?20, credential_name)
WHERE owner_identity_id = ?21 AND did = ?22"#,
            params![
                normalize_optional_string(&record.name),
                normalize_optional_string(&handle),
                normalize_optional_string(&record.nick_name),
                normalize_optional_string(&record.bio),
                normalize_optional_string(&record.profile_md),
                normalize_optional_string(&record.tags),
                normalize_optional_string(&record.relationship),
                normalize_optional_string(&record.source_type),
                normalize_optional_string(&record.source_name),
                normalize_optional_string(&record.source_group_id),
                normalize_optional_string(&record.connected_at),
                normalize_optional_string(&record.recommended_reason),
                normalize_optional_bool(record.followed),
                normalize_optional_bool(record.messaged),
                normalize_optional_string(&record.note),
                normalize_optional_string(&record.first_seen_at),
                now,
                normalize_metadata(&record.metadata),
                owner_identity_id.clone(),
                normalize_credential_name(&record.credential_name),
                owner_identity_id,
                record.did,
            ],
        )?;
    } else {
        tx.execute(
            r#"
INSERT INTO contacts
    (owner_identity_id, owner_did, did, name, handle, nick_name, bio, profile_md, tags, relationship, source_type, source_name,
     source_group_id, connected_at, recommended_reason, followed, messaged, note, first_seen_at, last_seen_at, metadata, credential_name)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)"#,
            params![
                owner_identity_id.clone(),
                owner_did,
                record.did,
                normalize_optional_string(&record.name),
                normalize_optional_string(&record.handle),
                normalize_optional_string(&record.nick_name),
                normalize_optional_string(&record.bio),
                normalize_optional_string(&record.profile_md),
                normalize_optional_string(&record.tags),
                normalize_optional_string(&record.relationship),
                normalize_optional_string(&record.source_type),
                normalize_optional_string(&record.source_name),
                normalize_optional_string(&record.source_group_id),
                normalize_optional_string(&record.connected_at),
                normalize_optional_string(&record.recommended_reason),
                default_bool_value(record.followed),
                default_bool_value(record.messaged),
                normalize_optional_string(&record.note),
                default_string(record.first_seen_at.clone(), &now),
                default_string(record.last_seen_at.clone(), &now),
                normalize_metadata(&record.metadata),
                normalize_credential_name(&record.credential_name),
            ],
        )?;
    }
    if !handle.is_empty() {
        upsert_contact_handle_binding(
            &tx,
            ContactHandleBindingImport {
                owner_did,
                owner_identity_id,
                handle,
                did: record.did,
                is_current: true,
                first_seen_at: default_string(record.first_seen_at, &now),
                last_seen_at: default_string(record.last_seen_at, &now),
                source_type: record.source_type,
                source_group_id: record.source_group_id,
                metadata: record.metadata,
                credential_name: record.credential_name,
            },
        )?;
    }
    tx.commit()?;
    Ok(())
}

fn query_contact_did_handle(
    tx: &Transaction<'_>,
    owner_identity_id: &str,
    did: &str,
) -> StoreResult<Vec<(String, String)>> {
    let mut statement =
        tx.prepare("SELECT did, handle FROM contacts WHERE owner_identity_id = ?1 AND did = ?2")?;
    let rows = statement.query_map(params![owner_identity_id, did], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?.unwrap_or_default(),
        ))
    })?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::from)
}

fn query_contacts_by_handle(
    tx: &Transaction<'_>,
    owner_identity_id: &str,
    handle: &str,
) -> StoreResult<Vec<(String, String)>> {
    let mut statement = tx
        .prepare("SELECT did, handle FROM contacts WHERE owner_identity_id = ?1 AND handle = ?2")?;
    let rows = statement.query_map(params![owner_identity_id, handle], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?.unwrap_or_default(),
        ))
    })?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::from)
}

#[derive(Debug)]
struct ContactHandleBindingImport {
    owner_did: String,
    owner_identity_id: String,
    handle: String,
    did: String,
    is_current: bool,
    first_seen_at: String,
    last_seen_at: String,
    source_type: String,
    source_group_id: String,
    metadata: String,
    credential_name: String,
}

fn upsert_contact_handle_binding(
    tx: &Transaction<'_>,
    record: ContactHandleBindingImport,
) -> StoreResult<()> {
    let handle = record.handle.trim().to_string();
    let did = record.did.trim().to_string();
    if handle.is_empty() || did.is_empty() {
        return Ok(());
    }
    let owner_did = normalize_owner_did(&record.owner_did);
    let owner_identity_id = record.owner_identity_id;
    let first_seen_at = default_string(record.first_seen_at, &now_utc());
    let last_seen_at = default_string(record.last_seen_at, &first_seen_at);
    if record.is_current {
        tx.execute(
            r#"
UPDATE contact_handle_bindings
SET is_current = 0,
    last_seen_at = CASE
        WHEN last_seen_at IS NULL OR last_seen_at < ?1 THEN ?1
        ELSE last_seen_at
    END
WHERE owner_identity_id = ?2 AND handle = ?3 AND did <> ?4"#,
            params![last_seen_at, owner_identity_id, handle, did],
        )?;
    }
    tx.execute(
        r#"
INSERT INTO contact_handle_bindings
    (owner_identity_id, owner_did, handle, did, is_current, first_seen_at, last_seen_at, source_type, source_group_id, metadata, credential_name)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
ON CONFLICT(owner_identity_id, handle, did)
DO UPDATE SET
    owner_did = excluded.owner_did,
    is_current = excluded.is_current,
    first_seen_at = COALESCE(contact_handle_bindings.first_seen_at, excluded.first_seen_at),
    last_seen_at = excluded.last_seen_at,
    source_type = COALESCE(excluded.source_type, contact_handle_bindings.source_type),
    source_group_id = COALESCE(excluded.source_group_id, contact_handle_bindings.source_group_id),
    metadata = COALESCE(excluded.metadata, contact_handle_bindings.metadata),
    credential_name = COALESCE(excluded.credential_name, contact_handle_bindings.credential_name)"#,
        params![
            owner_identity_id,
            owner_did,
            handle,
            did,
            bool_to_int(record.is_current),
            first_seen_at,
            last_seen_at,
            normalize_optional_string(&record.source_type),
            normalize_optional_string(&record.source_group_id),
            normalize_metadata(&record.metadata),
            normalize_credential_name(&record.credential_name),
        ],
    )?;
    Ok(())
}

#[derive(Debug)]
struct GroupImport {
    owner: LegacyOwnerScope,
    group_id: String,
    group_did: String,
    name: String,
    group_mode: String,
    slug: String,
    description: String,
    goal: String,
    rules: String,
    message_prompt: String,
    doc_url: String,
    group_owner_did: String,
    group_owner_handle: String,
    my_role: String,
    membership_status: String,
    join_enabled: Option<bool>,
    join_code: String,
    join_code_expires_at: String,
    member_count: Option<i64>,
    last_synced_seq: Option<i64>,
    last_read_seq: Option<i64>,
    last_message_at: String,
    remote_created_at: String,
    remote_updated_at: String,
    stored_at: String,
    metadata: String,
    credential_name: String,
}

fn upsert_group(target: &Connection, record: GroupImport) -> StoreResult<()> {
    let owner_identity_id = record.owner.owner_identity_id;
    let owner_did = record.owner.owner_did;
    if owner_did.is_empty() || record.group_id.trim().is_empty() {
        return Err(StoreError::Invalid(
            "owner_did and group_id are required".to_string(),
        ));
    }
    let now = now_utc();
    target.execute(
        r#"
INSERT INTO groups
    (owner_identity_id, owner_did, group_id, group_did, name, group_mode, slug, description, goal, rules, message_prompt,
     doc_url, group_owner_did, group_owner_handle, my_role, membership_status, join_enabled, join_code,
     join_code_expires_at, member_count, last_synced_seq, last_read_seq, last_message_at, remote_created_at,
     remote_updated_at, stored_at, metadata, credential_name)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28)
ON CONFLICT(owner_identity_id, group_id)
DO UPDATE SET
    owner_did = excluded.owner_did,
    group_did = COALESCE(excluded.group_did, groups.group_did),
    name = COALESCE(excluded.name, groups.name),
    group_mode = excluded.group_mode,
    slug = COALESCE(excluded.slug, groups.slug),
    description = COALESCE(excluded.description, groups.description),
    goal = COALESCE(excluded.goal, groups.goal),
    rules = COALESCE(excluded.rules, groups.rules),
    message_prompt = COALESCE(excluded.message_prompt, groups.message_prompt),
    doc_url = COALESCE(excluded.doc_url, groups.doc_url),
    group_owner_did = COALESCE(excluded.group_owner_did, groups.group_owner_did),
    group_owner_handle = COALESCE(excluded.group_owner_handle, groups.group_owner_handle),
    my_role = COALESCE(excluded.my_role, groups.my_role),
    membership_status = excluded.membership_status,
    join_enabled = COALESCE(excluded.join_enabled, groups.join_enabled),
    join_code = COALESCE(excluded.join_code, groups.join_code),
    join_code_expires_at = COALESCE(excluded.join_code_expires_at, groups.join_code_expires_at),
    member_count = COALESCE(excluded.member_count, groups.member_count),
    last_synced_seq = COALESCE(excluded.last_synced_seq, groups.last_synced_seq),
    last_read_seq = COALESCE(excluded.last_read_seq, groups.last_read_seq),
    last_message_at = COALESCE(excluded.last_message_at, groups.last_message_at),
    remote_created_at = COALESCE(excluded.remote_created_at, groups.remote_created_at),
    remote_updated_at = COALESCE(excluded.remote_updated_at, groups.remote_updated_at),
    stored_at = excluded.stored_at,
    metadata = COALESCE(excluded.metadata, groups.metadata),
    credential_name = COALESCE(excluded.credential_name, groups.credential_name)"#,
        params![
            owner_identity_id,
            owner_did,
            record.group_id,
            normalize_optional_string(&record.group_did),
            normalize_optional_string(&record.name),
            default_string(record.group_mode, "general"),
            normalize_optional_string(&record.slug),
            normalize_optional_string(&record.description),
            normalize_optional_string(&record.goal),
            normalize_optional_string(&record.rules),
            normalize_optional_string(&record.message_prompt),
            normalize_optional_string(&record.doc_url),
            normalize_optional_string(&record.group_owner_did),
            normalize_optional_string(&record.group_owner_handle),
            normalize_optional_string(&record.my_role),
            default_string(record.membership_status, "active"),
            normalize_optional_bool(record.join_enabled),
            normalize_optional_string(&record.join_code),
            normalize_optional_string(&record.join_code_expires_at),
            normalize_optional_int64(record.member_count),
            normalize_optional_int64(record.last_synced_seq),
            normalize_optional_int64(record.last_read_seq),
            normalize_optional_string(&record.last_message_at),
            normalize_optional_string(&record.remote_created_at),
            normalize_optional_string(&record.remote_updated_at),
            default_string(record.stored_at, &now),
            normalize_metadata(&record.metadata),
            normalize_credential_name(&record.credential_name),
        ],
    )?;
    Ok(())
}

#[derive(Debug)]
struct GroupMemberImport {
    owner: LegacyOwnerScope,
    group_id: String,
    user_id: String,
    member_did: String,
    member_handle: String,
    profile_url: String,
    role: String,
    status: String,
    joined_at: String,
    sent_message_count: Option<i64>,
    last_synced_at: String,
    metadata: String,
    credential_name: String,
}

fn upsert_group_member(target: &Connection, record: GroupMemberImport) -> StoreResult<()> {
    let owner_identity_id = record.owner.owner_identity_id;
    let owner_did = record.owner.owner_did;
    target.execute(
        r#"
INSERT INTO group_members
    (owner_identity_id, owner_did, group_id, user_id, member_did, member_handle, profile_url, role, status,
     joined_at, sent_message_count, last_synced_at, metadata, credential_name)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
ON CONFLICT(owner_identity_id, group_id, user_id)
DO UPDATE SET
    owner_did = excluded.owner_did,
    member_did = COALESCE(excluded.member_did, group_members.member_did),
    member_handle = COALESCE(excluded.member_handle, group_members.member_handle),
    profile_url = COALESCE(excluded.profile_url, group_members.profile_url),
    role = COALESCE(excluded.role, group_members.role),
    status = excluded.status,
    joined_at = COALESCE(excluded.joined_at, group_members.joined_at),
    sent_message_count = excluded.sent_message_count,
    last_synced_at = excluded.last_synced_at,
    metadata = COALESCE(excluded.metadata, group_members.metadata),
    credential_name = COALESCE(excluded.credential_name, group_members.credential_name)"#,
        params![
            owner_identity_id,
            owner_did,
            record.group_id,
            record.user_id,
            normalize_optional_string(&record.member_did),
            normalize_optional_string(&record.member_handle),
            normalize_optional_string(&record.profile_url),
            normalize_optional_string(&record.role),
            default_string(record.status, "active"),
            normalize_optional_string(&record.joined_at),
            normalize_optional_int64(default_int64_ptr(record.sent_message_count, Some(0))),
            default_string(record.last_synced_at, &now_utc()),
            normalize_metadata(&record.metadata),
            normalize_credential_name(&record.credential_name),
        ],
    )?;
    Ok(())
}

#[derive(Debug)]
struct RelationshipEventImport {
    event_id: String,
    owner: LegacyOwnerScope,
    target_did: String,
    target_handle: String,
    event_type: String,
    source_type: String,
    source_name: String,
    source_group_id: String,
    reason: String,
    score: Option<f64>,
    status: String,
    created_at: String,
    updated_at: String,
    metadata: String,
    credential_name: String,
}

fn append_relationship_event(
    target: &Connection,
    record: RelationshipEventImport,
) -> StoreResult<String> {
    let event_id = default_string(record.event_id, &generate_id());
    let now = now_utc();
    let owner_identity_id = record.owner.owner_identity_id;
    let owner_did = record.owner.owner_did;
    target.execute(
        r#"
INSERT INTO relationship_events
    (event_id, owner_identity_id, owner_did, target_did, target_handle, event_type, source_type, source_name, source_group_id,
     reason, score, status, created_at, updated_at, metadata, credential_name)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
ON CONFLICT(owner_identity_id, event_id)
DO UPDATE SET
    owner_did = excluded.owner_did,
    target_did = excluded.target_did,
    target_handle = COALESCE(excluded.target_handle, relationship_events.target_handle),
    event_type = excluded.event_type,
    source_type = COALESCE(excluded.source_type, relationship_events.source_type),
    source_name = COALESCE(excluded.source_name, relationship_events.source_name),
    source_group_id = COALESCE(excluded.source_group_id, relationship_events.source_group_id),
    reason = COALESCE(excluded.reason, relationship_events.reason),
    score = COALESCE(excluded.score, relationship_events.score),
    status = excluded.status,
    created_at = relationship_events.created_at,
    updated_at = excluded.updated_at,
    metadata = COALESCE(excluded.metadata, relationship_events.metadata),
    credential_name = COALESCE(excluded.credential_name, relationship_events.credential_name)"#,
        params![
            event_id,
            owner_identity_id,
            owner_did,
            record.target_did,
            normalize_optional_string(&record.target_handle),
            record.event_type,
            normalize_optional_string(&record.source_type),
            normalize_optional_string(&record.source_name),
            normalize_optional_string(&record.source_group_id),
            normalize_optional_string(&record.reason),
            normalize_optional_float64(record.score),
            default_string(record.status, "pending"),
            default_string(record.created_at, &now),
            default_string(record.updated_at, &now),
            normalize_metadata(&record.metadata),
            normalize_credential_name(&record.credential_name),
        ],
    )?;
    Ok(event_id)
}

fn is_missing_database_error(err: &StoreError) -> bool {
    let message = err.to_string();
    message.contains("no such file") || message.contains("unable to open database file")
}

fn is_missing_table_error(err: &StoreError) -> bool {
    err.to_string().contains("no such table")
}

fn string_from_row(row: &Value, key: &str) -> String {
    row.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn bool_from_row(row: &Value, key: &str) -> bool {
    row.get(key).map(bool_from_value).unwrap_or(false)
}

fn bool_from_value(value: &Value) -> bool {
    match value {
        Value::Number(number) => number
            .as_i64()
            .map(|value| value != 0)
            .or_else(|| number.as_f64().map(|value| value != 0.0))
            .unwrap_or(false),
        Value::Bool(value) => *value,
        Value::String(value) => value == "1" || value.eq_ignore_ascii_case("true"),
        _ => false,
    }
}

fn bool_ptr_from_row(row: &Value, key: &str) -> Option<bool> {
    match row.get(key) {
        Some(Value::Null) | None => None,
        Some(value) => Some(bool_from_value(value)),
    }
}

fn int_from_row(row: &Value, key: &str) -> i64 {
    row.get(key).map(int_from_value).unwrap_or(0)
}

fn int_from_value(value: &Value) -> i64 {
    match value {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_f64().map(|value| value as i64))
            .unwrap_or(0),
        Value::String(value) => parse_i64_go_style(value).unwrap_or(0),
        _ => 0,
    }
}

fn int64_ptr_from_row(row: &Value, key: &str) -> Option<i64> {
    match row.get(key) {
        Some(Value::Null) | None => None,
        Some(Value::String(value)) if value.trim().is_empty() => None,
        Some(value) => Some(int_from_value(value)),
    }
}

fn float64_ptr_from_row(row: &Value, key: &str) -> Option<f64> {
    match row.get(key) {
        Some(Value::Null) | None => None,
        Some(Value::Number(number)) => number.as_f64(),
        Some(Value::String(value)) if value.trim().is_empty() => None,
        Some(Value::String(value)) => Some(value.trim().parse::<f64>().unwrap_or(0.0)),
        _ => None,
    }
}

fn metadata_from_row(row: &Value, key: &str) -> String {
    match row.get(key) {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Null) | None => String::new(),
        Some(value) => serde_json::to_string(value).unwrap_or_default(),
    }
}

fn parse_i64_go_style(raw: &str) -> Option<i64> {
    let trimmed = raw.trim_start();
    let mut end = 0usize;
    for (index, ch) in trimmed.char_indices() {
        if index == 0 && matches!(ch, '+' | '-') {
            end = ch.len_utf8();
            continue;
        }
        if ch.is_ascii_digit() {
            end = index + ch.len_utf8();
            continue;
        }
        break;
    }
    trimmed[..end].parse::<i64>().ok()
}
