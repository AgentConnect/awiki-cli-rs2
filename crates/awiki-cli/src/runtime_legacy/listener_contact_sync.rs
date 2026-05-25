use crate::legacy_store::{self as store, ContactRecord};

pub type IncomingContactLookup<'a> = &'a mut dyn FnMut(&str) -> anyhow::Result<Option<String>>;

pub fn sync_incoming_contact(
    connection: &mut rusqlite::Connection,
    owner_did: &str,
    sender_did: &str,
    source_type: &str,
    source_group_id: &str,
    lookup_handle_by_did: Option<IncomingContactLookup<'_>>,
) -> anyhow::Result<String> {
    let owner_did = owner_did.trim();
    let sender_did = sender_did.trim();
    if owner_did.is_empty() || sender_did.is_empty() || owner_did == sender_did {
        return Ok(String::new());
    }

    let local_handle = store::resolve_contact_handle_by_did(connection, owner_did, sender_did)?;
    if !local_handle.is_empty() {
        return Ok(local_handle);
    }

    let Some(lookup_handle_by_did) = lookup_handle_by_did else {
        return Ok(String::new());
    };
    let Some(remote_result) = lookup_handle_by_did(sender_did)? else {
        return Ok(String::new());
    };
    let handle = normalize_listener_handle(&remote_result);
    if handle.is_empty() {
        return Ok(String::new());
    }

    store::upsert_contact(
        connection,
        ContactRecord {
            owner_did: owner_did.to_string(),
            did: sender_did.to_string(),
            handle: handle.clone(),
            source_type: source_type.to_string(),
            source_group_id: source_group_id.to_string(),
            messaged: Some(true),
            first_seen_at: store::now_utc(),
            last_seen_at: store::now_utc(),
            ..ContactRecord::default()
        },
    )?;
    Ok(handle)
}

pub fn normalize_listener_handle(value: &str) -> String {
    let value = value.trim().to_lowercase();
    if value.is_empty() {
        return String::new();
    }
    let value = value.strip_prefix("wba://").unwrap_or(&value);
    match value.find('.') {
        Some(index) if index > 0 => value[..index].to_string(),
        _ => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::legacy_store::{ContactRecord, StoreResult};
    use rusqlite::Connection;
    use serde_json::Value;
    use time::format_description::well_known::Rfc3339;
    use time::OffsetDateTime;

    #[test]
    fn normalize_listener_handle_matches_go() {
        assert_eq!(normalize_listener_handle(" WBA://Alice.Example "), "alice");
        assert_eq!(normalize_listener_handle("bob"), "bob");
        assert_eq!(normalize_listener_handle(" .alice"), ".alice");
        assert_eq!(normalize_listener_handle(".alice"), ".alice");
        assert_eq!(normalize_listener_handle("  "), "");
    }

    #[test]
    fn empty_or_self_dids_skip_without_lookup_or_upsert() -> anyhow::Result<()> {
        let mut db = open_db()?;
        let mut calls = 0;
        let mut lookup = |_did: &str| -> anyhow::Result<Option<String>> {
            calls += 1;
            Ok(Some("alice".to_string()))
        };

        assert_eq!(
            sync_incoming_contact(
                &mut db,
                " ",
                "did:peer",
                "direct.incoming",
                "",
                Some(&mut lookup),
            )?,
            ""
        );
        assert_eq!(
            sync_incoming_contact(
                &mut db,
                "did:owner",
                " ",
                "direct.incoming",
                "",
                Some(&mut lookup),
            )?,
            ""
        );
        assert_eq!(
            sync_incoming_contact(
                &mut db,
                "did:owner",
                "did:owner",
                "direct.incoming",
                "",
                Some(&mut lookup),
            )?,
            ""
        );

        assert_eq!(calls, 0);
        assert_eq!(contact_count(&db)?, 0);
        Ok(())
    }

    #[test]
    fn local_contact_handle_wins_without_remote_lookup() -> anyhow::Result<()> {
        let mut db = open_db()?;
        store::upsert_contact(
            &mut db,
            ContactRecord {
                owner_did: "did:owner".to_string(),
                did: "did:peer".to_string(),
                handle: "LocalHandle".to_string(),
                source_type: "seed".to_string(),
                ..ContactRecord::default()
            },
        )?;
        let mut calls = 0;
        let mut lookup = |_did: &str| -> anyhow::Result<Option<String>> {
            calls += 1;
            Ok(Some("remote".to_string()))
        };

        assert_eq!(
            sync_incoming_contact(
                &mut db,
                " did:owner ",
                " did:peer ",
                "direct.incoming",
                "",
                Some(&mut lookup),
            )?,
            "LocalHandle"
        );
        assert_eq!(calls, 0);
        Ok(())
    }

    #[test]
    fn local_handle_binding_fallback_wins_without_remote_lookup() -> anyhow::Result<()> {
        let mut db = open_db()?;
        store::upsert_contact(
            &mut db,
            ContactRecord {
                owner_did: "did:owner".to_string(),
                did: "did:peer-old".to_string(),
                handle: "alice".to_string(),
                source_type: "seed".to_string(),
                ..ContactRecord::default()
            },
        )?;
        store::upsert_contact(
            &mut db,
            ContactRecord {
                owner_did: "did:owner".to_string(),
                did: "did:peer-new".to_string(),
                handle: "alice".to_string(),
                source_type: "seed".to_string(),
                ..ContactRecord::default()
            },
        )?;
        let mut calls = 0;
        let mut lookup = |_did: &str| -> anyhow::Result<Option<String>> {
            calls += 1;
            Ok(Some("remote".to_string()))
        };

        assert_eq!(
            sync_incoming_contact(
                &mut db,
                "did:owner",
                "did:peer-old",
                "group.incoming",
                "did:group",
                Some(&mut lookup),
            )?,
            "alice"
        );
        assert_eq!(calls, 0);
        Ok(())
    }

    #[test]
    fn missing_local_and_no_remote_is_noop() -> anyhow::Result<()> {
        let mut db = open_db()?;

        assert_eq!(
            sync_incoming_contact(
                &mut db,
                "did:owner",
                "did:peer",
                "direct.incoming",
                "",
                None,
            )?,
            ""
        );
        assert_eq!(contact_count(&db)?, 0);
        Ok(())
    }

    #[test]
    fn remote_error_returns_without_upsert() -> anyhow::Result<()> {
        let mut db = open_db()?;
        let mut lookup =
            |_did: &str| -> anyhow::Result<Option<String>> { anyhow::bail!("lookup boom") };

        let err = sync_incoming_contact(
            &mut db,
            "did:owner",
            "did:peer",
            "direct.incoming",
            "",
            Some(&mut lookup),
        )
        .expect_err("remote lookup error should propagate");
        assert_eq!(err.to_string(), "lookup boom");
        assert_eq!(contact_count(&db)?, 0);
        Ok(())
    }

    #[test]
    fn nil_or_blank_remote_handle_is_noop() -> anyhow::Result<()> {
        let mut db = open_db()?;
        let mut nil_lookup = |_did: &str| -> anyhow::Result<Option<String>> { Ok(None) };
        assert_eq!(
            sync_incoming_contact(
                &mut db,
                "did:owner",
                "did:peer",
                "direct.incoming",
                "",
                Some(&mut nil_lookup),
            )?,
            ""
        );

        let mut blank_lookup =
            |_did: &str| -> anyhow::Result<Option<String>> { Ok(Some("  ".to_string())) };
        assert_eq!(
            sync_incoming_contact(
                &mut db,
                "did:owner",
                "did:peer",
                "direct.incoming",
                "",
                Some(&mut blank_lookup),
            )?,
            ""
        );
        assert_eq!(contact_count(&db)?, 0);
        Ok(())
    }

    #[test]
    fn remote_lookup_upserts_direct_contact_with_normalized_handle() -> anyhow::Result<()> {
        let mut db = open_db()?;
        let mut lookup = |did: &str| -> anyhow::Result<Option<String>> {
            assert_eq!(did, "did:peer");
            Ok(Some(" WBA://Alice.Example ".to_string()))
        };

        assert_eq!(
            sync_incoming_contact(
                &mut db,
                " did:owner ",
                " did:peer ",
                "direct.incoming",
                "",
                Some(&mut lookup),
            )?,
            "alice"
        );

        let contact = store::get_contact_by_did(&db, "did:owner", "did:peer")?;
        assert_eq!(string_field(&contact, "owner_did"), "did:owner");
        assert_eq!(string_field(&contact, "did"), "did:peer");
        assert_eq!(string_field(&contact, "handle"), "alice");
        assert_eq!(string_field(&contact, "source_type"), "direct.incoming");
        assert!(contact["source_group_id"].is_null());
        assert_eq!(contact["messaged"].as_i64(), Some(1));
        assert_go_rfc3339_timestamp(string_field(&contact, "first_seen_at"));
        assert_go_rfc3339_timestamp(string_field(&contact, "last_seen_at"));
        assert_eq!(
            store::resolve_contact_handle_by_did(&db, "did:owner", "did:peer")?,
            "alice"
        );
        Ok(())
    }

    #[test]
    fn remote_lookup_preserves_group_source_metadata() -> anyhow::Result<()> {
        let mut db = open_db()?;
        let mut lookup =
            |_did: &str| -> anyhow::Result<Option<String>> { Ok(Some("Bob".to_string())) };

        assert_eq!(
            sync_incoming_contact(
                &mut db,
                "did:owner",
                "did:peer",
                "group.incoming",
                "did:group",
                Some(&mut lookup),
            )?,
            "bob"
        );

        let contact = store::get_contact_by_did(&db, "did:owner", "did:peer")?;
        assert_eq!(string_field(&contact, "source_type"), "group.incoming");
        assert_eq!(string_field(&contact, "source_group_id"), "did:group");
        assert_eq!(current_binding_did(&db, "did:owner", "bob")?, "did:peer");
        Ok(())
    }

    fn open_db() -> StoreResult<Connection> {
        let db = Connection::open_in_memory().expect("open sqlite memory db");
        store::ensure_schema(&db)?;
        Ok(db)
    }

    fn contact_count(connection: &Connection) -> StoreResult<i64> {
        Ok(connection.query_row("SELECT COUNT(*) FROM contacts", [], |row| row.get(0))?)
    }

    fn current_binding_did(
        connection: &Connection,
        owner_did: &str,
        handle: &str,
    ) -> StoreResult<String> {
        Ok(connection.query_row(
            "SELECT did FROM contact_handle_bindings WHERE owner_did = ?1 AND handle = ?2 AND is_current = 1",
            (owner_did, handle),
            |row| row.get(0),
        )?)
    }

    fn string_field<'a>(value: &'a Value, field: &str) -> &'a str {
        value
            .get(field)
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("missing string field {field}: {value:?}"))
    }

    fn assert_go_rfc3339_timestamp(value: &str) {
        assert_eq!(value.len(), "2026-05-14T11:38:35Z".len());
        assert!(value.ends_with('Z'));
        assert!(!value.contains('.'));
        OffsetDateTime::parse(value, &Rfc3339).expect("timestamp should parse as RFC3339");
    }
}
