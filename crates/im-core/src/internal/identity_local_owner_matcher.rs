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
  AND (owner_identity_id=?1 OR previous_did=?2 OR current_did=?2)"#,
            rusqlite::params![candidate.owner_identity_id, authority.previous_did],
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
        display_name: (!entry.name.trim().is_empty())
            .then(|| entry.name.clone())
            .unwrap_or(canonical.local_part),
        make_default: entry.is_default,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

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
            match_stable_owner(&path, &fixture_index(), authority(), None).unwrap(),
            StableOwnerMatch::Exact(StableOwnerCandidate { owner_identity_id, .. })
                if owner_identity_id == "owner-alice"
        ));
    }

    #[test]
    fn recovery_owner_continuity_distinguishes_fresh_none_from_partial_conflict() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("im.sqlite");
        assert_eq!(
            match_stable_owner(&path, &fixture_index(), authority(), None).unwrap(),
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
            match_stable_owner(&path, &fixture_index(), authority(), None).unwrap(),
            StableOwnerMatch::Conflict
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
            match_stable_owner(&path, &fixture_index(), authority(), None).unwrap(),
            StableOwnerMatch::Conflict
        );
    }
}
