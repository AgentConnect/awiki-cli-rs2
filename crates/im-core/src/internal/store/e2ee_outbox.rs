#![allow(dead_code)]

use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct E2eeOutboxRecord {
    pub(crate) outbox_id: String,
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) peer_did: String,
    pub(crate) session_id: String,
    pub(crate) original_type: String,
    pub(crate) plaintext: String,
    pub(crate) local_status: String,
    pub(crate) attempt_count: u32,
    pub(crate) sent_msg_id: String,
    pub(crate) sent_server_seq: Option<i64>,
    pub(crate) last_error_code: String,
    pub(crate) retry_hint: String,
    pub(crate) failed_msg_id: String,
    pub(crate) failed_server_seq: Option<i64>,
    pub(crate) metadata: String,
    pub(crate) last_attempt_at: String,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) credential_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct E2eeOutboxOwnerScope {
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) credential_name: String,
}

impl E2eeOutboxOwnerScope {
    pub(crate) fn for_client(client: &crate::core::ImClient) -> Self {
        Self {
            owner_identity_id: client.current_identity().id.as_str().to_owned(),
            owner_did: client.did().as_str().to_owned(),
            credential_name: client
                .current_identity()
                .local_alias
                .clone()
                .unwrap_or_default(),
        }
    }
}

pub(crate) fn queue_e2ee_outbox(
    connection: &Connection,
    record: E2eeOutboxRecord,
) -> crate::ImResult<String> {
    crate::internal::local_state::schema::ensure_schema(connection)?;
    let outbox_id = default_string(&record.outbox_id, &generate_id());
    let now = now_utc_like();
    connection
        .execute(
            r#"
INSERT INTO e2ee_outbox
    (outbox_id, owner_identity_id, owner_did, peer_did, session_id, original_type, plaintext,
     local_status, attempt_count, sent_msg_id, sent_server_seq, last_error_code, retry_hint,
     failed_msg_id, failed_server_seq, metadata, last_attempt_at, created_at, updated_at, credential_name)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)"#,
            params![
                outbox_id,
                nullable_text(&record.owner_identity_id),
                record.owner_did.trim(),
                required("peer_did", &record.peer_did)?,
                nullable_text(&record.session_id),
                default_string(&record.original_type, "text"),
                record.plaintext,
                default_string(&record.local_status, "queued"),
                record.attempt_count,
                nullable_text(&record.sent_msg_id),
                record.sent_server_seq,
                nullable_text(&record.last_error_code),
                nullable_text(&record.retry_hint),
                nullable_text(&record.failed_msg_id),
                record.failed_server_seq,
                nullable_text(&record.metadata),
                nullable_text(&record.last_attempt_at),
                default_string(&record.created_at, &now),
                default_string(&record.updated_at, &now),
                nullable_text(&record.credential_name),
            ],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    Ok(outbox_id)
}

pub(crate) fn get_e2ee_outbox(
    connection: &Connection,
    scope: &E2eeOutboxOwnerScope,
    outbox_id: &str,
) -> crate::ImResult<Option<E2eeOutboxRecord>> {
    crate::internal::local_state::schema::ensure_schema(connection)?;
    let outbox_id = required("outbox_id", outbox_id)?;
    connection
        .query_row(
            &format!(
                r#"
SELECT *
FROM e2ee_outbox
WHERE outbox_id = ?1 AND {}
LIMIT 1"#,
                owner_scope_predicate("?2", "?3", "?4")
            ),
            params![
                outbox_id,
                scope.owner_identity_id.trim(),
                scope.owner_did.trim(),
                scope.credential_name.trim(),
            ],
            outbox_from_row,
        )
        .optional()
        .map_err(crate::internal::local_state::local_state_unavailable)
}

pub(crate) fn list_e2ee_outbox(
    connection: &Connection,
    scope: &E2eeOutboxOwnerScope,
    local_status: Option<&str>,
) -> crate::ImResult<Vec<E2eeOutboxRecord>> {
    crate::internal::local_state::schema::ensure_schema(connection)?;
    let status = local_status.unwrap_or_default().trim();
    let (sql, params): (String, Vec<&dyn rusqlite::ToSql>) = if status.is_empty() {
        (
            format!(
                r#"
SELECT *
FROM e2ee_outbox
WHERE {}
ORDER BY updated_at DESC, outbox_id DESC"#,
                owner_scope_predicate("?1", "?2", "?3")
            ),
            vec![
                &scope.owner_identity_id,
                &scope.owner_did,
                &scope.credential_name,
            ],
        )
    } else {
        (
            format!(
                r#"
SELECT *
FROM e2ee_outbox
WHERE {} AND local_status = ?4
ORDER BY updated_at DESC, outbox_id DESC"#,
                owner_scope_predicate("?1", "?2", "?3")
            ),
            vec![
                &scope.owner_identity_id,
                &scope.owner_did,
                &scope.credential_name,
                &status,
            ],
        )
    };
    let mut statement = connection
        .prepare(&sql)
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let rows = statement
        .query_map(params.as_slice(), outbox_from_row)
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(crate::internal::local_state::local_state_unavailable)?);
    }
    Ok(result)
}

pub(crate) fn mark_e2ee_outbox_sent(
    connection: &Connection,
    scope: &E2eeOutboxOwnerScope,
    outbox_id: &str,
    session_id: &str,
    sent_msg_id: &str,
    sent_server_seq: Option<i64>,
    metadata: &str,
) -> crate::ImResult<()> {
    update_sent_or_failure(
        connection,
        scope,
        outbox_id,
        r#"
SET session_id = COALESCE(?5, session_id),
    local_status = 'sent',
    attempt_count = attempt_count + 1,
    sent_msg_id = COALESCE(?6, sent_msg_id),
    sent_server_seq = COALESCE(?7, sent_server_seq),
    metadata = COALESCE(?8, metadata),
    last_attempt_at = ?9,
    updated_at = ?9,
    last_error_code = NULL,
    retry_hint = NULL,
    failed_msg_id = NULL,
    failed_server_seq = NULL"#,
        &[
            &nullable_text(session_id),
            &nullable_text(sent_msg_id),
            &sent_server_seq,
            &nullable_text(metadata),
            &now_utc_like(),
        ],
    )
}

pub(crate) fn set_e2ee_outbox_failure_by_id(
    connection: &Connection,
    scope: &E2eeOutboxOwnerScope,
    outbox_id: &str,
    error_code: &str,
    retry_hint: &str,
    metadata: &str,
) -> crate::ImResult<()> {
    update_sent_or_failure(
        connection,
        scope,
        outbox_id,
        r#"
SET local_status = 'failed',
    last_error_code = ?5,
    retry_hint = COALESCE(?6, retry_hint),
    metadata = COALESCE(?7, metadata),
    updated_at = ?8"#,
        &[
            &required("error_code", error_code)?,
            &nullable_text(retry_hint),
            &nullable_text(metadata),
            &now_utc_like(),
        ],
    )
}

pub(crate) fn update_e2ee_outbox_status(
    connection: &Connection,
    scope: &E2eeOutboxOwnerScope,
    outbox_id: &str,
    status: &str,
) -> crate::ImResult<Option<E2eeOutboxRecord>> {
    crate::internal::local_state::schema::ensure_schema(connection)?;
    let outbox_id = required("outbox_id", outbox_id)?;
    let status = required("status", status)?;
    let updated_at = now_utc_like();
    connection
        .execute(
            &format!(
                r#"
UPDATE e2ee_outbox
SET local_status = ?5,
    updated_at = ?6
WHERE outbox_id = ?1 AND {}"#,
                owner_scope_predicate("?2", "?3", "?4")
            ),
            params![
                outbox_id,
                scope.owner_identity_id.trim(),
                scope.owner_did.trim(),
                scope.credential_name.trim(),
                status,
                updated_at,
            ],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    get_e2ee_outbox(connection, scope, &outbox_id)
}

pub(crate) fn retry_e2ee_outbox(
    connection: &Connection,
    scope: &E2eeOutboxOwnerScope,
    outbox_id: &str,
) -> crate::ImResult<Option<E2eeOutboxRecord>> {
    update_e2ee_outbox_status(connection, scope, outbox_id, "queued")
}

pub(crate) fn drop_e2ee_outbox(
    connection: &Connection,
    scope: &E2eeOutboxOwnerScope,
    outbox_id: &str,
) -> crate::ImResult<Option<E2eeOutboxRecord>> {
    update_e2ee_outbox_status(connection, scope, outbox_id, "dropped")
}

pub(crate) fn secure_outbox_entry_from_record(
    record: &E2eeOutboxRecord,
) -> crate::ImResult<crate::secure::SecureOutboxEntry> {
    Ok(crate::secure::SecureOutboxEntry {
        id: crate::secure::SecureOutboxId::parse(&record.outbox_id)?,
        target: crate::messages::MessageTarget::Direct(crate::ids::PeerRef::parse(
            &record.peer_did,
            "",
        )?),
        message_kind: default_string(&record.original_type, "text"),
        status: secure_outbox_status(&record.local_status),
        attempt_count: record.attempt_count,
        last_error: last_error_problem(record),
        created_at: optional_string(&record.created_at),
        updated_at: optional_string(&record.updated_at),
    })
}

fn update_sent_or_failure(
    connection: &Connection,
    scope: &E2eeOutboxOwnerScope,
    outbox_id: &str,
    set_clause: &str,
    tail_params: &[&dyn rusqlite::ToSql],
) -> crate::ImResult<()> {
    crate::internal::local_state::schema::ensure_schema(connection)?;
    let outbox_id = required("outbox_id", outbox_id)?;
    let sql = format!(
        r#"
UPDATE e2ee_outbox
{set_clause}
WHERE outbox_id = ?1 AND {}"#,
        owner_scope_predicate("?2", "?3", "?4")
    );
    let mut values: Vec<&dyn rusqlite::ToSql> = vec![
        &outbox_id,
        &scope.owner_identity_id,
        &scope.owner_did,
        &scope.credential_name,
    ];
    values.extend_from_slice(tail_params);
    connection
        .execute(&sql, values.as_slice())
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    Ok(())
}

fn owner_scope_predicate(
    owner_identity_id_param: &str,
    owner_did_param: &str,
    credential_name_param: &str,
) -> String {
    format!(
        r#"(owner_identity_id = {owner_identity_id_param}
    OR ((owner_identity_id IS NULL OR TRIM(owner_identity_id) = '') AND owner_did = {owner_did_param})
    OR ((owner_identity_id IS NULL OR TRIM(owner_identity_id) = '') AND TRIM(COALESCE(credential_name, '')) <> '' AND credential_name = {credential_name_param}))"#
    )
}

fn outbox_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<E2eeOutboxRecord> {
    Ok(E2eeOutboxRecord {
        outbox_id: row.get("outbox_id")?,
        owner_identity_id: row
            .get::<_, Option<String>>("owner_identity_id")?
            .unwrap_or_default(),
        owner_did: row
            .get::<_, Option<String>>("owner_did")?
            .unwrap_or_default(),
        peer_did: row
            .get::<_, Option<String>>("peer_did")?
            .unwrap_or_default(),
        session_id: row
            .get::<_, Option<String>>("session_id")?
            .unwrap_or_default(),
        original_type: row
            .get::<_, Option<String>>("original_type")?
            .unwrap_or_default(),
        plaintext: row
            .get::<_, Option<String>>("plaintext")?
            .unwrap_or_default(),
        local_status: row
            .get::<_, Option<String>>("local_status")?
            .unwrap_or_default(),
        attempt_count: row.get::<_, Option<i64>>("attempt_count")?.unwrap_or(0) as u32,
        sent_msg_id: row
            .get::<_, Option<String>>("sent_msg_id")?
            .unwrap_or_default(),
        sent_server_seq: row.get("sent_server_seq")?,
        last_error_code: row
            .get::<_, Option<String>>("last_error_code")?
            .unwrap_or_default(),
        retry_hint: row
            .get::<_, Option<String>>("retry_hint")?
            .unwrap_or_default(),
        failed_msg_id: row
            .get::<_, Option<String>>("failed_msg_id")?
            .unwrap_or_default(),
        failed_server_seq: row.get("failed_server_seq")?,
        metadata: row
            .get::<_, Option<String>>("metadata")?
            .unwrap_or_default(),
        last_attempt_at: row
            .get::<_, Option<String>>("last_attempt_at")?
            .unwrap_or_default(),
        created_at: row
            .get::<_, Option<String>>("created_at")?
            .unwrap_or_default(),
        updated_at: row
            .get::<_, Option<String>>("updated_at")?
            .unwrap_or_default(),
        credential_name: row
            .get::<_, Option<String>>("credential_name")?
            .unwrap_or_default(),
    })
}

fn secure_outbox_status(status: &str) -> crate::secure::SecureOutboxStatus {
    match status.trim().to_ascii_lowercase().as_str() {
        "sending" => crate::secure::SecureOutboxStatus::Sending,
        "failed" => crate::secure::SecureOutboxStatus::Failed,
        "sent" => crate::secure::SecureOutboxStatus::Sent,
        "dropped" => crate::secure::SecureOutboxStatus::Dropped,
        _ => crate::secure::SecureOutboxStatus::Queued,
    }
}

fn last_error_problem(record: &E2eeOutboxRecord) -> Option<crate::secure::SecureProblem> {
    let code = record.last_error_code.trim();
    if code.is_empty() {
        return None;
    }
    Some(crate::secure::SecureProblem {
        code: crate::secure::SecureProblemCode::Unknown,
        message: code.to_owned(),
        retryable: record.retry_hint.trim() != "drop",
    })
}

fn required(field: &str, value: &str) -> crate::ImResult<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            format!("{field} must not be empty"),
        ));
    }
    Ok(value.to_owned())
}

fn optional_string(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn nullable_text(value: &str) -> Option<String> {
    optional_string(value)
}

fn default_string(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.to_owned()
    } else {
        value.to_owned()
    }
}

fn now_utc_like() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

fn generate_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("outbox-{nanos}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secure_outbox_lists_failed_entries_without_plaintext() {
        let db = Connection::open_in_memory().unwrap();
        let scope = scope("alice-id", "did:example:alice", "alice");
        queue_e2ee_outbox(
            &db,
            E2eeOutboxRecord {
                outbox_id: "outbox-1".to_owned(),
                owner_identity_id: scope.owner_identity_id.clone(),
                owner_did: scope.owner_did.clone(),
                credential_name: scope.credential_name.clone(),
                peer_did: "did:example:bob".to_owned(),
                plaintext: "secret plaintext".to_owned(),
                local_status: "failed".to_owned(),
                last_error_code: "pending_confirmation".to_owned(),
                retry_hint: "retry".to_owned(),
                created_at: "2026-05-24T00:00:00Z".to_owned(),
                updated_at: "2026-05-24T00:00:01Z".to_owned(),
                ..E2eeOutboxRecord::default()
            },
        )
        .unwrap();

        let failed = list_e2ee_outbox(&db, &scope, Some("failed")).unwrap();

        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].plaintext, "secret plaintext");
        let entry = secure_outbox_entry_from_record(&failed[0]).unwrap();
        assert_eq!(entry.id.as_str(), "outbox-1");
        assert_eq!(entry.message_kind, "text");
        assert!(matches!(
            entry.status,
            crate::secure::SecureOutboxStatus::Failed
        ));
        assert_eq!(
            entry.last_error.as_ref().map(|problem| &problem.message),
            Some(&"pending_confirmation".to_owned())
        );
        assert!(!format!("{entry:?}").contains("secret plaintext"));
    }

    #[test]
    fn secure_outbox_retry_and_drop_are_owner_scoped() {
        let db = Connection::open_in_memory().unwrap();
        let alice = scope("alice-id", "did:example:alice", "alice");
        let charlie = scope("charlie-id", "did:example:charlie", "charlie");
        queue_e2ee_outbox(
            &db,
            E2eeOutboxRecord {
                outbox_id: "outbox-1".to_owned(),
                owner_identity_id: alice.owner_identity_id.clone(),
                owner_did: alice.owner_did.clone(),
                credential_name: alice.credential_name.clone(),
                peer_did: "did:example:bob".to_owned(),
                plaintext: "secret".to_owned(),
                local_status: "failed".to_owned(),
                created_at: "2026-05-24T00:00:00Z".to_owned(),
                updated_at: "2026-05-24T00:00:00Z".to_owned(),
                ..E2eeOutboxRecord::default()
            },
        )
        .unwrap();

        assert!(retry_e2ee_outbox(&db, &charlie, "outbox-1")
            .unwrap()
            .is_none());
        let retried = retry_e2ee_outbox(&db, &alice, "outbox-1").unwrap().unwrap();
        assert_eq!(retried.local_status, "queued");
        let dropped = drop_e2ee_outbox(&db, &alice, "outbox-1").unwrap().unwrap();
        assert_eq!(dropped.local_status, "dropped");
    }

    fn scope(identity: &str, did: &str, credential: &str) -> E2eeOutboxOwnerScope {
        E2eeOutboxOwnerScope {
            owner_identity_id: identity.to_owned(),
            owner_did: did.to_owned(),
            credential_name: credential.to_owned(),
        }
    }
}
