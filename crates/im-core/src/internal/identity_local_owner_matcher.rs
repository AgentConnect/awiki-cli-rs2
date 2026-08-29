//! Shared, fail-closed stable-owner selection for direct-previous Recovery.

use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StableOwnerAuthority<'a> {
    pub(crate) account_user_id: &'a str,
    pub(crate) full_handle: &'a str,
    pub(crate) previous_did: &'a str,
    /// The committed transition generation. A local candidate must be its
    /// direct canonical predecessor.
    pub(crate) binding_generation: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StableOwnerCandidate {
    pub(crate) owner_identity_id: String,
    pub(crate) local_alias: String,
    pub(crate) display_name: String,
    pub(crate) make_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StableOwnerMatch {
    Exact(StableOwnerCandidate),
    None,
    Conflict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RetiredOwnerEpochRelation {
    Current,
    DirectPrevious,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RetiredOwnerEvidence {
    pub(crate) owner_identity_id: String,
    pub(crate) retired_did: String,
    pub(crate) retired_protocol_device_id: String,
    pub(crate) retired_binding_generation: String,
    pub(crate) epoch_relation: RetiredOwnerEpochRelation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RegistrationOwnerDisposition {
    ExactLivePredecessor(StableOwnerCandidate),
    RetiredNoLiveCredential(RetiredOwnerEvidence),
    FreshNone,
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RegistrationOwnerAuthority<'a> {
    pub(crate) account_user_id: &'a str,
    pub(crate) full_handle: &'a str,
    pub(crate) previous_did: &'a str,
    pub(crate) current_did: &'a str,
    pub(crate) binding_generation: &'a str,
}

pub(crate) fn classify_registration_owner(
    sqlite_path: &Path,
    identity_root_dir: &Path,
    index: &crate::internal::identity_store::IndexPayload,
    authority: RegistrationOwnerAuthority<'_>,
    excluded_transition_source_id: Option<&str>,
) -> crate::ImResult<RegistrationOwnerDisposition> {
    crate::internal::identity_wire::handle_recovery::canonical_handle(authority.full_handle)?;
    crate::ids::Did::parse(authority.previous_did)?;
    crate::ids::Did::parse(authority.current_did)?;
    let Some(previous_generation) =
        crate::internal::identity_handle_recovery_pending::previous_canonical_generation(
            authority.binding_generation,
        )
    else {
        return Err(crate::ImError::PermissionDenied);
    };
    if authority.account_user_id.trim().is_empty()
        || authority.previous_did == authority.current_did
    {
        return Err(crate::ImError::PermissionDenied);
    }

    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    let unfinished_transition_count: i64 = connection
        .query_row(
            r#"SELECT COUNT(*) FROM identity_transition_pending
WHERE phase IN ('pending','identity_switched')
  AND (?1 IS NULL OR source_id<>?1)
  AND (
    account_user_id=?2 OR handle=?3
    OR previous_did IN (?4,?5) OR current_did IN (?4,?5)
    OR owner_identity_id IN (
      SELECT owner_identity_id FROM identity_account_bindings
      WHERE account_id=?2 OR handle_scope=?3 OR current_did IN (?4,?5)
    )
  )"#,
            rusqlite::params![
                excluded_transition_source_id,
                authority.account_user_id,
                authority.full_handle,
                authority.previous_did,
                authority.current_did,
            ],
            |row| row.get(0),
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let unfinished_operation_count: i64 = connection
        .query_row(
            r#"SELECT COUNT(*) FROM handle_recovery_operations_v4
WHERE lifecycle_class IN ('pre_commit','remote_unresolved','remote_committed','local_transition_pending')
  AND (
    account_user_id=?1 OR full_handle=?2
    OR owner_identity_id IN (
      SELECT owner_identity_id FROM identity_account_bindings
      WHERE account_id=?1 OR handle_scope=?2 OR current_did IN (?3,?4)
    )
  )"#,
            rusqlite::params![
                authority.account_user_id,
                authority.full_handle,
                authority.previous_did,
                authority.current_did,
            ],
            |row| row.get(0),
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if unfinished_transition_count != 0 || unfinished_operation_count != 0 {
        return Ok(RegistrationOwnerDisposition::Conflict);
    }

    let mut statement = connection
        .prepare(
            r#"SELECT owner_identity_id,account_id,handle_scope,current_did,device_id,identity_generation
FROM identity_account_bindings
WHERE account_id=?1 OR handle_scope=?2 OR current_did IN (?3,?4)
ORDER BY owner_identity_id"#,
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let related_bindings = statement
        .query_map(
            rusqlite::params![
                authority.account_user_id,
                authority.full_handle,
                authority.previous_did,
                authority.current_did,
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let related_index_count = index
        .credentials
        .values()
        .filter(|entry| {
            entry.user_id == authority.account_user_id
                || entry.full_handle == authority.full_handle
                || entry.did == authority.previous_did
                || entry.did == authority.current_did
        })
        .count();
    if related_bindings.is_empty() && related_index_count == 0 {
        return Ok(RegistrationOwnerDisposition::FreshNone);
    }

    // Retirement is intentionally classified before the direct-previous live
    // matcher. Otherwise an exact retired current binding is prematurely
    // treated as suspicious related state and can never reach ordinary Join.
    if related_bindings.len() == 1 && related_index_count == 0 {
        let (
            owner_identity_id,
            account_id,
            handle_scope,
            retired_did,
            retired_device_id,
            retired_generation,
        ) = &related_bindings[0];
        if account_id == authority.account_user_id
            && handle_scope.as_deref() == Some(authority.full_handle)
        {
            let epoch_relation = if retired_did == authority.current_did
                && retired_generation == authority.binding_generation
            {
                Some(RetiredOwnerEpochRelation::Current)
            } else if retired_did == authority.previous_did
                && retired_generation == &previous_generation
            {
                Some(RetiredOwnerEpochRelation::DirectPrevious)
            } else {
                None
            };
            if let Some(epoch_relation) = epoch_relation {
                if crate::internal::identity_retirement::matches_completed_binding(
                    identity_root_dir,
                    owner_identity_id,
                    retired_did,
                    retired_device_id,
                )? {
                    return Ok(RegistrationOwnerDisposition::RetiredNoLiveCredential(
                        RetiredOwnerEvidence {
                            owner_identity_id: owner_identity_id.clone(),
                            retired_did: retired_did.clone(),
                            retired_protocol_device_id: retired_device_id.clone(),
                            retired_binding_generation: retired_generation.clone(),
                            epoch_relation,
                        },
                    ));
                }
            }
        }
    }

    match match_stable_owner(
        sqlite_path,
        index,
        StableOwnerAuthority {
            account_user_id: authority.account_user_id,
            full_handle: authority.full_handle,
            previous_did: authority.previous_did,
            binding_generation: authority.binding_generation,
        },
        None,
        excluded_transition_source_id,
    )? {
        StableOwnerMatch::Exact(owner) => {
            Ok(RegistrationOwnerDisposition::ExactLivePredecessor(owner))
        }
        StableOwnerMatch::None | StableOwnerMatch::Conflict => {
            Ok(RegistrationOwnerDisposition::Conflict)
        }
    }
}

/// Classifies an ordinary registration response when no Recovery authority is
/// available. Live or ambiguous local state related to the Handle or target
/// DID must fail closed as a missing transition. One exact stable binding whose
/// credential was removed by a completed identity-retirement transaction is
/// not live identity state and may start an ordinary Join again.
pub(crate) fn match_stable_owner_without_transition(
    sqlite_path: &Path,
    identity_root_dir: &Path,
    index: &crate::internal::identity_store::IndexPayload,
    full_handle: &str,
    current_did: &str,
) -> crate::ImResult<StableOwnerMatch> {
    crate::internal::identity_wire::handle_recovery::canonical_handle(full_handle)?;
    crate::ids::Did::parse(current_did)?;
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    let mut statement = connection
        .prepare(
            r#"SELECT owner_identity_id,handle_scope,current_did,device_id
FROM identity_account_bindings
WHERE handle_scope=?1 OR current_did=?2
ORDER BY owner_identity_id"#,
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let related_bindings = statement
        .query_map(rusqlite::params![full_handle, current_did], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(crate::internal::local_state::local_state_unavailable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let related_index_count = index
        .credentials
        .values()
        .filter(|entry| entry.full_handle == full_handle || entry.did == current_did)
        .count();
    if related_bindings.is_empty() && related_index_count == 0 {
        return Ok(StableOwnerMatch::None);
    }
    if related_index_count != 0 || related_bindings.len() != 1 {
        return Ok(StableOwnerMatch::Conflict);
    }
    let (owner_identity_id, handle_scope, binding_did, protocol_device_id) = &related_bindings[0];
    if handle_scope.as_deref() != Some(full_handle) || binding_did != current_did {
        return Ok(StableOwnerMatch::Conflict);
    }
    Ok(
        if crate::internal::identity_retirement::matches_completed_binding(
            identity_root_dir,
            owner_identity_id,
            binding_did,
            protocol_device_id,
        )? {
            StableOwnerMatch::None
        } else {
            StableOwnerMatch::Conflict
        },
    )
}

#[derive(Debug)]
struct BindingCandidate {
    owner_identity_id: String,
    identity_generation: String,
}

pub(crate) fn match_stable_owner(
    sqlite_path: &Path,
    index: &crate::internal::identity_store::IndexPayload,
    authority: StableOwnerAuthority<'_>,
    excluded_recovery_operation_id: Option<&str>,
    excluded_transition_source_id: Option<&str>,
) -> crate::ImResult<StableOwnerMatch> {
    let canonical =
        crate::internal::identity_wire::handle_recovery::canonical_handle(authority.full_handle)?;
    let Some(previous_generation) =
        crate::internal::identity_handle_recovery_pending::previous_canonical_generation(
            authority.binding_generation,
        )
    else {
        return Err(crate::ImError::PermissionDenied);
    };
    if authority.account_user_id.trim().is_empty() || authority.previous_did.trim().is_empty() {
        return Err(crate::ImError::PermissionDenied);
    }

    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    let mut statement = connection
        .prepare(
            r#"SELECT owner_identity_id,account_id,handle_scope,current_did,identity_generation
FROM identity_account_bindings
WHERE account_id=?1 OR handle_scope=?2 OR current_did=?3
ORDER BY owner_identity_id"#,
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let rows = statement
        .query_map(
            rusqlite::params![
                authority.account_user_id,
                authority.full_handle,
                authority.previous_did
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let related = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(crate::internal::local_state::local_state_unavailable)?;

    let mut exact = Vec::new();
    let mut suspicious_related_state = false;
    for (owner_identity_id, account_id, handle_scope, current_did, identity_generation) in related {
        let exact_scope = account_id == authority.account_user_id
            && handle_scope.as_deref() == Some(authority.full_handle)
            && current_did == authority.previous_did;
        if exact_scope && identity_generation == previous_generation {
            exact.push(BindingCandidate {
                owner_identity_id,
                identity_generation,
            });
        } else {
            // A partially matching local binding is not a fresh machine. It
            // must not be silently downgraded to an ordinary/fresh owner.
            suspicious_related_state = true;
        }
    }

    if exact.is_empty() {
        return Ok(if suspicious_related_state {
            StableOwnerMatch::Conflict
        } else {
            StableOwnerMatch::None
        });
    }
    if exact.len() != 1 || suspicious_related_state {
        return Ok(StableOwnerMatch::Conflict);
    }
    let candidate = exact.pop().ok_or(crate::ImError::PermissionDenied)?;

    let unfinished_transition_count: i64 = connection
        .query_row(
            r#"SELECT COUNT(*) FROM identity_transition_pending
WHERE phase IN ('pending','identity_switched')
  AND (owner_identity_id=?1 OR previous_did=?2 OR current_did=?2)
  AND (?3 IS NULL OR source_id<>?3)"#,
            rusqlite::params![
                candidate.owner_identity_id,
                authority.previous_did,
                excluded_transition_source_id
            ],
            |row| row.get(0),
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let unfinished_operation_count: i64 = connection
        .query_row(
            r#"SELECT COUNT(*) FROM handle_recovery_operations_v4
WHERE owner_identity_id=?1
  AND lifecycle_class IN ('pre_commit','remote_unresolved','remote_committed','local_transition_pending')
  AND (?2 IS NULL OR operation_id<>?2)"#,
            rusqlite::params![candidate.owner_identity_id, excluded_recovery_operation_id],
            |row| row.get(0),
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if unfinished_transition_count != 0 || unfinished_operation_count != 0 {
        return Ok(StableOwnerMatch::Conflict);
    }

    let matching_entries = index
        .credentials
        .values()
        .filter(|entry| {
            entry.unique_id == candidate.owner_identity_id
                && entry.user_id == authority.account_user_id
                && entry.full_handle == authority.full_handle
                && entry.did == authority.previous_did
                && entry.binding_generation.as_deref()
                    == Some(candidate.identity_generation.as_str())
        })
        .collect::<Vec<_>>();
    if matching_entries.len() != 1 {
        return Ok(StableOwnerMatch::Conflict);
    }
    let entry = matching_entries[0];
    if entry.credential_name.trim().is_empty() {
        return Ok(StableOwnerMatch::Conflict);
    }
    Ok(StableOwnerMatch::Exact(StableOwnerCandidate {
        owner_identity_id: candidate.owner_identity_id,
        local_alias: entry.credential_name.clone(),
        display_name: if entry.name.trim().is_empty() {
            canonical.local_part
        } else {
            entry.name.clone()
        },
        make_default: entry.is_default,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use sha2::{Digest, Sha256};

    fn fixture_index() -> crate::internal::identity_store::IndexPayload {
        let mut index = crate::internal::identity_store::IndexPayload::default();
        index.credentials.insert(
            "alice".to_owned(),
            crate::internal::identity_store::IndexEntry {
                credential_name: "alice".to_owned(),
                did: "did:wba:example.invalid:user:alice:old".to_owned(),
                unique_id: "owner-alice".to_owned(),
                user_id: "account-alice".to_owned(),
                name: "Alice".to_owned(),
                full_handle: "alice.example.invalid".to_owned(),
                binding_generation: Some("7".to_owned()),
                is_default: true,
                ..Default::default()
            },
        );
        index
    }

    fn insert_binding(
        path: &Path,
        owner: &str,
        account: &str,
        handle: &str,
        did: &str,
        generation: &str,
    ) {
        let connection = crate::internal::local_state::open_writable(path).unwrap();
        connection
            .execute(
                "INSERT INTO identity_account_bindings(owner_identity_id,account_id,handle_scope,current_did,device_id,identity_generation,device_auth_generation,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,'3',1,1)",
                rusqlite::params![owner, account, handle, did, format!("device-{owner}"), generation],
            )
            .unwrap();
    }

    fn authority<'a>() -> StableOwnerAuthority<'a> {
        StableOwnerAuthority {
            account_user_id: "account-alice",
            full_handle: "alice.example.invalid",
            previous_did: "did:wba:example.invalid:user:alice:old",
            binding_generation: "8",
        }
    }

    fn write_completed_retirement(
        identity_root_dir: &Path,
        owner_identity_id: &str,
        did: &str,
        protocol_device_id: &str,
    ) {
        let directory = identity_root_dir.join(".identity-retirements");
        std::fs::create_dir_all(&directory).unwrap();
        let digest = Sha256::digest(owner_identity_id.as_bytes());
        let path = directory.join(format!("{}.json", URL_SAFE_NO_PAD.encode(digest)));
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&serde_json::json!({
                "schema_version": 1,
                "identity_id": owner_identity_id,
                "did": did,
                "local_alias": "alice",
                "identity_dir_name": "owner-alice",
                "protocol_device_id": protocol_device_id,
                "phase": "completed"
            }))
            .unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn recovery_owner_continuity_matches_one_exact_direct_previous_owner() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("im.sqlite");
        insert_binding(
            &path,
            "owner-alice",
            "account-alice",
            "alice.example.invalid",
            "did:wba:example.invalid:user:alice:old",
            "7",
        );

        assert!(matches!(
            match_stable_owner(&path, &fixture_index(), authority(), None, None).unwrap(),
            StableOwnerMatch::Exact(StableOwnerCandidate { owner_identity_id, .. })
                if owner_identity_id == "owner-alice"
        ));
    }

    #[test]
    fn recovery_owner_continuity_distinguishes_fresh_none_from_partial_conflict() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("im.sqlite");
        assert_eq!(
            match_stable_owner(&path, &fixture_index(), authority(), None, None).unwrap(),
            StableOwnerMatch::None
        );

        insert_binding(
            &path,
            "owner-alice",
            "account-alice",
            "alice.example.invalid",
            "did:wba:example.invalid:user:alice:partial",
            "7",
        );
        assert_eq!(
            match_stable_owner(&path, &fixture_index(), authority(), None, None).unwrap(),
            StableOwnerMatch::Conflict
        );
    }

    #[test]
    fn registration_recovery_join_missing_transition_distinguishes_fresh_from_local_state() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("im.sqlite");
        let identity_root_dir = root.path().join("identities");
        let empty_index = crate::internal::identity_store::IndexPayload::default();
        assert_eq!(
            match_stable_owner_without_transition(
                &path,
                &identity_root_dir,
                &empty_index,
                "alice.example.invalid",
                "did:wba:example.invalid:user:alice:new",
            )
            .unwrap(),
            StableOwnerMatch::None
        );
        insert_binding(
            &path,
            "owner-alice",
            "account-alice",
            "alice.example.invalid",
            "did:wba:example.invalid:user:alice:old",
            "7",
        );
        assert_eq!(
            match_stable_owner_without_transition(
                &path,
                &identity_root_dir,
                &empty_index,
                "alice.example.invalid",
                "did:wba:example.invalid:user:alice:new",
            )
            .unwrap(),
            StableOwnerMatch::Conflict
        );
    }

    #[test]
    fn registration_join_treats_exact_completed_retirement_as_no_live_local_identity() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("im.sqlite");
        let identity_root_dir = root.path().join("identities");
        let did = "did:wba:example.invalid:user:alice:retired";
        let empty_index = crate::internal::identity_store::IndexPayload::default();
        insert_binding(
            &path,
            "owner-alice",
            "account-alice",
            "alice.example.invalid",
            did,
            "7",
        );
        write_completed_retirement(&identity_root_dir, "owner-alice", did, "device-owner-alice");

        assert_eq!(
            match_stable_owner_without_transition(
                &path,
                &identity_root_dir,
                &empty_index,
                "alice.example.invalid",
                did,
            )
            .unwrap(),
            StableOwnerMatch::None
        );
    }

    #[test]
    fn registration_join_keeps_mismatched_retirement_fail_closed() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("im.sqlite");
        let identity_root_dir = root.path().join("identities");
        let did = "did:wba:example.invalid:user:alice:retired";
        let empty_index = crate::internal::identity_store::IndexPayload::default();
        insert_binding(
            &path,
            "owner-alice",
            "account-alice",
            "alice.example.invalid",
            did,
            "7",
        );
        write_completed_retirement(&identity_root_dir, "owner-alice", did, "device-different");

        assert_eq!(
            match_stable_owner_without_transition(
                &path,
                &identity_root_dir,
                &empty_index,
                "alice.example.invalid",
                did,
            )
            .unwrap(),
            StableOwnerMatch::Conflict
        );
    }

    #[test]
    fn registration_owner_accepts_exact_retired_current_before_suspicious_filter() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("im.sqlite");
        let identity_root_dir = root.path().join("identities");
        let current_did = "did:wba:example.invalid:user:alice:new";
        insert_binding(
            &path,
            "owner-alice",
            "account-alice",
            "alice.example.invalid",
            current_did,
            "8",
        );
        write_completed_retirement(
            &identity_root_dir,
            "owner-alice",
            current_did,
            "device-owner-alice",
        );

        assert!(matches!(
            classify_registration_owner(
                &path,
                &identity_root_dir,
                &crate::internal::identity_store::IndexPayload::default(),
                RegistrationOwnerAuthority {
                    account_user_id: "account-alice",
                    full_handle: "alice.example.invalid",
                    previous_did: "did:wba:example.invalid:user:alice:old",
                    current_did,
                    binding_generation: "8",
                },
                None,
            )
            .unwrap(),
            RegistrationOwnerDisposition::RetiredNoLiveCredential(RetiredOwnerEvidence {
                owner_identity_id,
                epoch_relation: RetiredOwnerEpochRelation::Current,
                ..
            }) if owner_identity_id == "owner-alice"
        ));
    }

    #[test]
    fn registration_owner_accepts_exact_retired_direct_previous() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("im.sqlite");
        let identity_root_dir = root.path().join("identities");
        let previous_did = "did:wba:example.invalid:user:alice:old";
        insert_binding(
            &path,
            "owner-alice",
            "account-alice",
            "alice.example.invalid",
            previous_did,
            "7",
        );
        write_completed_retirement(
            &identity_root_dir,
            "owner-alice",
            previous_did,
            "device-owner-alice",
        );

        assert!(matches!(
            classify_registration_owner(
                &path,
                &identity_root_dir,
                &crate::internal::identity_store::IndexPayload::default(),
                RegistrationOwnerAuthority {
                    account_user_id: "account-alice",
                    full_handle: "alice.example.invalid",
                    previous_did,
                    current_did: "did:wba:example.invalid:user:alice:new",
                    binding_generation: "8",
                },
                None,
            )
            .unwrap(),
            RegistrationOwnerDisposition::RetiredNoLiveCredential(RetiredOwnerEvidence {
                owner_identity_id,
                epoch_relation: RetiredOwnerEpochRelation::DirectPrevious,
                ..
            }) if owner_identity_id == "owner-alice"
        ));
    }

    #[test]
    fn retired_registration_join_prepared_does_not_block_new_registration() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("im.sqlite");
        let identity_root_dir = root.path().join("identities");
        let previous_did = "did:wba:example.invalid:user:alice:old";
        let current_did = "did:wba:example.invalid:user:alice:new";
        insert_binding(
            &path,
            "owner-alice",
            "account-alice",
            "alice.example.invalid",
            previous_did,
            "7",
        );
        write_completed_retirement(
            &identity_root_dir,
            "owner-alice",
            previous_did,
            "device-owner-alice",
        );
        let transition =
            crate::internal::identity_registration_join_preparation::RegistrationJoinTransition {
                account_user_id: "account-alice".to_owned(),
                previous_did: previous_did.to_owned(),
                current_did: current_did.to_owned(),
                binding_generation: "8".to_owned(),
            };
        let evidence = RetiredOwnerEvidence {
            owner_identity_id: "owner-alice".to_owned(),
            retired_did: previous_did.to_owned(),
            retired_protocol_device_id: "device-owner-alice".to_owned(),
            retired_binding_generation: "7".to_owned(),
            epoch_relation: RetiredOwnerEpochRelation::DirectPrevious,
        };
        let journal =
            crate::internal::identity_registration_retired_join::RetiredJoinRollover::prepared(
                "join-orphan",
                "account-alice",
                "alice.example.invalid",
                &transition,
                &evidence,
                "dev-new",
                "2099-08-28T12:00:00Z",
            )
            .unwrap();
        crate::internal::identity_registration_retired_join::insert_prepared(&path, &journal)
            .unwrap();

        assert!(matches!(
            classify_registration_owner(
                &path,
                &identity_root_dir,
                &crate::internal::identity_store::IndexPayload::default(),
                RegistrationOwnerAuthority {
                    account_user_id: "account-alice",
                    full_handle: "alice.example.invalid",
                    previous_did,
                    current_did,
                    binding_generation: "8",
                },
                None,
            )
            .unwrap(),
            RegistrationOwnerDisposition::RetiredNoLiveCredential(_)
        ));
    }

    #[test]
    fn registration_owner_rejects_retired_n_minus_two() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("im.sqlite");
        let identity_root_dir = root.path().join("identities");
        let retired_did = "did:wba:example.invalid:user:alice:n-minus-two";
        insert_binding(
            &path,
            "owner-alice",
            "account-alice",
            "alice.example.invalid",
            retired_did,
            "6",
        );
        write_completed_retirement(
            &identity_root_dir,
            "owner-alice",
            retired_did,
            "device-owner-alice",
        );

        assert_eq!(
            classify_registration_owner(
                &path,
                &identity_root_dir,
                &crate::internal::identity_store::IndexPayload::default(),
                RegistrationOwnerAuthority {
                    account_user_id: "account-alice",
                    full_handle: "alice.example.invalid",
                    previous_did: "did:wba:example.invalid:user:alice:old",
                    current_did: "did:wba:example.invalid:user:alice:new",
                    binding_generation: "8",
                },
                None,
            )
            .unwrap(),
            RegistrationOwnerDisposition::Conflict
        );
    }

    #[test]
    fn registration_owner_rejects_wrong_marker_device_or_account() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("im.sqlite");
        let identity_root_dir = root.path().join("identities");
        let current_did = "did:wba:example.invalid:user:alice:new";
        insert_binding(
            &path,
            "owner-alice",
            "account-alice",
            "alice.example.invalid",
            current_did,
            "8",
        );
        write_completed_retirement(&identity_root_dir, "owner-alice", current_did, "dev-wrong");

        assert_eq!(
            classify_registration_owner(
                &path,
                &identity_root_dir,
                &crate::internal::identity_store::IndexPayload::default(),
                RegistrationOwnerAuthority {
                    account_user_id: "account-alice",
                    full_handle: "alice.example.invalid",
                    previous_did: "did:wba:example.invalid:user:alice:old",
                    current_did,
                    binding_generation: "8",
                },
                None,
            )
            .unwrap(),
            RegistrationOwnerDisposition::Conflict
        );
    }

    #[test]
    fn registration_owner_rejects_live_multiple_or_unfinished_state() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("im.sqlite");
        let identity_root_dir = root.path().join("identities");
        let current_did = "did:wba:example.invalid:user:alice:new";
        insert_binding(
            &path,
            "owner-alice",
            "account-alice",
            "alice.example.invalid",
            current_did,
            "8",
        );
        write_completed_retirement(
            &identity_root_dir,
            "owner-alice",
            current_did,
            "device-owner-alice",
        );
        let connection = crate::internal::local_state::open_writable(&path).unwrap();
        connection
            .execute(
                "INSERT INTO handle_recovery_operations_v4(operation_id,owner_identity_id,account_user_id,full_handle,lifecycle_class,commit_attempted,key_state,vault_key_id,created_at,updated_at) VALUES ('active-op','owner-alice','account-alice','alice.example.invalid','remote_unresolved',1,'available','secret-ref','old','old')",
                [],
            )
            .unwrap();

        assert_eq!(
            classify_registration_owner(
                &path,
                &identity_root_dir,
                &crate::internal::identity_store::IndexPayload::default(),
                RegistrationOwnerAuthority {
                    account_user_id: "account-alice",
                    full_handle: "alice.example.invalid",
                    previous_did: "did:wba:example.invalid:user:alice:old",
                    current_did,
                    binding_generation: "8",
                },
                None,
            )
            .unwrap(),
            RegistrationOwnerDisposition::Conflict
        );

        let live_root = tempfile::tempdir().unwrap();
        let live_path = live_root.path().join("im.sqlite");
        insert_binding(
            &live_path,
            "owner-alice",
            "account-alice",
            "alice.example.invalid",
            current_did,
            "8",
        );
        let mut live_index = fixture_index();
        let live_entry = live_index.credentials.get_mut("alice").unwrap();
        live_entry.did = current_did.to_owned();
        live_entry.binding_generation = Some("8".to_owned());
        assert_eq!(
            classify_registration_owner(
                &live_path,
                &live_root.path().join("identities"),
                &live_index,
                RegistrationOwnerAuthority {
                    account_user_id: "account-alice",
                    full_handle: "alice.example.invalid",
                    previous_did: "did:wba:example.invalid:user:alice:old",
                    current_did,
                    binding_generation: "8",
                },
                None,
            )
            .unwrap(),
            RegistrationOwnerDisposition::Conflict
        );

        let multiple_root = tempfile::tempdir().unwrap();
        let multiple_path = multiple_root.path().join("im.sqlite");
        insert_binding(
            &multiple_path,
            "owner-alice",
            "account-alice",
            "alice.example.invalid",
            current_did,
            "8",
        );
        insert_binding(
            &multiple_path,
            "owner-alice-other",
            "account-alice",
            "alice.example.invalid",
            current_did,
            "8",
        );
        assert_eq!(
            classify_registration_owner(
                &multiple_path,
                &multiple_root.path().join("identities"),
                &crate::internal::identity_store::IndexPayload::default(),
                RegistrationOwnerAuthority {
                    account_user_id: "account-alice",
                    full_handle: "alice.example.invalid",
                    previous_did: "did:wba:example.invalid:user:alice:old",
                    current_did,
                    binding_generation: "8",
                },
                None,
            )
            .unwrap(),
            RegistrationOwnerDisposition::Conflict
        );
    }

    #[test]
    fn recovery_owner_continuity_rejects_unfinished_transition() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("im.sqlite");
        insert_binding(
            &path,
            "owner-alice",
            "account-alice",
            "alice.example.invalid",
            "did:wba:example.invalid:user:alice:old",
            "7",
        );
        let connection = crate::internal::local_state::open_writable(&path).unwrap();
        connection
            .execute(
                "INSERT INTO identity_transition_pending(recovery_id,schema_version,contract_version,contract_hash,source_kind,source_id,state_root_fingerprint,account_user_id,owner_identity_id,handle,previous_did,current_did,binding_generation,phase,created_at,updated_at) VALUES ('other-transition',1,?1,?2,'joined_device','join-other',?3,'account-alice','owner-alice','alice.example.invalid','did:wba:example.invalid:user:alice:old','did:wba:example.invalid:user:alice:new','8','pending','2026-08-10T00:00:00Z','2026-08-10T00:00:00Z')",
                rusqlite::params![
                    crate::internal::identity_handle_recovery_pending::V4_CONTRACT_VERSION,
                    crate::internal::identity_handle_recovery_pending::V4_CONTRACT_HASH,
                    crate::internal::identity_transition_pending::state_root_fingerprint(&path),
                ],
            )
            .unwrap();

        assert_eq!(
            match_stable_owner(&path, &fixture_index(), authority(), None, None).unwrap(),
            StableOwnerMatch::Conflict
        );
    }
}
