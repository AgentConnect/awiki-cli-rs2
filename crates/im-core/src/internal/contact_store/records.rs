use rusqlite::{params, Connection, OptionalExtension, Statement};
use serde_json::Value;
use time::OffsetDateTime;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ContactRecord {
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) did: String,
    pub(crate) name: String,
    pub(crate) handle: String,
    pub(crate) nick_name: String,
    pub(crate) bio: String,
    pub(crate) profile_md: String,
    pub(crate) tags: String,
    pub(crate) relationship: String,
    pub(crate) source_type: String,
    pub(crate) source_name: String,
    pub(crate) source_group_id: String,
    pub(crate) connected_at: String,
    pub(crate) recommended_reason: String,
    pub(crate) followed: Option<bool>,
    pub(crate) messaged: Option<bool>,
    pub(crate) note: String,
    pub(crate) first_seen_at: String,
    pub(crate) last_seen_at: String,
    pub(crate) metadata: String,
    pub(crate) credential_name: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ContactHandleBindingRecord {
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) handle: String,
    pub(crate) did: String,
    pub(crate) is_current: bool,
    pub(crate) first_seen_at: String,
    pub(crate) last_seen_at: String,
    pub(crate) source_type: String,
    pub(crate) source_group_id: String,
    pub(crate) metadata: String,
    pub(crate) credential_name: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct RelationshipEventRecord {
    pub(crate) event_id: String,
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) target_did: String,
    pub(crate) target_handle: String,
    pub(crate) event_type: String,
    pub(crate) source_type: String,
    pub(crate) source_name: String,
    pub(crate) source_group_id: String,
    pub(crate) reason: String,
    pub(crate) score: Option<f64>,
    pub(crate) status: String,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) metadata: String,
    pub(crate) credential_name: String,
}

pub(crate) fn ensure_schema(connection: &Connection) -> crate::ImResult<()> {
    crate::internal::local_state::schema::ensure_schema(connection)
}

pub(crate) fn get_contact_by_did(
    connection: &Connection,
    owner_identity_id: &str,
    _owner_did: &str,
    did: &str,
) -> crate::ImResult<ContactRecord> {
    query_one_contact(
        connection,
        &format!(
            "SELECT * FROM contacts WHERE {} AND did = ?2",
            owner_predicate(1)
        ),
        &[
            &required_owner_identity_id(owner_identity_id)?,
            &did.trim().to_string(),
        ],
    )
}

pub(crate) fn get_contact_by_did_json(
    connection: &Connection,
    owner_did: &str,
    did: &str,
) -> crate::ImResult<Value> {
    let _ = (connection, owner_did, did);
    Err(crate::ImError::invalid_input(
        Some("owner_identity_id".to_owned()),
        "owner_identity_id is required",
    ))
}

pub(crate) fn get_contact_by_did_json_for_owner_identity(
    connection: &Connection,
    owner_identity_id: &str,
    _owner_did: &str,
    did: &str,
) -> crate::ImResult<Value> {
    query_one_json(
        connection,
        &format!(
            "SELECT * FROM contacts WHERE {} AND did = ?2",
            owner_predicate(1)
        ),
        &[
            &required_owner_identity_id(owner_identity_id)?,
            &did.trim().to_string(),
        ],
    )
}

pub(crate) fn get_current_contact_by_handle(
    connection: &Connection,
    owner_identity_id: &str,
    owner_did: &str,
    handle: &str,
) -> crate::ImResult<ContactRecord> {
    get_current_contact_by_handle_for_owner(connection, owner_identity_id, owner_did, handle)
}

fn get_current_contact_by_handle_for_owner(
    connection: &Connection,
    owner_identity_id: &str,
    _owner_did: &str,
    handle: &str,
) -> crate::ImResult<ContactRecord> {
    let owner_identity_id = required_owner_identity_id(owner_identity_id)?;
    let handle = handle.trim().to_string();
    if handle.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("handle".to_string()),
            "handle must not be empty",
        ));
    }
    let from_contact = query_one_contact(
        connection,
        &format!(
            "SELECT * FROM contacts WHERE {} AND handle = ?2",
            owner_predicate(1)
        ),
        &[&owner_identity_id, &handle],
    );
    if from_contact.is_ok() {
        return from_contact;
    }
    query_one_contact(
        connection,
        r#"
SELECT contacts.*
FROM contacts
JOIN contact_handle_bindings
  ON contact_handle_bindings.owner_identity_id = contacts.owner_identity_id
 AND contact_handle_bindings.did = contacts.did
WHERE contact_handle_bindings.owner_identity_id = ?1
  AND contact_handle_bindings.handle = ?2
  AND contact_handle_bindings.is_current = 1
ORDER BY contact_handle_bindings.last_seen_at DESC
LIMIT 1"#,
        &[&owner_identity_id, &handle],
    )
}

fn get_current_contact_by_handle_for_owner_did(
    connection: &Connection,
    owner_did: &str,
    handle: &str,
) -> crate::ImResult<ContactRecord> {
    let _ = (connection, owner_did, handle);
    Err(crate::ImError::invalid_input(
        Some("owner_identity_id".to_owned()),
        "owner_identity_id is required",
    ))
}

pub(crate) fn get_current_contact_by_handle_json(
    connection: &Connection,
    owner_did: &str,
    handle: &str,
) -> crate::ImResult<Value> {
    let record = get_current_contact_by_handle_for_owner_did(connection, owner_did, handle)?;
    contact_record_json(&record)
}

pub(crate) fn get_current_contact_by_handle_json_for_owner_identity(
    connection: &Connection,
    owner_identity_id: &str,
    owner_did: &str,
    handle: &str,
) -> crate::ImResult<Value> {
    let record =
        get_current_contact_by_handle_for_owner(connection, owner_identity_id, owner_did, handle)?;
    contact_record_json(&record)
}

pub(crate) fn resolve_contact_handle_by_did(
    connection: &Connection,
    owner_did: &str,
    did: &str,
) -> crate::ImResult<String> {
    let _ = (connection, owner_did, did);
    Err(crate::ImError::invalid_input(
        Some("owner_identity_id".to_owned()),
        "owner_identity_id is required",
    ))
}

pub(crate) fn resolve_contact_handle_by_did_for_owner_identity(
    connection: &Connection,
    owner_identity_id: &str,
    _owner_did: &str,
    did: &str,
) -> crate::ImResult<String> {
    let owner_identity_id = required_owner_identity_id(owner_identity_id)?;
    let did = did.trim().to_string();
    let contact_handle = connection
        .query_row(
            &format!(
                "SELECT handle FROM contacts WHERE {} AND did = ?2 AND TRIM(COALESCE(handle, '')) <> ''",
                owner_predicate(1)
            ),
            params![owner_identity_id.as_str(), did.as_str()],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(super::local_state_unavailable)?
        .flatten()
        .unwrap_or_default();
    if !contact_handle.is_empty() {
        return Ok(contact_handle);
    }
    let binding_handle = connection
        .query_row(
            r#"
SELECT handle
FROM contact_handle_bindings
WHERE owner_identity_id = ?1
  AND did = ?2
ORDER BY is_current DESC, last_seen_at DESC
LIMIT 1"#,
            params![owner_identity_id.as_str(), did.as_str()],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(super::local_state_unavailable)?
        .flatten()
        .unwrap_or_default();
    Ok(binding_handle)
}

pub(crate) fn list_dids_by_handle(
    connection: &Connection,
    owner_did: &str,
    handle: &str,
) -> crate::ImResult<Vec<String>> {
    let _ = (connection, owner_did, handle);
    Err(crate::ImError::invalid_input(
        Some("owner_identity_id".to_owned()),
        "owner_identity_id is required",
    ))
}

pub(crate) fn list_dids_by_handle_for_owner_identity(
    connection: &Connection,
    owner_identity_id: &str,
    _owner_did: &str,
    handle: &str,
) -> crate::ImResult<Vec<String>> {
    let owner_identity_id = required_owner_identity_id(owner_identity_id)?;
    let handle = handle.trim().to_string();
    let mut statement = connection
        .prepare(
            r#"
SELECT did
FROM contact_handle_bindings
WHERE owner_identity_id = ?1
  AND handle = ?2
ORDER BY is_current DESC, last_seen_at DESC"#,
        )
        .map_err(super::local_state_unavailable)?;
    let mut rows = statement
        .query(params![owner_identity_id.as_str(), handle.as_str()])
        .map_err(super::local_state_unavailable)?;
    let mut result = Vec::new();
    while let Some(row) = rows.next().map_err(super::local_state_unavailable)? {
        let did = row
            .get::<_, Option<String>>(0)
            .map_err(super::local_state_unavailable)?
            .unwrap_or_default();
        if did.is_empty() || result.iter().any(|known| known == &did) {
            continue;
        }
        result.push(did);
    }
    if !result.is_empty() {
        return Ok(result);
    }
    let did = connection
        .query_row(
            &format!(
                "SELECT did FROM contacts WHERE {} AND handle = ?2",
                owner_predicate(1)
            ),
            params![owner_identity_id.as_str(), handle.as_str()],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(super::local_state_unavailable)?
        .flatten()
        .unwrap_or_default();
    if did.is_empty() {
        Ok(Vec::new())
    } else {
        Ok(vec![did])
    }
}

pub(crate) fn list_contact_handle_history(
    connection: &Connection,
    handle: &str,
) -> crate::ImResult<Vec<Value>> {
    let handle = handle.trim().to_string();
    let mut statement = connection
        .prepare(
            r#"
SELECT owner_did, handle, did, is_current, first_seen_at, last_seen_at, source_type, source_group_id, metadata
FROM contact_handle_bindings
WHERE handle = ?1
ORDER BY owner_did ASC, is_current DESC, last_seen_at DESC"#,
        )
        .map_err(super::local_state_unavailable)?;
    let names = column_names(&statement);
    let mut rows = statement
        .query(params![handle.as_str()])
        .map_err(super::local_state_unavailable)?;
    let mut result = Vec::new();
    while let Some(row) = rows.next().map_err(super::local_state_unavailable)? {
        result.push(row_to_json(row, &names)?);
    }
    Ok(result)
}

pub(crate) fn list_contacts(
    connection: &Connection,
    owner_identity_id: &str,
    _owner_did: &str,
    limit: i64,
) -> crate::ImResult<Vec<ContactRecord>> {
    let owner_identity_id = required_owner_identity_id(owner_identity_id)?;
    let limit = if limit <= 0 { 100 } else { limit };
    let mut statement = connection
        .prepare(
            r#"
SELECT *
FROM contacts
WHERE owner_identity_id = ?1
ORDER BY COALESCE(last_seen_at, first_seen_at, connected_at) DESC, did ASC
LIMIT ?2"#,
        )
        .map_err(super::local_state_unavailable)?;
    let rows = statement
        .query_map(params![owner_identity_id.as_str(), limit], contact_from_row)
        .map_err(super::local_state_unavailable)?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(super::local_state_unavailable)?);
    }
    Ok(result)
}

pub(crate) fn list_contact_dids_for_message_history_recovery(
    connection: &Connection,
    owner_identity_id: &str,
    owner_did: &str,
    limit: i64,
) -> crate::ImResult<Vec<String>> {
    let owner_identity_id = required_owner_identity_id(owner_identity_id)?;
    let owner_did = normalize_owner_did(owner_did);
    let limit = if limit <= 0 { 50 } else { limit };
    let mut statement = connection
        .prepare(
            r#"
SELECT did
FROM contacts
WHERE owner_identity_id = ?1
  AND TRIM(COALESCE(did, '')) <> ''
  AND did <> ?2
ORDER BY
  messaged DESC,
  CASE WHEN TRIM(COALESCE(last_seen_at, first_seen_at, connected_at, '')) = '' THEN 1 ELSE 0 END,
  COALESCE(last_seen_at, first_seen_at, connected_at) DESC,
  did ASC
LIMIT ?3"#,
        )
        .map_err(super::local_state_unavailable)?;
    let rows = statement
        .query_map(
            params![owner_identity_id.as_str(), owner_did.as_str(), limit],
            |row| row.get::<_, Option<String>>("did"),
        )
        .map_err(super::local_state_unavailable)?;
    let mut result = Vec::new();
    for row in rows {
        let did = row
            .map_err(super::local_state_unavailable)?
            .unwrap_or_default();
        if !did.trim().is_empty() {
            result.push(did);
        }
    }
    Ok(result)
}

pub(crate) fn upsert_contact(
    connection: &mut Connection,
    record: ContactRecord,
) -> crate::ImResult<()> {
    if record.did.trim().is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("did".to_string()),
            "contact did is required",
        ));
    }
    ensure_schema(connection)?;
    let owner_identity_id = required_owner_identity_id(&record.owner_identity_id)?;
    let owner_did = normalize_owner_did(&record.owner_did);
    let did = record.did.trim().to_string();
    let handle = record.handle.trim().to_string();
    let transaction = connection
        .transaction()
        .map_err(super::local_state_unavailable)?;
    let now = now_utc();
    let existing_by_did = query_contact_did_handle(
        &transaction,
        &format!(
            "SELECT did, handle FROM contacts WHERE {} AND did = ?2",
            owner_predicate(1)
        ),
        &owner_identity_id,
        &did,
    )?;
    let existing_by_handle = if handle.is_empty() {
        Vec::new()
    } else {
        query_contact_did_handle(
            &transaction,
            &format!(
                "SELECT did, handle FROM contacts WHERE {} AND handle = ?2",
                owner_predicate(1)
            ),
            &owner_identity_id,
            &handle,
        )?
    };
    if !handle.is_empty() && !existing_by_handle.is_empty() && existing_by_handle[0].0.trim() != did
    {
        let affected = transaction
            .execute(
                &format!(
                    "UPDATE contacts SET handle = NULL, last_seen_at = ?1 WHERE {} AND did = ?3",
                    owner_predicate(2)
                ),
                params![
                    now.as_str(),
                    owner_identity_id.as_str(),
                    existing_by_handle[0].0.as_str()
                ],
            )
            .map_err(super::local_state_unavailable)?;
        ensure_expected_rows(
            affected,
            "clear_previous_contact_handle",
            &owner_identity_id,
            existing_by_handle[0].0.as_str(),
        )?;
    }
    if existing_by_did.is_empty() {
        insert_contact(
            &transaction,
            &record,
            &owner_identity_id,
            &owner_did,
            &did,
            &now,
        )?;
    } else {
        update_contact(
            &transaction,
            &record,
            &owner_identity_id,
            &did,
            &handle,
            &now,
        )?;
    }
    if !handle.is_empty() {
        upsert_contact_handle_binding(
            &transaction,
            ContactHandleBindingRecord {
                owner_identity_id: owner_identity_id.clone(),
                owner_did: owner_did.clone(),
                handle,
                did,
                is_current: true,
                first_seen_at: default_string(record.first_seen_at.clone(), &now),
                last_seen_at: default_string(record.last_seen_at.clone(), &now),
                source_type: record.source_type.clone(),
                source_group_id: record.source_group_id.clone(),
                metadata: record.metadata.clone(),
                credential_name: record.credential_name.clone(),
            },
        )?;
    }
    transaction
        .commit()
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

pub(crate) fn append_relationship_event(
    connection: &Connection,
    record: RelationshipEventRecord,
) -> crate::ImResult<String> {
    if record.target_did.trim().is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("target_did".to_string()),
            "target did is required",
        ));
    }
    if record.event_type.trim().is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("event_type".to_string()),
            "event type is required",
        ));
    }
    ensure_schema(connection)?;
    let now = now_utc();
    let owner_identity_id = required_owner_identity_id(&record.owner_identity_id)?;
    let owner_did = normalize_owner_did(&record.owner_did);
    let event_id = default_string(
        record.event_id.clone(),
        &relationship_event_id(&record, &now),
    );
    let created_at = default_string(record.created_at.clone(), &now);
    let updated_at = default_string(record.updated_at.clone(), &created_at);
    connection
        .execute(
            r#"
INSERT INTO relationship_events
    (event_id, owner_identity_id, owner_did, target_did, target_handle, event_type, source_type, source_name,
     source_group_id, reason, score, status, created_at, updated_at, metadata, credential_name)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
ON CONFLICT(owner_identity_id, event_id) DO UPDATE SET
    owner_did = excluded.owner_did,
    target_did = excluded.target_did,
    target_handle = excluded.target_handle,
    event_type = excluded.event_type,
    source_type = excluded.source_type,
    source_name = excluded.source_name,
    source_group_id = excluded.source_group_id,
    reason = excluded.reason,
    score = excluded.score,
    status = excluded.status,
    updated_at = excluded.updated_at,
    metadata = excluded.metadata,
    credential_name = COALESCE(excluded.credential_name, relationship_events.credential_name)"#,
            params![
                event_id.as_str(),
                owner_identity_id.as_str(),
                owner_did.as_str(),
                record.target_did.trim(),
                normalize_optional_string(&record.target_handle),
                record.event_type.trim(),
                normalize_optional_string(&record.source_type),
                normalize_optional_string(&record.source_name),
                normalize_optional_string(&record.source_group_id),
                normalize_optional_string(&record.reason),
                record.score,
                default_string(record.status.clone(), "applied"),
                created_at.as_str(),
                updated_at.as_str(),
                normalize_metadata(&record.metadata),
                normalize_optional_string(&record.credential_name),
            ],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(event_id)
}

pub(crate) fn contact_to_dto(record: &ContactRecord) -> crate::ImResult<crate::directory::Contact> {
    let metadata = metadata_object(&record.metadata);
    let avatar_uri = metadata_string(&metadata, "avatar_uri")
        .or_else(|| metadata_string(&metadata, "avatar_url"))
        .or_else(|| metadata_string(&metadata, "avatar"));
    let avatar_url = metadata_string(&metadata, "avatar_url")
        .or_else(|| metadata_string(&metadata, "avatar"))
        .or_else(|| avatar_uri.clone());
    Ok(crate::directory::Contact {
        did: crate::ids::Did::parse(record.did.trim())?,
        handle: optional_string(&record.handle)
            .map(|handle| crate::ids::Handle::parse(handle, ""))
            .transpose()?,
        display_name: optional_string(&record.name).or_else(|| optional_string(&record.nick_name)),
        avatar_uri,
        avatar_url,
        profile_uri: metadata_string(&metadata, "profile_uri")
            .or_else(|| metadata_string(&metadata, "profile_url")),
        subject_type: metadata_string(&metadata, "subject_type"),
        relationship: optional_string(&record.relationship),
        followed: record.followed.unwrap_or(false),
        messaged: record.messaged.unwrap_or(false),
        note: optional_string(&record.note),
        last_seen_at: optional_string(&record.last_seen_at),
    })
}

pub(crate) fn display_profile_from_record(
    record: &ContactRecord,
) -> crate::ImResult<crate::directory::DisplayProfile> {
    let contact = contact_to_dto(record)?;
    Ok(crate::directory::DisplayProfile {
        did: Some(contact.did),
        handle: contact.handle,
        display_name: contact.display_name,
        avatar_uri: contact.avatar_uri,
        avatar_url: contact.avatar_url,
        profile_uri: contact.profile_uri,
        subject_type: contact.subject_type,
        cache_hit: true,
        is_stale: false,
        legacy_fallback: true,
        warnings: Vec::new(),
    })
}

pub(crate) fn relation_status_from_record(
    peer: crate::ids::PeerRef,
    record: Option<ContactRecord>,
) -> crate::ImResult<crate::directory::RelationStatus> {
    let did = match &record {
        Some(record) => Some(crate::ids::Did::parse(record.did.trim())?),
        None if peer.as_str().trim().starts_with("did:") => {
            Some(crate::ids::Did::parse(peer.as_str())?)
        }
        None => None,
    };
    Ok(crate::directory::RelationStatus {
        peer,
        did,
        is_contact: record.is_some(),
        followed: record
            .as_ref()
            .and_then(|record| record.followed)
            .unwrap_or(false),
        messaged: record
            .as_ref()
            .and_then(|record| record.messaged)
            .unwrap_or(false),
        relationship: record
            .as_ref()
            .and_then(|record| optional_string(&record.relationship)),
    })
}

fn insert_contact(
    connection: &Connection,
    record: &ContactRecord,
    owner_identity_id: &str,
    owner_did: &str,
    did: &str,
    now: &str,
) -> crate::ImResult<()> {
    connection
        .execute(
            r#"
INSERT INTO contacts
    (owner_identity_id, owner_did, did, name, handle, nick_name, bio, profile_md, tags, relationship, source_type,
     source_name, source_group_id, connected_at, recommended_reason, followed, messaged, note,
     first_seen_at, last_seen_at, metadata, credential_name)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)"#,
            params![
                owner_identity_id,
                owner_did,
                did,
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
                default_string(record.first_seen_at.clone(), now),
                default_string(record.last_seen_at.clone(), now),
                normalize_metadata(&record.metadata),
                normalize_credential_name(&record.credential_name),
            ],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

fn update_contact(
    connection: &Connection,
    record: &ContactRecord,
    owner_identity_id: &str,
    did: &str,
    handle: &str,
    now: &str,
) -> crate::ImResult<()> {
    let affected = connection
        .execute(
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
    owner_did = ?19,
    credential_name = COALESCE(?20, credential_name)
WHERE owner_identity_id = ?21 AND did = ?22"#,
            params![
                normalize_optional_string(&record.name),
                normalize_optional_string(handle),
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
                normalize_owner_did(&record.owner_did),
                normalize_optional_string(&record.credential_name),
                owner_identity_id,
                did,
            ],
        )
        .map_err(super::local_state_unavailable)?;
    ensure_expected_rows(affected, "update_contact", owner_identity_id, did)?;
    Ok(())
}

fn upsert_contact_handle_binding(
    connection: &Connection,
    record: ContactHandleBindingRecord,
) -> crate::ImResult<()> {
    let handle = record.handle.trim().to_string();
    let did = record.did.trim().to_string();
    if handle.is_empty() || did.is_empty() {
        return Ok(());
    }
    let owner_identity_id = required_owner_identity_id(&record.owner_identity_id)?;
    let owner_did = normalize_owner_did(&record.owner_did);
    let first_seen_at = default_string(record.first_seen_at.clone(), &now_utc());
    let last_seen_at = default_string(record.last_seen_at.clone(), &first_seen_at);
    if record.is_current {
        connection
            .execute(
                &format!(
                    r#"
UPDATE contact_handle_bindings
SET is_current = 0,
    last_seen_at = CASE
        WHEN last_seen_at IS NULL OR last_seen_at < ?1 THEN ?2
        ELSE last_seen_at
    END
WHERE {} AND handle = ?4 AND did <> ?5"#,
                    owner_predicate(3)
                ),
                params![
                    last_seen_at.as_str(),
                    last_seen_at.as_str(),
                    owner_identity_id.as_str(),
                    handle.as_str(),
                    did.as_str(),
                ],
            )
            .map_err(super::local_state_unavailable)?;
    }
    let affected = connection
        .execute(
            r#"
INSERT INTO contact_handle_bindings
    (owner_identity_id, owner_did, handle, did, is_current, first_seen_at, last_seen_at, source_type,
     source_group_id, metadata, credential_name)
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
                owner_identity_id.as_str(),
                owner_did.as_str(),
                handle.as_str(),
                did.as_str(),
                bool_to_int(record.is_current),
                first_seen_at.as_str(),
                last_seen_at.as_str(),
                normalize_optional_string(&record.source_type),
                normalize_optional_string(&record.source_group_id),
                normalize_metadata(&record.metadata),
                normalize_credential_name(&record.credential_name),
            ],
        )
        .map_err(super::local_state_unavailable)?;
    ensure_expected_rows(
        affected,
        "upsert_contact_handle_binding",
        &owner_identity_id,
        &did,
    )?;
    Ok(())
}

fn backfill_contact_handle_bindings(connection: &Connection) -> crate::ImResult<()> {
    let _ = connection;
    Ok(())
}

fn query_contact_did_handle(
    connection: &Connection,
    statement: &str,
    owner_identity_id: &str,
    value: &str,
) -> crate::ImResult<Vec<(String, String)>> {
    let mut statement = connection
        .prepare(statement)
        .map_err(super::local_state_unavailable)?;
    let rows = statement
        .query_map(params![owner_identity_id, value], |row| {
            Ok((
                row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            ))
        })
        .map_err(super::local_state_unavailable)?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(super::local_state_unavailable)?);
    }
    Ok(result)
}

fn query_one_contact(
    connection: &Connection,
    statement: &str,
    params: &[&dyn rusqlite::ToSql],
) -> crate::ImResult<ContactRecord> {
    let mut statement = connection
        .prepare(statement)
        .map_err(super::local_state_unavailable)?;
    let mut rows = statement
        .query(params)
        .map_err(super::local_state_unavailable)?;
    if let Some(row) = rows.next().map_err(super::local_state_unavailable)? {
        return contact_from_row(row).map_err(super::local_state_unavailable);
    }
    Err(crate::ImError::PeerNotFound {
        peer: "contact".to_string(),
    })
}

fn query_one_json(
    connection: &Connection,
    statement: &str,
    params: &[&dyn rusqlite::ToSql],
) -> crate::ImResult<Value> {
    let mut statement = connection
        .prepare(statement)
        .map_err(super::local_state_unavailable)?;
    let names = column_names(&statement);
    let mut rows = statement
        .query(params)
        .map_err(super::local_state_unavailable)?;
    if let Some(row) = rows.next().map_err(super::local_state_unavailable)? {
        return row_to_json(row, &names);
    }
    Err(crate::ImError::PeerNotFound {
        peer: "contact".to_string(),
    })
}

fn contact_record_json(record: &ContactRecord) -> crate::ImResult<Value> {
    Ok(serde_json::json!({
        "owner_identity_id": optional_json_string(&record.owner_identity_id),
        "owner_did": record.owner_did,
        "did": record.did,
        "name": optional_json_string(&record.name),
        "handle": optional_json_string(&record.handle),
        "nick_name": optional_json_string(&record.nick_name),
        "bio": optional_json_string(&record.bio),
        "profile_md": optional_json_string(&record.profile_md),
        "tags": optional_json_string(&record.tags),
        "relationship": optional_json_string(&record.relationship),
        "source_type": optional_json_string(&record.source_type),
        "source_name": optional_json_string(&record.source_name),
        "source_group_id": optional_json_string(&record.source_group_id),
        "connected_at": optional_json_string(&record.connected_at),
        "recommended_reason": optional_json_string(&record.recommended_reason),
        "followed": record.followed.unwrap_or(false) as i64,
        "messaged": record.messaged.unwrap_or(false) as i64,
        "note": optional_json_string(&record.note),
        "first_seen_at": optional_json_string(&record.first_seen_at),
        "last_seen_at": optional_json_string(&record.last_seen_at),
        "metadata": optional_json_string(&record.metadata),
    }))
}

fn contact_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ContactRecord> {
    Ok(ContactRecord {
        owner_identity_id: row
            .get::<_, Option<String>>("owner_identity_id")?
            .unwrap_or_default(),
        owner_did: row
            .get::<_, Option<String>>("owner_did")?
            .unwrap_or_default(),
        did: row.get::<_, Option<String>>("did")?.unwrap_or_default(),
        name: row.get::<_, Option<String>>("name")?.unwrap_or_default(),
        handle: row.get::<_, Option<String>>("handle")?.unwrap_or_default(),
        nick_name: row
            .get::<_, Option<String>>("nick_name")?
            .unwrap_or_default(),
        bio: row.get::<_, Option<String>>("bio")?.unwrap_or_default(),
        profile_md: row
            .get::<_, Option<String>>("profile_md")?
            .unwrap_or_default(),
        tags: row.get::<_, Option<String>>("tags")?.unwrap_or_default(),
        relationship: row
            .get::<_, Option<String>>("relationship")?
            .unwrap_or_default(),
        source_type: row
            .get::<_, Option<String>>("source_type")?
            .unwrap_or_default(),
        source_name: row
            .get::<_, Option<String>>("source_name")?
            .unwrap_or_default(),
        source_group_id: row
            .get::<_, Option<String>>("source_group_id")?
            .unwrap_or_default(),
        connected_at: row
            .get::<_, Option<String>>("connected_at")?
            .unwrap_or_default(),
        recommended_reason: row
            .get::<_, Option<String>>("recommended_reason")?
            .unwrap_or_default(),
        followed: row
            .get::<_, Option<i64>>("followed")?
            .map(|value| value != 0),
        messaged: row
            .get::<_, Option<i64>>("messaged")?
            .map(|value| value != 0),
        note: row.get::<_, Option<String>>("note")?.unwrap_or_default(),
        first_seen_at: row
            .get::<_, Option<String>>("first_seen_at")?
            .unwrap_or_default(),
        last_seen_at: row
            .get::<_, Option<String>>("last_seen_at")?
            .unwrap_or_default(),
        metadata: row
            .get::<_, Option<String>>("metadata")?
            .unwrap_or_default(),
        credential_name: row
            .get::<_, Option<String>>("credential_name")?
            .unwrap_or_default(),
    })
}

fn column_names(statement: &Statement<'_>) -> Vec<String> {
    statement
        .column_names()
        .into_iter()
        .map(ToOwned::to_owned)
        .collect()
}

fn row_to_json(row: &rusqlite::Row<'_>, names: &[String]) -> crate::ImResult<Value> {
    let mut object = serde_json::Map::new();
    for (index, name) in names.iter().enumerate() {
        object.insert(
            name.clone(),
            value_ref_to_json(row.get_ref(index).map_err(super::local_state_unavailable)?),
        );
    }
    Ok(Value::Object(object))
}

fn value_ref_to_json(value: rusqlite::types::ValueRef<'_>) -> Value {
    match value {
        rusqlite::types::ValueRef::Null => Value::Null,
        rusqlite::types::ValueRef::Integer(value) => serde_json::json!(value),
        rusqlite::types::ValueRef::Real(value) => serde_json::Number::from_f64(value)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        rusqlite::types::ValueRef::Text(value) => {
            Value::String(String::from_utf8_lossy(value).into_owned())
        }
        rusqlite::types::ValueRef::Blob(value) => {
            Value::String(String::from_utf8_lossy(value).into_owned())
        }
    }
}

fn now_utc() -> String {
    let value = OffsetDateTime::now_utc();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        value.year(),
        u8::from(value.month()),
        value.day(),
        value.hour(),
        value.minute(),
        value.second()
    )
}

fn normalize_owner_did(value: &str) -> String {
    value.trim().to_string()
}

fn normalize_owner_identity_id(value: &str) -> String {
    value.trim().to_string()
}

fn required_owner_identity_id(value: &str) -> crate::ImResult<String> {
    let value = normalize_owner_identity_id(value);
    if value.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("owner_identity_id".to_owned()),
            "owner_identity_id is required",
        ));
    }
    Ok(value)
}

fn normalize_credential_name(value: &str) -> String {
    value.trim().to_string()
}

fn owner_predicate(identity_param: usize) -> String {
    format!("owner_identity_id = ?{identity_param}")
}

fn ensure_expected_rows(
    affected: usize,
    operation: &'static str,
    owner_identity_id: &str,
    key: &str,
) -> crate::ImResult<()> {
    if affected == 0 {
        return Err(crate::ImError::LocalStateUnavailable {
            detail: format!(
                "{operation} affected 0 rows for owner_identity_id={} key={}",
                owner_identity_id.trim(),
                key.trim()
            ),
        });
    }
    Ok(())
}

fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> crate::ImResult<()> {
    if has_column(connection, table, column)? {
        return Ok(());
    }
    connection
        .execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
            [],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

fn has_column(connection: &Connection, table: &str, column: &str) -> crate::ImResult<bool> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(super::local_state_unavailable)?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(super::local_state_unavailable)?;
    for row in rows {
        if row.map_err(super::local_state_unavailable)? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn normalize_optional_string(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn normalize_optional_bool(value: Option<bool>) -> Option<i64> {
    value.map(bool_to_int)
}

fn normalize_metadata(value: &str) -> Option<String> {
    normalize_optional_string(value)
}

fn default_bool_value(value: Option<bool>) -> i64 {
    value.map(bool_to_int).unwrap_or(0)
}

fn bool_to_int(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}

fn default_string(value: String, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value
    }
}

fn relationship_event_id(record: &RelationshipEventRecord, now: &str) -> String {
    let nanos = OffsetDateTime::now_utc().unix_timestamp_nanos();
    format!(
        "{}:{}:{}:{}:{}",
        record.owner_did.trim(),
        record.target_did.trim(),
        record.event_type.trim(),
        now,
        nanos
    )
}

fn optional_string(value: &str) -> Option<String> {
    normalize_optional_string(value)
}

fn optional_json_string(value: &str) -> Value {
    optional_string(value)
        .map(Value::String)
        .unwrap_or(Value::Null)
}

fn metadata_object(value: &str) -> serde_json::Map<String, Value> {
    let Ok(Value::Object(object)) = serde_json::from_str(value.trim()) else {
        return serde_json::Map::new();
    };
    object
}

fn metadata_string(object: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    match object.get(key) {
        Some(Value::String(value)) => optional_string(value),
        Some(Value::Number(value)) => Some(value.to_string()),
        Some(Value::Bool(value)) => Some(value.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contacts_upsert_rebinds_current_handle_and_preserves_history() {
        let mut db = Connection::open_in_memory().unwrap();
        ensure_schema(&db).unwrap();

        upsert_contact(
            &mut db,
            ContactRecord {
                owner_identity_id: "owner-id".to_string(),
                owner_did: "did:owner".to_string(),
                did: "did:peer-old".to_string(),
                handle: "alice".to_string(),
                source_type: "listener.direct_incoming".to_string(),
                credential_name: "default".to_string(),
                ..ContactRecord::default()
            },
        )
        .unwrap();
        upsert_contact(
            &mut db,
            ContactRecord {
                owner_identity_id: "owner-id".to_string(),
                owner_did: "did:owner".to_string(),
                did: "did:peer-new".to_string(),
                handle: "alice".to_string(),
                source_type: "listener.direct_incoming".to_string(),
                credential_name: "default".to_string(),
                ..ContactRecord::default()
            },
        )
        .unwrap();

        let current = get_current_contact_by_handle(&db, "owner-id", "did:owner", "alice").unwrap();
        assert_eq!(current.did, "did:peer-new");
        let old_contact = get_contact_by_did_json_for_owner_identity(
            &db,
            "owner-id",
            "did:owner",
            "did:peer-old",
        )
        .unwrap();
        assert!(old_contact["handle"].is_null());
        assert_eq!(
            resolve_contact_handle_by_did_for_owner_identity(
                &db,
                "owner-id",
                "did:owner",
                "did:peer-old"
            )
            .unwrap(),
            "alice"
        );
        assert_eq!(
            list_dids_by_handle_for_owner_identity(&db, "owner-id", "did:owner", "alice").unwrap(),
            vec!["did:peer-new".to_string(), "did:peer-old".to_string()]
        );
    }

    #[test]
    fn contacts_owner_identity_queries_ignore_owner_did_snapshot_without_legacy_fallback() {
        let mut db = Connection::open_in_memory().unwrap();
        ensure_schema(&db).unwrap();

        upsert_contact(
            &mut db,
            ContactRecord {
                owner_identity_id: "default".to_string(),
                owner_did: "did:owner-current".to_string(),
                did: "did:peer-identity".to_string(),
                handle: "alice".to_string(),
                credential_name: "default".to_string(),
                ..ContactRecord::default()
            },
        )
        .unwrap();
        upsert_contact(
            &mut db,
            ContactRecord {
                owner_identity_id: "default".to_string(),
                owner_did: "did:owner-legacy".to_string(),
                did: "did:peer-legacy".to_string(),
                handle: "bob".to_string(),
                credential_name: "legacy".to_string(),
                ..ContactRecord::default()
            },
        )
        .unwrap();
        upsert_contact(
            &mut db,
            ContactRecord {
                owner_identity_id: "other".to_string(),
                owner_did: "did:owner-legacy".to_string(),
                did: "did:peer-other".to_string(),
                handle: "carol".to_string(),
                credential_name: "other".to_string(),
                ..ContactRecord::default()
            },
        )
        .unwrap();

        let identity_value: String = db
            .query_row(
                "SELECT owner_identity_id FROM contacts WHERE did = 'did:peer-identity'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(identity_value, "default");

        let contact = get_contact_by_did_json_for_owner_identity(
            &db,
            "default",
            "did:owner-legacy",
            "did:peer-identity",
        )
        .unwrap();
        assert_eq!(contact["did"], "did:peer-identity");

        let same_identity_did_snapshot = get_current_contact_by_handle_json_for_owner_identity(
            &db,
            "default",
            "did:owner-current",
            "bob",
        )
        .unwrap();
        assert_eq!(same_identity_did_snapshot["did"], "did:peer-legacy");

        assert!(get_current_contact_by_handle_json_for_owner_identity(
            &db,
            "default",
            "did:owner-legacy",
            "carol",
        )
        .is_err());
    }

    #[test]
    fn contacts_upsert_same_identity_did_after_owner_did_change_updates_one_row() {
        let mut db = Connection::open_in_memory().unwrap();
        ensure_schema(&db).unwrap();

        upsert_contact(
            &mut db,
            ContactRecord {
                owner_identity_id: "alice-id".to_string(),
                owner_did: "did:owner:old".to_string(),
                did: "did:peer".to_string(),
                handle: "alice".to_string(),
                relationship: "friend".to_string(),
                note: "keep this".to_string(),
                credential_name: "alice".to_string(),
                ..ContactRecord::default()
            },
        )
        .unwrap();
        upsert_contact(
            &mut db,
            ContactRecord {
                owner_identity_id: "alice-id".to_string(),
                owner_did: "did:owner:new".to_string(),
                did: "did:peer".to_string(),
                handle: "alice".to_string(),
                name: "Alice Peer".to_string(),
                credential_name: "alice".to_string(),
                ..ContactRecord::default()
            },
        )
        .unwrap();

        let (count, owner_did, name, relationship, note): (i64, String, String, String, String) =
            db.query_row(
                r#"
SELECT COUNT(*), MAX(owner_did), MAX(name), MAX(relationship), MAX(note)
FROM contacts
WHERE owner_identity_id = 'alice-id' AND did = 'did:peer'"#,
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();

        assert_eq!(count, 1);
        assert_eq!(owner_did, "did:owner:new");
        assert_eq!(name, "Alice Peer");
        assert_eq!(relationship, "friend");
        assert_eq!(note, "keep this");
    }

    #[test]
    fn contact_current_handle_uniqueness_is_scoped_by_owner_identity() {
        let mut db = Connection::open_in_memory().unwrap();
        ensure_schema(&db).unwrap();

        for (owner_identity_id, owner_did, peer_did) in [
            ("alice-id", "did:owner:alice", "did:peer:alice"),
            ("bob-id", "did:owner:bob", "did:peer:bob"),
        ] {
            upsert_contact(
                &mut db,
                ContactRecord {
                    owner_identity_id: owner_identity_id.to_string(),
                    owner_did: owner_did.to_string(),
                    did: peer_did.to_string(),
                    handle: "shared".to_string(),
                    credential_name: owner_identity_id.to_string(),
                    ..ContactRecord::default()
                },
            )
            .unwrap();
        }

        let alice_current =
            get_current_contact_by_handle(&db, "alice-id", "did:owner:alice", "shared").unwrap();
        let bob_current =
            get_current_contact_by_handle(&db, "bob-id", "did:owner:bob", "shared").unwrap();

        assert_eq!(alice_current.did, "did:peer:alice");
        assert_eq!(bob_current.did, "did:peer:bob");
        for owner_identity_id in ["alice-id", "bob-id"] {
            let current_count: i64 = db
                .query_row(
                    r#"
SELECT COUNT(*)
FROM contact_handle_bindings
WHERE owner_identity_id = ?1 AND handle = 'shared' AND is_current = 1"#,
                    [owner_identity_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(current_count, 1);
        }
    }

    #[test]
    fn relationship_event_ids_are_owner_identity_scoped() {
        let db = Connection::open_in_memory().unwrap();
        ensure_schema(&db).unwrap();

        for (owner_identity_id, owner_did, status) in [
            ("alice-id", "did:owner:alice", "applied"),
            ("bob-id", "did:owner:bob", "pending"),
        ] {
            append_relationship_event(
                &db,
                RelationshipEventRecord {
                    event_id: "evt-shared".to_string(),
                    owner_identity_id: owner_identity_id.to_string(),
                    owner_did: owner_did.to_string(),
                    target_did: "did:peer".to_string(),
                    target_handle: "peer".to_string(),
                    event_type: "followed".to_string(),
                    status: status.to_string(),
                    credential_name: owner_identity_id.to_string(),
                    ..RelationshipEventRecord::default()
                },
            )
            .unwrap();
        }

        let total: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM relationship_events WHERE event_id = 'evt-shared'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let alice_status: String = db
            .query_row(
                r#"
SELECT status
FROM relationship_events
WHERE owner_identity_id = 'alice-id' AND event_id = 'evt-shared'"#,
                [],
                |row| row.get(0),
            )
            .unwrap();
        let bob_status: String = db
            .query_row(
                r#"
SELECT status
FROM relationship_events
WHERE owner_identity_id = 'bob-id' AND event_id = 'evt-shared'"#,
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(total, 2);
        assert_eq!(alice_status, "applied");
        assert_eq!(bob_status, "pending");
    }

    #[test]
    fn contacts_list_dids_by_handle_falls_back_to_contacts_without_history_bindings() {
        let db = Connection::open_in_memory().unwrap();
        ensure_schema(&db).unwrap();
        db.execute(
            r#"
INSERT INTO contacts
    (owner_identity_id, owner_did, did, handle, first_seen_at, last_seen_at, metadata)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"#,
            (
                "owner-id",
                "did:owner",
                "did:peer",
                "alice",
                "2026-01-01T00:00:00Z",
                "2026-01-01T00:00:00Z",
                r#"{"source":"seed"}"#,
            ),
        )
        .unwrap();

        assert_eq!(
            list_dids_by_handle_for_owner_identity(&db, "owner-id", "did:owner", "alice").unwrap(),
            vec!["did:peer".to_string()]
        );
    }

    #[test]
    fn contacts_lists_peer_dids_for_message_history_recovery() {
        let db = Connection::open_in_memory().unwrap();
        ensure_schema(&db).unwrap();
        db.execute(
            r#"
INSERT INTO contacts
    (owner_identity_id, owner_did, did, handle, messaged, first_seen_at, last_seen_at, credential_name)
VALUES
    ('alice-id', 'did:owner', 'did:peer-seen', 'seen', 0, '2026-05-21T00:00:00Z', '2026-05-21T00:00:02Z', 'alice'),
    ('alice-id', 'did:owner', 'did:peer-messaged', 'messaged', 1, '2026-05-21T00:00:00Z', '2026-05-21T00:00:01Z', 'alice'),
    ('alice-id', 'did:owner', 'did:owner', 'self', 1, '2026-05-21T00:00:00Z', '2026-05-21T00:00:04Z', 'alice'),
    ('other-id', 'did:other', 'did:peer-other', 'other', 1, '2026-05-21T00:00:00Z', '2026-05-21T00:00:05Z', 'other')"#,
            [],
        )
        .unwrap();

        let dids = list_contact_dids_for_message_history_recovery(&db, "alice-id", "did:owner", 10)
            .unwrap();

        assert_eq!(dids, vec!["did:peer-messaged", "did:peer-seen"]);
    }
}
