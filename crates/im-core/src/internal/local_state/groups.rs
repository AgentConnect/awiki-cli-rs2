#[cfg(feature = "sqlite")]
use rusqlite::OptionalExtension;
#[cfg(feature = "sqlite")]
use time::OffsetDateTime;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct GroupRecord {
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) group_id: String,
    pub(crate) group_did: String,
    pub(crate) name: String,
    pub(crate) group_mode: String,
    pub(crate) slug: String,
    pub(crate) description: String,
    pub(crate) goal: String,
    pub(crate) rules: String,
    pub(crate) message_prompt: String,
    pub(crate) doc_url: String,
    pub(crate) group_owner_did: String,
    pub(crate) group_owner_handle: String,
    pub(crate) my_role: String,
    pub(crate) membership_status: String,
    pub(crate) join_enabled: Option<bool>,
    pub(crate) join_code: String,
    pub(crate) join_code_expires_at: String,
    pub(crate) member_count: Option<i64>,
    pub(crate) last_synced_seq: Option<i64>,
    pub(crate) last_read_seq: Option<i64>,
    pub(crate) last_message_at: String,
    pub(crate) remote_created_at: String,
    pub(crate) remote_updated_at: String,
    pub(crate) stored_at: String,
    pub(crate) metadata: String,
    pub(crate) credential_name: String,
}

#[cfg(all(test, feature = "sqlite"))]
mod handle_member_identity_tests {
    use super::*;

    #[test]
    fn handle_rebind_reuses_local_id_and_did_only_remains_distinct() {
        let mut db = rusqlite::Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let handle_member = |did: &str, generation: &str| GroupMemberRecord {
            member_did: did.to_owned(),
            member_handle: "alice.example.com".to_owned(),
            anchor_kind: "handle".to_owned(),
            anchor_value: "alice.example.com".to_owned(),
            handle_binding_generation: generation.to_owned(),
            ..GroupMemberRecord::default()
        };
        replace_group_members(
            &mut db,
            "owner-id",
            "did:owner",
            "did:group",
            &[handle_member("did:alice:old", "1")],
            "owner",
        )
        .unwrap();
        let first_id: String = db
            .query_row("SELECT user_id FROM group_members", [], |row| row.get(0))
            .unwrap();
        let first_membership_id: String = db
            .query_row("SELECT membership_id FROM group_members", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(first_id.starts_with("peer_"));

        replace_group_members(
            &mut db,
            "owner-id",
            "did:owner",
            "did:group",
            &[
                handle_member("did:alice:new", "2"),
                GroupMemberRecord {
                    member_did: "did:alice:new".to_owned(),
                    anchor_kind: "did".to_owned(),
                    anchor_value: "did:alice:new".to_owned(),
                    ..GroupMemberRecord::default()
                },
            ],
            "owner",
        )
        .unwrap();
        let rows = list_cached_group_members_for_owner_identity(
            &db,
            "owner-id",
            "did:owner",
            "did:group",
            10,
        )
        .unwrap();
        assert_eq!(rows.len(), 2);
        let handle = rows
            .iter()
            .find(|row| row["anchor_kind"] == "handle")
            .unwrap();
        let did = rows.iter().find(|row| row["anchor_kind"] == "did").unwrap();
        assert_eq!(handle["user_id"], first_id);
        assert_eq!(handle["member_did"], "did:alice:new");
        assert_ne!(handle["user_id"], did["user_id"]);
        let rebound_membership_id: String = db
            .query_row(
                "SELECT membership_id FROM group_members WHERE anchor_kind = 'handle'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(rebound_membership_id, first_membership_id);

        let rollback = replace_group_members(
            &mut db,
            "owner-id",
            "did:owner",
            "did:group",
            &[handle_member("did:alice:rollback", "1")],
            "owner",
        );
        assert!(rollback.is_err());
        let preserved: (String, String) = db
            .query_row(
                "SELECT user_id, member_did FROM group_members WHERE anchor_kind = 'handle'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(preserved, (first_id, "did:alice:new".to_owned()));
    }

    #[test]
    fn fallback_membership_id_ignores_binding_generation_but_honors_rejoin_epoch() {
        let first = fallback_membership_id(
            "did:wba:awiki.info:groups:g1:e1_group",
            "handle",
            "alice.awiki.info",
            None,
        );
        let rebound = fallback_membership_id(
            "did:wba:awiki.info:groups:g1:e1_group",
            "handle",
            "alice.awiki.info",
            None,
        );
        let rejoined = fallback_membership_id(
            "did:wba:awiki.info:groups:g1:e1_group",
            "handle",
            "alice.awiki.info",
            Some("join-event-2"),
        );
        assert_eq!(first, rebound);
        assert_ne!(first, rejoined);
    }

    #[test]
    fn active_group_projection_ensures_one_canonical_empty_conversation() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let record = GroupRecord {
            owner_identity_id: "owner-id".to_owned(),
            owner_did: "did:example:owner".to_owned(),
            group_id: "group-storage-id".to_owned(),
            group_did: "did:example:canonical-group".to_owned(),
            name: "Empty Group".to_owned(),
            membership_status: "active".to_owned(),
            stored_at: "2026-07-15T00:00:00Z".to_owned(),
            ..GroupRecord::default()
        };

        upsert_group(&db, record.clone()).unwrap();
        upsert_group(&db, record).unwrap();

        assert_eq!(
            db.query_row(
                r#"SELECT COUNT(*) FROM conversation_registry
WHERE owner_identity_id = 'owner-id'
  AND conversation_id = 'group:did:example:canonical-group'
  AND canonical_group_did = 'did:example:canonical-group'
  AND lifecycle_state = 'active' AND resolution_state = 'resolved'"#,
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM messages", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            0
        );
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct GroupMemberRecord {
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) group_id: String,
    pub(crate) user_id: String,
    pub(crate) membership_id: String,
    pub(crate) peer_persona_id: String,
    pub(crate) member_did: String,
    pub(crate) member_credential_did: String,
    pub(crate) member_handle: String,
    pub(crate) anchor_kind: String,
    pub(crate) anchor_value: String,
    pub(crate) handle_binding_generation: String,
    pub(crate) membership_epoch: String,
    pub(crate) profile_url: String,
    pub(crate) role: String,
    pub(crate) status: String,
    pub(crate) joined_at: String,
    pub(crate) sent_message_count: Option<i64>,
    pub(crate) last_synced_at: String,
    pub(crate) metadata: String,
    pub(crate) credential_name: String,
}

#[cfg(feature = "sqlite")]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct GroupE2eeSummaryRecord {
    pub(crate) record: GroupRecord,
    pub(crate) epoch: Option<String>,
    pub(crate) group_state_version: Option<String>,
}

#[cfg(feature = "sqlite")]
pub(crate) fn upsert_group(
    connection: &rusqlite::Connection,
    record: GroupRecord,
) -> crate::ImResult<()> {
    let owner_did = normalize(&record.owner_did);
    let owner_identity_id = required_owner_identity_id(&record.owner_identity_id)?;
    let group_id = normalize(&record.group_id);
    if owner_did.is_empty() || group_id.is_empty() {
        return Err(crate::ImError::invalid_input(
            None,
            "owner_did and group_id are required",
        ));
    }
    let stored_at = default_string(record.stored_at.clone(), &now_utc());
    let metadata = preserve_group_security_metadata(
        connection,
        &owner_identity_id,
        &group_id,
        &record.metadata,
    )?;
    connection
        .execute(
            r#"
INSERT INTO groups
    (owner_identity_id, owner_did, group_id, group_did, name, group_mode, slug, description, goal, rules, message_prompt,
     doc_url, group_owner_did, group_owner_handle, my_role, membership_status, join_enabled, join_code,
     join_code_expires_at, member_count, last_synced_seq, last_read_seq, last_message_at,
     remote_created_at, remote_updated_at, stored_at, metadata, credential_name)
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
    membership_status = CASE
        WHEN ?29 IS NULL THEN groups.membership_status
        ELSE excluded.membership_status
    END,
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
            rusqlite::params![
                owner_identity_id,
                owner_did,
                group_id,
                optional_string(&record.group_did),
                optional_string(&record.name),
                default_string(record.group_mode.clone(), "general"),
                optional_string(&record.slug),
                optional_string(&record.description),
                optional_string(&record.goal),
                optional_string(&record.rules),
                optional_string(&record.message_prompt),
                optional_string(&record.doc_url),
                optional_string(&record.group_owner_did),
                optional_string(&record.group_owner_handle),
                optional_string(&record.my_role),
                optional_string(&record.membership_status).unwrap_or_else(|| "active".to_owned()),
                optional_bool(record.join_enabled),
                optional_string(&record.join_code),
                optional_string(&record.join_code_expires_at),
                record.member_count,
                record.last_synced_seq,
                record.last_read_seq,
                optional_string(&record.last_message_at),
                optional_string(&record.remote_created_at),
                optional_string(&record.remote_updated_at),
                stored_at,
                optional_string(&metadata),
                normalize(&record.credential_name),
                optional_string(&record.membership_status),
            ],
        )
        .map_err(super::local_state_unavailable)?;
    ensure_active_group_conversation(connection, &owner_identity_id, &group_id)?;
    Ok(())
}

#[cfg(feature = "sqlite")]
fn ensure_active_group_conversation(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    group_id: &str,
) -> crate::ImResult<()> {
    let projection = connection
        .query_row(
            r#"SELECT owner_did, COALESCE(NULLIF(TRIM(group_did), ''), ''),
                      COALESCE(NULLIF(TRIM(membership_status), ''), 'active'),
                      COALESCE(NULLIF(TRIM(last_message_at), ''), stored_at)
FROM groups WHERE owner_identity_id = ?1 AND group_id = ?2"#,
            (owner_identity_id, group_id),
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()
        .map_err(super::local_state_unavailable)?;
    let Some((owner_did, group_did, membership_status, activity_at)) = projection else {
        return Ok(());
    };
    if matches!(
        membership_status.trim().to_ascii_lowercase().as_str(),
        "left" | "removed" | "inactive" | "non_member"
    ) || crate::ids::Did::parse(&group_did).is_err()
    {
        return Ok(());
    }
    let conversation_id = super::owner_scope::group_conversation_id(&group_did);
    super::conversation_registry::ensure(
        connection,
        &super::conversation_registry::ConversationRegistryRecord {
            owner_identity_id: owner_identity_id.to_owned(),
            owner_did,
            conversation_id,
            thread_kind: "group".to_owned(),
            thread_id: group_did,
            activity_at,
        },
    )
}

#[cfg(feature = "sqlite")]
fn preserve_group_security_metadata(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    group_id: &str,
    incoming: &str,
) -> crate::ImResult<String> {
    let Ok(mut incoming_value) = serde_json::from_str::<serde_json::Value>(incoming) else {
        return Ok(incoming.to_owned());
    };
    let Some(incoming_object) = incoming_value.as_object_mut() else {
        return Ok(incoming.to_owned());
    };
    let existing: Option<String> = connection
        .query_row(
            "SELECT metadata FROM groups WHERE owner_identity_id=?1 AND group_id=?2",
            rusqlite::params![owner_identity_id, group_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(super::local_state_unavailable)?;
    let Some(existing_object) = existing
        .as_deref()
        .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
        .and_then(|value| value.as_object().cloned())
    else {
        return Ok(incoming.to_owned());
    };

    for key in ["message_security_profile", "required_security_profile"] {
        if matches!(
            incoming_object.get(key),
            None | Some(serde_json::Value::Null)
        ) {
            if let Some(value) = existing_object
                .get(key)
                .filter(|value| value.as_str().is_some())
            {
                incoming_object.insert(key.to_owned(), value.clone());
            }
        }
    }
    let existing_policy_profile = existing_object
        .get("group_policy")
        .and_then(|policy| policy.get("message_security_profile"))
        .filter(|value| value.as_str().is_some())
        .cloned();
    if let Some(existing_policy_profile) = existing_policy_profile {
        match incoming_object.get_mut("group_policy") {
            Some(serde_json::Value::Object(policy)) => {
                if matches!(
                    policy.get("message_security_profile"),
                    None | Some(serde_json::Value::Null)
                ) {
                    policy.insert(
                        "message_security_profile".to_owned(),
                        existing_policy_profile,
                    );
                }
            }
            None | Some(serde_json::Value::Null) => {
                incoming_object.insert(
                    "group_policy".to_owned(),
                    serde_json::json!({
                        "message_security_profile": existing_policy_profile
                    }),
                );
            }
            Some(_) => {}
        }
    }
    serde_json::to_string(&incoming_value).map_err(|error| crate::ImError::Internal {
        message: format!("failed to preserve group security metadata: {error}"),
    })
}

#[cfg(feature = "sqlite")]
pub(crate) fn upsert_group_e2ee_summary(
    connection: &rusqlite::Connection,
    summary: GroupE2eeSummaryRecord,
) -> crate::ImResult<()> {
    let owner_did = normalize(&summary.record.owner_did);
    let owner_identity_id = required_owner_identity_id(&summary.record.owner_identity_id)?;
    let group_id = normalize(&summary.record.group_id);
    if owner_did.is_empty() || group_id.is_empty() {
        return Err(crate::ImError::invalid_input(
            None,
            "owner_did and group_id are required",
        ));
    }
    if let Some(existing) = get_group_snapshot_for_owner_identity(
        connection,
        &owner_identity_id,
        &owner_did,
        &group_id,
    )? {
        if !should_apply_group_e2ee_summary(
            existing.get("metadata"),
            summary.epoch.as_deref(),
            summary.group_state_version.as_deref(),
        ) {
            return Ok(());
        }
    }
    upsert_group(connection, summary.record)
}

#[cfg(feature = "sqlite")]
pub(crate) fn replace_group_members(
    connection: &mut rusqlite::Connection,
    owner_identity_id: &str,
    owner_did: &str,
    group_id: &str,
    members: &[GroupMemberRecord],
    credential_name: &str,
) -> crate::ImResult<()> {
    let owner_did = normalize(owner_did);
    let owner_identity_id = required_owner_identity_id(owner_identity_id)?;
    let group_id = normalize(group_id);
    if owner_did.is_empty() || group_id.is_empty() {
        return Err(crate::ImError::invalid_input(
            None,
            "owner_did and group_id are required",
        ));
    }
    let transaction = connection
        .transaction()
        .map_err(super::local_state_unavailable)?;
    let existing_ids = {
        let mut statement = transaction
            .prepare(
                "SELECT anchor_kind, anchor_value, user_id, COALESCE(member_did, ''), COALESCE(handle_binding_generation, ''), COALESCE(membership_id, '') FROM group_members WHERE owner_identity_id = ?1 AND group_id = ?2",
            )
            .map_err(super::local_state_unavailable)?;
        let rows = statement
            .query_map(
                rusqlite::params![owner_identity_id.as_str(), group_id.as_str()],
                |row| {
                    Ok((
                        (row.get::<_, String>(0)?, row.get::<_, String>(1)?),
                        (
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, String>(5)?,
                        ),
                    ))
                },
            )
            .map_err(super::local_state_unavailable)?;
        let mut values = std::collections::BTreeMap::new();
        for row in rows {
            let (anchor, user_id) = row.map_err(super::local_state_unavailable)?;
            values.insert(anchor, user_id);
        }
        values
    };
    transaction
        .execute(
            "DELETE FROM group_members WHERE owner_identity_id = ?1 AND group_id = ?2",
            rusqlite::params![owner_identity_id.as_str(), group_id.as_str()],
        )
        .map_err(super::local_state_unavailable)?;
    let now = now_utc();
    let canonical_group_did = transaction
        .query_row(
            r#"SELECT COALESCE(NULLIF(TRIM(group_did), ''), group_id)
FROM groups WHERE owner_identity_id = ?1 AND group_id = ?2"#,
            rusqlite::params![owner_identity_id.as_str(), group_id.as_str()],
            |row| row.get::<_, String>(0),
        )
        .unwrap_or_else(|_| group_id.clone());
    {
        let mut seen_anchors = std::collections::BTreeSet::new();
        let mut statement = transaction
            .prepare(
                r#"
INSERT INTO group_members
    (owner_identity_id, owner_did, group_id, user_id, membership_id, peer_persona_id,
     member_did, member_credential_did, member_handle, anchor_kind, anchor_value,
     handle_binding_generation, membership_epoch, profile_url, role, status, joined_at,
     sent_message_count, last_synced_at, metadata, credential_name)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)"#,
            )
            .map_err(super::local_state_unavailable)?;
        for member in members {
            let anchor_kind = default_string(normalize(&member.anchor_kind), "did");
            let anchor_value = default_string(
                normalize(&member.anchor_value),
                &normalize(&member.member_did),
            );
            if !matches!(anchor_kind.as_str(), "did" | "handle") || anchor_value.is_empty() {
                continue;
            }
            if !seen_anchors.insert((anchor_kind.clone(), anchor_value.clone())) {
                return Err(crate::ImError::invalid_input(
                    Some("group_members".to_owned()),
                    "group member snapshot contains a duplicate membership anchor",
                ));
            }
            let existing = existing_ids.get(&(anchor_kind.clone(), anchor_value.clone()));
            if anchor_kind == "handle" {
                validate_handle_generation_transition(
                    existing.map(|(_, did, generation, _)| (did.as_str(), generation.as_str())),
                    &member.member_did,
                    &member.handle_binding_generation,
                )?;
            }
            let membership_id = optional_string(&member.membership_id)
                .or_else(|| existing.and_then(|(_, _, _, value)| optional_string(value)))
                .unwrap_or_else(|| {
                    fallback_membership_id(
                        &canonical_group_did,
                        &anchor_kind,
                        &anchor_value,
                        optional_string(&member.membership_epoch).as_deref(),
                    )
                });
            let user_id = existing
                .map(|(user_id, _, _, _)| user_id.clone())
                .or_else(|| optional_string(&member.user_id))
                .unwrap_or_else(|| compatibility_user_id_for_membership(&membership_id));
            let last_synced_at = default_string(member.last_synced_at.clone(), &now);
            statement
                .execute(rusqlite::params![
                    owner_identity_id.as_str(),
                    owner_did.as_str(),
                    group_id.as_str(),
                    user_id,
                    membership_id,
                    optional_string(&member.peer_persona_id),
                    optional_string(&member.member_did),
                    optional_string(&member.member_credential_did)
                        .or_else(|| optional_string(&member.member_did)),
                    optional_string(&member.member_handle),
                    anchor_kind,
                    anchor_value,
                    optional_string(&member.handle_binding_generation),
                    optional_string(&member.membership_epoch),
                    optional_string(&member.profile_url),
                    optional_string(&member.role),
                    default_string(member.status.clone(), "active"),
                    optional_string(&member.joined_at),
                    member.sent_message_count.or(Some(0)),
                    last_synced_at,
                    optional_string(&member.metadata),
                    normalize(&default_string(
                        member.credential_name.clone(),
                        credential_name,
                    )),
                ])
                .map_err(super::local_state_unavailable)?;
        }
    }
    transaction
        .commit()
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

#[cfg(feature = "sqlite")]
fn compatibility_user_id_for_membership(membership_id: &str) -> String {
    membership_id
        .strip_prefix("membership:v1:")
        .map(|value| format!("peer_{value}"))
        .unwrap_or_else(|| membership_id.to_owned())
}

#[cfg(feature = "sqlite")]
pub(crate) fn fallback_membership_id(
    canonical_group_did: &str,
    anchor_kind: &str,
    normalized_anchor_value: &str,
    membership_epoch: Option<&str>,
) -> String {
    use sha2::{Digest as _, Sha256};
    let input = format!(
        "membership:v1\ngroup:{}\nanchor-kind:{}\nanchor:{}\nepoch:{}",
        canonical_group_did.trim(),
        anchor_kind.trim().to_ascii_lowercase(),
        normalized_anchor_value.trim().to_ascii_lowercase(),
        membership_epoch.unwrap_or_default().trim(),
    );
    let digest = Sha256::digest(input.as_bytes());
    let mut value = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(value, "{byte:02x}");
    }
    format!("membership:v1:{value}")
}

#[cfg(feature = "sqlite")]
fn validate_handle_generation_transition(
    existing: Option<(&str, &str)>,
    next_did: &str,
    next_generation: &str,
) -> crate::ImResult<()> {
    let next = canonical_generation(next_generation).ok_or_else(|| {
        crate::ImError::invalid_input(
            Some("handle_binding_generation".to_owned()),
            "Handle-backed member requires a canonical positive decimal generation",
        )
    })?;
    let Some((existing_did, existing_generation)) = existing else {
        return Ok(());
    };
    let Some(existing_generation) = canonical_generation(existing_generation) else {
        return Err(crate::ImError::LocalStateUnavailable {
            detail: "stored Handle-backed member has invalid binding generation".to_owned(),
        });
    };
    match next
        .len()
        .cmp(&existing_generation.len())
        .then_with(|| next.cmp(existing_generation))
    {
        std::cmp::Ordering::Less => Err(crate::ImError::invalid_input(
            Some("handle_binding_generation".to_owned()),
            "Handle binding generation rollback rejected",
        )),
        std::cmp::Ordering::Equal if next_did.trim() != existing_did.trim() => {
            Err(crate::ImError::invalid_input(
                Some("handle_binding_generation".to_owned()),
                "Handle DID changed without a newer binding generation",
            ))
        }
        _ => Ok(()),
    }
}

#[cfg(feature = "sqlite")]
fn canonical_generation(value: &str) -> Option<&str> {
    (!value.is_empty()
        && value != "0"
        && !value.starts_with('0')
        && value.bytes().all(|byte| byte.is_ascii_digit()))
    .then_some(value)
}

#[cfg(feature = "sqlite")]
pub(crate) fn mark_group_left(
    connection: &mut rusqlite::Connection,
    owner_identity_id: &str,
    owner_did: &str,
    group_id: &str,
    group_did: &str,
    credential_name: &str,
) -> crate::ImResult<()> {
    let owner_did = normalize(owner_did);
    let group_id = normalize(group_id);
    if owner_did.is_empty() || group_id.is_empty() {
        return Err(crate::ImError::invalid_input(
            None,
            "owner_did and group_id are required",
        ));
    }
    let now = now_utc();
    let credential_name = normalize(credential_name);
    let owner_identity_id = required_owner_identity_id(owner_identity_id)?;
    let transaction = connection
        .transaction()
        .map_err(super::local_state_unavailable)?;
    transaction
        .execute(
            r#"
INSERT INTO groups
    (owner_identity_id, owner_did, group_id, group_did, group_mode, my_role, membership_status, stored_at,
     credential_name)
VALUES (?1, ?2, ?3, ?4, 'general', NULL, 'left', ?5, ?6)
ON CONFLICT(owner_identity_id, group_id)
DO UPDATE SET
    owner_did = excluded.owner_did,
    group_did = COALESCE(excluded.group_did, groups.group_did),
    my_role = NULL,
    membership_status = 'left',
    stored_at = excluded.stored_at,
    credential_name = COALESCE(excluded.credential_name, groups.credential_name)"#,
            rusqlite::params![
                owner_identity_id.as_str(),
                owner_did.as_str(),
                group_id.as_str(),
                optional_string(group_did),
                now.as_str(),
                credential_name,
            ],
        )
        .map_err(super::local_state_unavailable)?;
    transaction
        .execute(
            "DELETE FROM group_members WHERE owner_identity_id = ?1 AND group_id = ?2",
            rusqlite::params![owner_identity_id.as_str(), group_id.as_str()],
        )
        .map_err(super::local_state_unavailable)?;
    super::conversation_registry::deactivate(
        &transaction,
        &owner_identity_id,
        &format!("group:{group_id}"),
    )?;
    if !group_did.trim().is_empty() {
        super::conversation_registry::deactivate(
            &transaction,
            &owner_identity_id,
            &format!("group:{}", group_did.trim()),
        )?;
    }
    transaction
        .commit()
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

#[cfg(feature = "sqlite")]
pub(crate) fn get_group_snapshot_for_owner_identity(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    _owner_did: &str,
    group_id: &str,
) -> crate::ImResult<Option<serde_json::Value>> {
    let group_id = group_id.trim();
    if group_id.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("group_id".to_string()),
            "group_id is required",
        ));
    }
    let statement = format!(
        r#"
SELECT *
FROM groups
WHERE {} AND (group_id = ?2 OR group_did = ?2)
LIMIT 1"#,
        owner_predicate("groups")
    );
    let mut statement = connection
        .prepare(&statement)
        .map_err(super::local_state_unavailable)?;
    let names = column_names(&statement);
    let mut rows = statement
        .query(rusqlite::params![
            required_owner_identity_id(owner_identity_id)?,
            group_id,
        ])
        .map_err(super::local_state_unavailable)?;
    let Some(row) = rows.next().map_err(super::local_state_unavailable)? else {
        return Ok(None);
    };
    row_to_json(row, &names).map(Some)
}

#[cfg(feature = "sqlite")]
pub(crate) fn list_cached_group_members_for_owner_identity(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    _owner_did: &str,
    group_id: &str,
    limit: i64,
) -> crate::ImResult<Vec<serde_json::Value>> {
    let group_id = group_id.trim();
    if group_id.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("group_id".to_string()),
            "group_id is required",
        ));
    }
    let limit = if limit <= 0 { 100 } else { limit };
    let statement = format!(
        r#"
SELECT *
FROM group_members
WHERE {} AND group_id IN (
    SELECT ?2
    UNION
    SELECT group_id
    FROM groups
    WHERE {} AND (group_id = ?2 OR group_did = ?2)
)
ORDER BY role ASC, member_handle ASC, member_did ASC
LIMIT ?3"#,
        owner_predicate("group_members"),
        owner_predicate("groups")
    );
    query_rows(
        connection,
        &statement,
        &[
            &required_owner_identity_id(owner_identity_id)?,
            &group_id,
            &limit,
        ],
    )
}

#[cfg(feature = "sqlite")]
pub(crate) fn list_group_messages_for_owner_identity(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    _owner_did: &str,
    group_id: &str,
    limit: i64,
    since_seq: Option<i64>,
) -> crate::ImResult<Vec<serde_json::Value>> {
    let group_id = group_id.trim();
    if group_id.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("group_id".to_string()),
            "group_id is required",
        ));
    }
    let limit = if limit <= 0 { 50 } else { limit };
    if let Some(since_seq) = since_seq {
        let statement = format!(
            r#"
SELECT *
FROM messages
WHERE {} AND (group_did = ?2 OR group_id = ?2)
  AND hydration_state = 'hydrated'
  AND COALESCE(server_seq, 0) > ?3
ORDER BY COALESCE(server_seq, 0) DESC, COALESCE(sent_at, stored_at) DESC
LIMIT ?4"#,
            owner_predicate("messages")
        );
        return query_rows(
            connection,
            &statement,
            &[
                &required_owner_identity_id(owner_identity_id)?,
                &group_id,
                &since_seq,
                &limit,
            ],
        );
    }
    let statement = format!(
        r#"
SELECT *
FROM messages
WHERE {} AND (group_did = ?2 OR group_id = ?2)
  AND hydration_state = 'hydrated'
ORDER BY COALESCE(server_seq, 0) DESC, COALESCE(sent_at, stored_at) DESC
LIMIT ?3"#,
        owner_predicate("messages")
    );
    query_rows(
        connection,
        &statement,
        &[
            &required_owner_identity_id(owner_identity_id)?,
            &group_id,
            &limit,
        ],
    )
}

#[cfg(feature = "sqlite")]
pub(crate) fn list_active_group_refs_for_owner_identity(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    _owner_did: &str,
    limit: i64,
) -> crate::ImResult<Vec<String>> {
    let limit = if limit <= 0 { 50 } else { limit };
    let statement = format!(
        r#"
SELECT COALESCE(NULLIF(TRIM(group_did), ''), group_id) AS group_ref
FROM groups
WHERE {}
  AND TRIM(COALESCE(group_id, '')) <> ''
  AND COALESCE(NULLIF(TRIM(membership_status), ''), 'active') NOT IN ('left', 'removed', 'inactive', 'non_member')
ORDER BY
  CASE WHEN TRIM(COALESCE(last_message_at, '')) = '' THEN 1 ELSE 0 END,
  COALESCE(last_message_at, stored_at) DESC,
  stored_at DESC,
  group_id ASC
LIMIT ?2"#,
        owner_predicate("groups")
    );
    let mut statement = connection
        .prepare(&statement)
        .map_err(super::local_state_unavailable)?;
    let rows = statement
        .query_map(
            rusqlite::params![required_owner_identity_id(owner_identity_id)?, limit],
            |row| row.get::<_, Option<String>>("group_ref"),
        )
        .map_err(super::local_state_unavailable)?;
    let mut result = Vec::new();
    for row in rows {
        let group_ref = row
            .map_err(super::local_state_unavailable)?
            .unwrap_or_default();
        if !group_ref.trim().is_empty() {
            result.push(group_ref);
        }
    }
    Ok(result)
}

#[cfg(feature = "sqlite")]
fn query_rows(
    connection: &rusqlite::Connection,
    statement: &str,
    params: &[&dyn rusqlite::ToSql],
) -> crate::ImResult<Vec<serde_json::Value>> {
    let mut statement = connection
        .prepare(statement)
        .map_err(super::local_state_unavailable)?;
    let names = column_names(&statement);
    let mut rows = statement
        .query(params)
        .map_err(super::local_state_unavailable)?;
    let mut result = Vec::new();
    while let Some(row) = rows.next().map_err(super::local_state_unavailable)? {
        result.push(row_to_json(row, &names)?);
    }
    Ok(result)
}

#[cfg(feature = "sqlite")]
fn column_names(statement: &rusqlite::Statement<'_>) -> Vec<String> {
    statement
        .column_names()
        .into_iter()
        .map(ToOwned::to_owned)
        .collect()
}

#[cfg(feature = "sqlite")]
fn row_to_json(row: &rusqlite::Row<'_>, names: &[String]) -> crate::ImResult<serde_json::Value> {
    let mut object = serde_json::Map::new();
    for (index, name) in names.iter().enumerate() {
        object.insert(
            name.clone(),
            value_ref_to_json(row.get_ref(index).map_err(super::local_state_unavailable)?),
        );
    }
    Ok(serde_json::Value::Object(object))
}

#[cfg(feature = "sqlite")]
fn value_ref_to_json(value: rusqlite::types::ValueRef<'_>) -> serde_json::Value {
    match value {
        rusqlite::types::ValueRef::Null => serde_json::Value::Null,
        rusqlite::types::ValueRef::Integer(value) => serde_json::json!(value),
        rusqlite::types::ValueRef::Real(value) => serde_json::Number::from_f64(value)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        rusqlite::types::ValueRef::Text(value) => {
            serde_json::Value::String(String::from_utf8_lossy(value).into_owned())
        }
        rusqlite::types::ValueRef::Blob(value) => {
            serde_json::Value::String(String::from_utf8_lossy(value).into_owned())
        }
    }
}

#[cfg(feature = "sqlite")]
fn owner_predicate(alias: &str) -> String {
    format!("{alias}.owner_identity_id = ?1")
}

#[cfg(feature = "sqlite")]
fn normalize(value: &str) -> String {
    value.trim().to_string()
}

#[cfg(feature = "sqlite")]
fn normalize_owner_identity_id(value: &str) -> String {
    value.trim().to_string()
}

#[cfg(feature = "sqlite")]
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

#[cfg(feature = "sqlite")]
fn default_string(value: String, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value
    }
}

#[cfg(feature = "sqlite")]
fn optional_string(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

#[cfg(feature = "sqlite")]
fn optional_bool(value: Option<bool>) -> Option<i64> {
    value.map(i64::from)
}

#[cfg(feature = "sqlite")]
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

#[cfg(feature = "sqlite")]
fn should_apply_group_e2ee_summary(
    existing_metadata: Option<&serde_json::Value>,
    next_epoch: Option<&str>,
    next_group_state_version: Option<&str>,
) -> bool {
    let Some(existing_metadata) = existing_metadata.and_then(decode_metadata_value) else {
        return true;
    };
    let existing_epoch = metadata_string(&existing_metadata, &["group_e2ee", "epoch"])
        .as_deref()
        .and_then(parse_u64);
    let next_epoch = next_epoch.and_then(parse_u64);
    match (existing_epoch, next_epoch) {
        (Some(existing), Some(next)) if next < existing => return false,
        (Some(existing), Some(next)) if next > existing => return true,
        _ => {}
    }

    let existing_version = metadata_string(&existing_metadata, &["group_state_version"])
        .or_else(|| metadata_string(&existing_metadata, &["group_e2ee", "group_state_version"]));
    let existing_version = existing_version.as_deref();
    match compare_group_state_versions(next_group_state_version, existing_version) {
        Some(std::cmp::Ordering::Less) => false,
        Some(_) => true,
        None => existing_version.is_none(),
    }
}

#[cfg(feature = "sqlite")]
fn decode_metadata_value(value: &serde_json::Value) -> Option<serde_json::Value> {
    match value {
        serde_json::Value::Object(_) => Some(value.clone()),
        serde_json::Value::String(raw) => serde_json::from_str(raw).ok(),
        _ => None,
    }
}

#[cfg(feature = "sqlite")]
fn metadata_string(value: &serde_json::Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(feature = "sqlite")]
fn compare_group_state_versions(
    next: Option<&str>,
    existing: Option<&str>,
) -> Option<std::cmp::Ordering> {
    let next = next.map(str::trim).filter(|value| !value.is_empty())?;
    let existing = existing.map(str::trim).filter(|value| !value.is_empty())?;
    match (parse_version_ordinal(next), parse_version_ordinal(existing)) {
        (Some(next), Some(existing)) => Some(next.cmp(&existing)),
        _ if next == existing => Some(std::cmp::Ordering::Equal),
        _ => None,
    }
}

#[cfg(feature = "sqlite")]
fn parse_version_ordinal(value: &str) -> Option<u64> {
    parse_u64(value).or_else(|| {
        let digits: String = value
            .chars()
            .rev()
            .take_while(|ch| ch.is_ascii_digit())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        if digits.is_empty() {
            None
        } else {
            digits.parse().ok()
        }
    })
}

#[cfg(feature = "sqlite")]
fn parse_u64(value: &str) -> Option<u64> {
    value.trim().parse().ok()
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn local_state_groups_read_cache_with_owner_identity_scope() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        db.execute(
            r#"
INSERT INTO groups
    (owner_identity_id, owner_did, group_id, group_did, name, stored_at, credential_name)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"#,
            (
                "alice-identity",
                "did:owner",
                "group-key",
                "did:group",
                "Demo Group",
                "2026-05-21T00:00:00Z",
                "alice",
            ),
        )
        .unwrap();
        db.execute(
            r#"
INSERT INTO group_members
    (owner_identity_id, owner_did, group_id, user_id, member_did, member_handle, role, status,
     joined_at, sent_message_count, last_synced_at, credential_name)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, ?10, ?11)"#,
            (
                "alice-identity",
                "did:owner",
                "group-key",
                "did:member",
                "did:member",
                "member.awiki.ai",
                "member",
                "active",
                "2026-05-21T00:00:00Z",
                "2026-05-21T00:00:00Z",
                "alice",
            ),
        )
        .unwrap();
        db.execute(
            r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, thread_id, direction, sender_did, group_id, group_did,
     content_type, content, server_seq, sent_at, stored_at, credential_name)
VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6, ?7, 'text/plain', 'hello', 7, ?8, ?8, ?9)"#,
            (
                "msg-group-1",
                "alice-identity",
                "did:owner",
                "thread:group-key",
                "did:member",
                "group-key",
                "did:group",
                "2026-05-21T00:00:00Z",
                "alice",
            ),
        )
        .unwrap();
        db.execute(
            r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction,
     sender_did, group_id, group_did, content_type, server_seq, hydration_state,
     sent_at, stored_at, credential_name)
VALUES ('msg-group-discovered', 'alice-identity', 'did:owner', 'group:did:group',
        'group:did:group', 0, 'did:member', 'group-key', 'did:group',
        'application/json', 8, 'discovered', '2026-05-21T00:00:01Z',
        '2026-05-21T00:00:01Z', 'alice')"#,
            [],
        )
        .unwrap();

        let snapshot =
            get_group_snapshot_for_owner_identity(&db, "alice-identity", "did:owner", "did:group")
                .unwrap()
                .unwrap();
        assert_eq!(snapshot["name"], "Demo Group");

        let members = list_cached_group_members_for_owner_identity(
            &db,
            "alice-identity",
            "did:owner",
            "did:group",
            10,
        )
        .unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0]["member_did"], "did:member");

        let messages = list_group_messages_for_owner_identity(
            &db,
            "alice-identity",
            "did:owner",
            "did:group",
            10,
            Some(1),
        )
        .unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["msg_id"], "msg-group-1");
        let messages = list_group_messages_for_owner_identity(
            &db,
            "alice-identity",
            "did:owner",
            "did:group",
            10,
            None,
        )
        .unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["msg_id"], "msg-group-1");
    }

    #[test]
    fn local_state_groups_lists_active_group_refs_for_recovery() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        db.execute(
            r#"
INSERT INTO groups
    (owner_identity_id, owner_did, group_id, group_did, name, membership_status, last_message_at, stored_at, credential_name)
VALUES
    ('alice-id', 'did:owner', 'group-a', 'did:group:a', 'A', 'active', '2026-05-21T00:00:02Z', '2026-05-21T00:00:02Z', 'alice'),
    ('alice-id', 'did:owner', 'group-b', '', 'B', 'active', '2026-05-21T00:00:03Z', '2026-05-21T00:00:03Z', 'alice'),
    ('alice-id', 'did:owner', 'group-left', 'did:group:left', 'Left', 'left', '2026-05-21T00:00:04Z', '2026-05-21T00:00:04Z', 'alice'),
    ('other-id', 'did:other', 'group-other', 'did:group:other', 'Other', 'active', '2026-05-21T00:00:05Z', '2026-05-21T00:00:05Z', 'other')"#,
            [],
        )
        .unwrap();

        let refs =
            list_active_group_refs_for_owner_identity(&db, "alice-id", "did:owner", 10).unwrap();

        assert_eq!(refs, vec!["group-b", "did:group:a"]);
    }

    #[test]
    fn group_upsert_same_identity_old_new_owner_did_keeps_single_snapshot() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();

        upsert_group(
            &db,
            GroupRecord {
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:owner:old".to_owned(),
                group_id: "did:group:same".to_owned(),
                group_did: "did:group:same".to_owned(),
                name: "Old Name".to_owned(),
                group_owner_did: "did:group-owner".to_owned(),
                member_count: Some(3),
                remote_created_at: "2026-05-21T00:00:00Z".to_owned(),
                credential_name: "alice".to_owned(),
                ..GroupRecord::default()
            },
        )
        .unwrap();
        upsert_group(
            &db,
            GroupRecord {
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:owner:new".to_owned(),
                group_id: "did:group:same".to_owned(),
                group_did: "did:group:same".to_owned(),
                name: "New Name".to_owned(),
                last_message_at: "2026-05-21T00:00:03Z".to_owned(),
                credential_name: "alice".to_owned(),
                ..GroupRecord::default()
            },
        )
        .unwrap();

        let (count, owner_did, name, group_owner_did, member_count): (
            i64,
            String,
            String,
            String,
            i64,
        ) = db
            .query_row(
                r#"
SELECT COUNT(*), MAX(owner_did), MAX(name), MAX(group_owner_did), MAX(member_count)
FROM groups
WHERE owner_identity_id = 'alice-id' AND group_id = 'did:group:same'"#,
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
        assert_eq!(name, "New Name");
        assert_eq!(group_owner_did, "did:group-owner");
        assert_eq!(member_count, 3);
    }

    #[test]
    fn sparse_group_upsert_preserves_security_profile_metadata() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let record = |name: &str, metadata: &str| GroupRecord {
            owner_identity_id: "alice-id".to_owned(),
            owner_did: "did:owner".to_owned(),
            group_id: "did:group:secure".to_owned(),
            group_did: "did:group:secure".to_owned(),
            name: name.to_owned(),
            metadata: metadata.to_owned(),
            credential_name: "alice".to_owned(),
            ..GroupRecord::default()
        };
        upsert_group(
            &db,
            record(
                "Rich",
                r#"{"required_security_profile":"transport-protected","group_policy":{"message_security_profile":"transport-protected"}}"#,
            ),
        )
        .unwrap();
        upsert_group(&db, record("Sparse", r#"{"member_count":3}"#)).unwrap();

        let (name, metadata): (String, String) = db
            .query_row(
                "SELECT name,metadata FROM groups WHERE owner_identity_id='alice-id' AND group_id='did:group:secure'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let metadata: serde_json::Value = serde_json::from_str(&metadata).unwrap();
        assert_eq!(name, "Sparse");
        assert_eq!(metadata["required_security_profile"], "transport-protected");
        assert_eq!(
            metadata["group_policy"]["message_security_profile"],
            "transport-protected"
        );
        assert_eq!(metadata["member_count"], 3);

        upsert_group(
            &db,
            record(
                "Malformed",
                r#"{"required_security_profile":42,"group_policy":{"message_security_profile":42}}"#,
            ),
        )
        .unwrap();
        let metadata: String = db
            .query_row(
                "SELECT metadata FROM groups WHERE owner_identity_id='alice-id' AND group_id='did:group:secure'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let metadata: serde_json::Value = serde_json::from_str(&metadata).unwrap();
        assert_eq!(metadata["required_security_profile"], 42);
        assert_eq!(metadata["group_policy"]["message_security_profile"], 42);
    }

    #[test]
    fn group_member_replacement_is_scoped_by_owner_identity() {
        let mut db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();

        replace_group_members(
            &mut db,
            "alice-id",
            "did:shared-owner",
            "did:group:shared",
            &[GroupMemberRecord {
                user_id: "did:member:old".to_owned(),
                member_did: "did:member:old".to_owned(),
                member_handle: "old.awiki.test".to_owned(),
                role: "member".to_owned(),
                credential_name: "alice".to_owned(),
                ..GroupMemberRecord::default()
            }],
            "alice",
        )
        .unwrap();
        replace_group_members(
            &mut db,
            "bob-id",
            "did:shared-owner",
            "did:group:shared",
            &[GroupMemberRecord {
                user_id: "did:member:bob".to_owned(),
                member_did: "did:member:bob".to_owned(),
                member_handle: "bob.awiki.test".to_owned(),
                role: "member".to_owned(),
                credential_name: "bob".to_owned(),
                ..GroupMemberRecord::default()
            }],
            "bob",
        )
        .unwrap();
        replace_group_members(
            &mut db,
            "alice-id",
            "did:owner:new",
            "did:group:shared",
            &[GroupMemberRecord {
                user_id: "did:member:new".to_owned(),
                member_did: "did:member:new".to_owned(),
                member_handle: "new.awiki.test".to_owned(),
                role: "admin".to_owned(),
                credential_name: "alice".to_owned(),
                ..GroupMemberRecord::default()
            }],
            "alice",
        )
        .unwrap();

        let alice_members = list_cached_group_members_for_owner_identity(
            &db,
            "alice-id",
            "did:owner:new",
            "did:group:shared",
            10,
        )
        .unwrap();
        let bob_members = list_cached_group_members_for_owner_identity(
            &db,
            "bob-id",
            "did:shared-owner",
            "did:group:shared",
            10,
        )
        .unwrap();

        assert_eq!(alice_members.len(), 1);
        assert_eq!(alice_members[0]["member_did"], "did:member:new");
        assert_eq!(bob_members.len(), 1);
        assert_eq!(bob_members[0]["member_did"], "did:member:bob");
    }

    #[test]
    fn group_upsert_without_status_does_not_reactivate_left_group() {
        let mut db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();

        mark_group_left(
            &mut db,
            "alice-id",
            "did:owner",
            "did:group:left",
            "did:group:left",
            "alice",
        )
        .unwrap();
        upsert_group(
            &db,
            GroupRecord {
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:owner:new".to_owned(),
                group_id: "did:group:left".to_owned(),
                group_did: "did:group:left".to_owned(),
                name: "Stale projection".to_owned(),
                credential_name: "alice".to_owned(),
                ..GroupRecord::default()
            },
        )
        .unwrap();

        let (owner_did, membership_status, name): (String, String, String) = db
            .query_row(
                r#"
SELECT owner_did, membership_status, name
FROM groups
WHERE owner_identity_id = 'alice-id' AND group_id = 'did:group:left'"#,
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();

        assert_eq!(owner_did, "did:owner:new");
        assert_eq!(membership_status, "left");
        assert_eq!(name, "Stale projection");
    }

    #[test]
    fn group_e2ee_summary_upsert_does_not_revert_to_older_epoch() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        upsert_group_e2ee_summary(
            &db,
            summary_record("did:group:e2ee", Some("5"), Some("state-5"), "newer-crypto"),
        )
        .unwrap();

        upsert_group_e2ee_summary(
            &db,
            summary_record("did:group:e2ee", Some("4"), Some("state-4"), "older-crypto"),
        )
        .unwrap();

        let snapshot =
            get_group_snapshot_for_owner_identity(&db, "alice-id", "did:owner", "did:group:e2ee")
                .unwrap()
                .unwrap();
        let metadata: serde_json::Value =
            serde_json::from_str(snapshot["metadata"].as_str().unwrap()).unwrap();
        assert_eq!(metadata["group_e2ee"]["epoch"], "5");
        assert_eq!(metadata["group_e2ee"]["group_state_version"], "state-5");
        assert_eq!(
            metadata["group_e2ee"]["crypto_group_id_b64u"],
            "newer-crypto"
        );
    }

    #[test]
    fn group_e2ee_summary_upsert_uses_version_when_epoch_is_missing() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        upsert_group_e2ee_summary(
            &db,
            summary_record("did:group:e2ee", None, Some("state-11"), "newer-crypto"),
        )
        .unwrap();

        upsert_group_e2ee_summary(
            &db,
            summary_record("did:group:e2ee", None, Some("state-10"), "older-crypto"),
        )
        .unwrap();

        let snapshot =
            get_group_snapshot_for_owner_identity(&db, "alice-id", "did:owner", "did:group:e2ee")
                .unwrap()
                .unwrap();
        let metadata: serde_json::Value =
            serde_json::from_str(snapshot["metadata"].as_str().unwrap()).unwrap();
        assert_eq!(metadata["group_e2ee"]["group_state_version"], "state-11");
        assert_eq!(
            metadata["group_e2ee"]["crypto_group_id_b64u"],
            "newer-crypto"
        );
    }

    #[test]
    fn group_e2ee_summary_upsert_rejects_missing_version_over_existing_version() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        upsert_group_e2ee_summary(
            &db,
            summary_record("did:group:e2ee", None, Some("state-11"), "newer-crypto"),
        )
        .unwrap();

        upsert_group_e2ee_summary(
            &db,
            summary_record("did:group:e2ee", None, None, "unknown-crypto"),
        )
        .unwrap();

        let snapshot =
            get_group_snapshot_for_owner_identity(&db, "alice-id", "did:owner", "did:group:e2ee")
                .unwrap()
                .unwrap();
        let metadata: serde_json::Value =
            serde_json::from_str(snapshot["metadata"].as_str().unwrap()).unwrap();
        assert_eq!(metadata["group_e2ee"]["group_state_version"], "state-11");
        assert_eq!(
            metadata["group_e2ee"]["crypto_group_id_b64u"],
            "newer-crypto"
        );
    }

    #[test]
    fn group_e2ee_summary_upsert_accepts_missing_version_for_empty_cache() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();

        upsert_group_e2ee_summary(
            &db,
            summary_record("did:group:e2ee", None, None, "first-crypto"),
        )
        .unwrap();

        let snapshot =
            get_group_snapshot_for_owner_identity(&db, "alice-id", "did:owner", "did:group:e2ee")
                .unwrap()
                .unwrap();
        let metadata: serde_json::Value =
            serde_json::from_str(snapshot["metadata"].as_str().unwrap()).unwrap();
        assert_eq!(
            metadata["group_e2ee"]["crypto_group_id_b64u"],
            "first-crypto"
        );
    }

    fn summary_record(
        group_did: &str,
        epoch: Option<&str>,
        group_state_version: Option<&str>,
        crypto_group_id_b64u: &str,
    ) -> GroupE2eeSummaryRecord {
        let mut group_e2ee = serde_json::Map::new();
        if let Some(epoch) = epoch {
            group_e2ee.insert(
                "epoch".to_owned(),
                serde_json::Value::String(epoch.to_owned()),
            );
        }
        if let Some(group_state_version) = group_state_version {
            group_e2ee.insert(
                "group_state_version".to_owned(),
                serde_json::Value::String(group_state_version.to_owned()),
            );
        }
        group_e2ee.insert(
            "crypto_group_id_b64u".to_owned(),
            serde_json::Value::String(crypto_group_id_b64u.to_owned()),
        );
        let mut metadata = serde_json::Map::new();
        metadata.insert(
            "message_security_profile".to_owned(),
            serde_json::Value::String("group-e2ee".to_owned()),
        );
        metadata.insert(
            "group_e2ee".to_owned(),
            serde_json::Value::Object(group_e2ee),
        );
        if let Some(group_state_version) = group_state_version {
            metadata.insert(
                "group_state_version".to_owned(),
                serde_json::Value::String(group_state_version.to_owned()),
            );
        }
        GroupE2eeSummaryRecord {
            record: GroupRecord {
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:owner".to_owned(),
                group_id: group_did.to_owned(),
                group_did: group_did.to_owned(),
                membership_status: "active".to_owned(),
                metadata: serde_json::Value::Object(metadata).to_string(),
                credential_name: "alice-id".to_owned(),
                ..GroupRecord::default()
            },
            epoch: epoch.map(ToOwned::to_owned),
            group_state_version: group_state_version.map(ToOwned::to_owned),
        }
    }
}
