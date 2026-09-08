//! Disposable display data and refresh leases. Never establishes identity or social relationships.

use rusqlite::{params, Connection, OptionalExtension};

use crate::{directory::DisplayProfile, identity::Profile, ids::Did, ImResult};

pub(crate) fn create_schema(db: &Connection) -> ImResult<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS display_profile_cache (
            owner_identity_id TEXT NOT NULL, did TEXT NOT NULL,
            profile_json TEXT, expires_at INTEGER NOT NULL DEFAULT 0,
            retry_at INTEGER NOT NULL DEFAULT 0, generation INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY(owner_identity_id, did)
        );",
    )
    .map_err(super::local_state_unavailable)
}

#[derive(Clone, Debug)]
pub(crate) struct RefreshLease {
    pub(crate) generation: i64,
    // Includes the fetched timestamp, so a newer explicit Profile refresh wins.
    persona_snapshot: Option<(String, String)>,
}

fn persona_snapshot(db: &Connection, owner: &str, did: &Did) -> ImResult<Option<(String, String)>> {
    db.query_row(
        "SELECT i.peer_persona_id, json_array(p.fetched_at,p.display_name,p.avatar_uri,p.profile_version,p.expires_at,p.full_handle,p.subject_type,p.updated_at)
         FROM peer_identifiers i LEFT JOIN peer_profiles p
           ON p.owner_identity_id=i.owner_identity_id AND p.peer_persona_id=i.peer_persona_id
         WHERE i.owner_identity_id=?1 AND i.identifier_kind='did'
           AND i.identifier_value=?2 AND i.is_current=1",
        params![owner, did.as_str()],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
    .map_err(super::local_state_unavailable)
}

pub(crate) fn read(
    db: &Connection,
    owner: &str,
    did: &Did,
    now: i64,
) -> ImResult<Option<DisplayProfile>> {
    let row: Option<(String, i64)> = db
        .query_row(
            "SELECT profile_json, expires_at FROM display_profile_cache
         WHERE owner_identity_id=?1 AND did=?2 AND profile_json IS NOT NULL",
            params![owner, did.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(super::local_state_unavailable)?;
    row.map(|(json, expires)| {
        let mut profile: DisplayProfile =
            serde_json::from_str(&json).map_err(|_| crate::ImError::Serialization {
                detail: "invalid display profile cache".to_owned(),
            })?;
        profile.is_stale = expires <= now;
        Ok(profile)
    })
    .transpose()
}

pub(crate) fn claim(
    db: &Connection,
    owner: &str,
    did: &Did,
    force: bool,
    now: i64,
) -> ImResult<Option<RefreshLease>> {
    let tx = db
        .unchecked_transaction()
        .map_err(super::local_state_unavailable)?;
    if !force {
        let expires: Option<i64> = tx.query_row(
            "SELECT COALESCE(CAST(p.expires_at AS INTEGER), CAST(p.fetched_at AS INTEGER)+300)
             FROM peer_identifiers i JOIN peer_profiles p ON p.owner_identity_id=i.owner_identity_id AND p.peer_persona_id=i.peer_persona_id
             WHERE i.owner_identity_id=?1 AND i.identifier_kind='did' AND i.identifier_value=?2 AND i.is_current=1",
            params![owner,did.as_str()], |r| r.get(0)).optional().map_err(super::local_state_unavailable)?;
        if expires.is_some_and(|expires| expires > now) {
            return Ok(None);
        }
    }
    // Bound disposable state, without evicting live leases.
    tx.execute(
        "DELETE FROM display_profile_cache WHERE owner_identity_id=?1 AND retry_at<=?2
        AND did IN (SELECT did FROM display_profile_cache WHERE owner_identity_id=?1
        ORDER BY expires_at DESC, did LIMIT -1 OFFSET 4095)",
        params![owner, now],
    )
    .map_err(super::local_state_unavailable)?;
    tx.execute(
        "INSERT OR IGNORE INTO display_profile_cache(owner_identity_id,did) VALUES (?1,?2)",
        params![owner, did.as_str()],
    )
    .map_err(super::local_state_unavailable)?;
    // A nonce survives delete/recreate races: a late lease can never match a new row.
    let changed = tx
        .execute(
            "UPDATE display_profile_cache SET generation=?5,retry_at=?3+30
        WHERE owner_identity_id=?1 AND did=?2 AND retry_at<=?3 AND (?4 OR expires_at<=?3)",
            params![owner, did.as_str(), now, force, rand::random::<i64>()],
        )
        .map_err(super::local_state_unavailable)?;
    let lease = if changed == 0 {
        None
    } else {
        Some(RefreshLease {
            generation: tx.query_row("SELECT generation FROM display_profile_cache WHERE owner_identity_id=?1 AND did=?2",
                params![owner,did.as_str()], |r| r.get(0)).map_err(super::local_state_unavailable)?,
            persona_snapshot: persona_snapshot(&tx, owner, did)?,
        })
    };
    tx.commit().map_err(super::local_state_unavailable)?;
    Ok(lease)
}

pub(crate) fn finish(
    db: &Connection,
    owner: &str,
    did: &Did,
    lease: &RefreshLease,
    profile: Option<Profile>,
    now: i64,
) -> ImResult<()> {
    let tx = db
        .unchecked_transaction()
        .map_err(super::local_state_unavailable)?;
    let generation: Option<i64> = tx
        .query_row(
            "SELECT generation FROM display_profile_cache WHERE owner_identity_id=?1 AND did=?2",
            params![owner, did.as_str()],
            |r| r.get(0),
        )
        .optional()
        .map_err(super::local_state_unavailable)?;
    // A deleted owner/cache or a successor lease must never be recreated by a late reply.
    if generation != Some(lease.generation) {
        return Ok(());
    }
    if let Some(mut profile) = profile {
        if persona_snapshot(&tx, owner, did)? != lease.persona_snapshot {
            return Ok(());
        }
        let ttl = profile.ttl.unwrap_or(300).clamp(30, 3600);
        profile.ttl = Some(ttl);
        let json = if super::peer_profiles::refresh_existing_from_public_profile(
            &tx, owner, did, &profile,
        )? {
            None
        } else {
            Some(
                serde_json::to_string(&DisplayProfile {
                    did: Some(did.clone()),
                    handle: profile.handle.clone(),
                    display_name: profile.display_name.clone(),
                    avatar_uri: profile.avatar_uri.clone(),
                    avatar_url: profile.avatar_url.clone(),
                    profile_uri: profile.profile_uri.clone(),
                    subject_type: profile.subject_type.clone(),
                    cache_hit: true,
                    is_stale: false,
                    legacy_fallback: false,
                    warnings: Vec::new(),
                })
                .map_err(|_| crate::ImError::Serialization {
                    detail: "invalid display profile cache".to_owned(),
                })?,
            )
        };
        tx.execute("UPDATE display_profile_cache SET profile_json=?3,expires_at=?4,retry_at=0 WHERE owner_identity_id=?1 AND did=?2",
            params![owner,did.as_str(),json,now+ttl as i64]).map_err(super::local_state_unavailable)?;
    } else {
        tx.execute(
            "UPDATE display_profile_cache SET retry_at=?3+5 WHERE owner_identity_id=?1 AND did=?2",
            params![owner, did.as_str(), now],
        )
        .map_err(super::local_state_unavailable)?;
    }
    tx.commit().map_err(super::local_state_unavailable)
}

#[cfg(test)]
#[path = "display_profile_cache_tests.rs"]
mod tests;
