use awiki_im_core::prelude::*;
use rusqlite::Connection;
use serde_json::json;
use std::fs;
use tempfile::tempdir;

#[test]
fn email_notifications_are_owner_scoped_and_legacy_compatible() {
    let temp = tempdir().unwrap();
    let db_path = temp.path().join("state.sqlite3");
    let db = Connection::open(&db_path).unwrap();
    awiki_im_core::compat::local_state::ensure_schema(&db).unwrap();
    db.execute(
        r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, thread_id, direction, content_type, title, content, stored_at, metadata)
VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6, ?7, ?8, ?9)"#,
        (
            "mail-1",
            "alice-id",
            "did:example:alice",
            "mail:alice@example.com",
            "mail.notification",
            "[邮件] ",
            "legacy content",
            "2026-05-21T00:00:00Z",
            r#"{"mailbox_address":"alice@example.com","subject":"","from_addr":"sender@example.com","preview":"Preview","has_attachments":"yes"}"#,
        ),
    )
    .unwrap();
    db.execute(
        r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, thread_id, direction, content_type, title, content, stored_at, metadata)
VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6, ?7, ?8, ?9)"#,
        (
            "mail-other",
            "other-id",
            "did:example:other",
            "mail:other@example.com",
            "mail.notification",
            "Other",
            "other content",
            "2026-05-22T00:00:00Z",
            r#"{"source_kind":"mail"}"#,
        ),
    )
    .unwrap();

    let page = awiki_im_core::compat::local_state::list_email_notifications_for_test(
        &db_path,
        "alice-id",
        "did:example:alice",
        PageLimit::new(20).unwrap(),
    )
    .expect("notifications");

    assert_eq!(page.items.len(), 1);
    let item = &page.items[0];
    assert_eq!(item.id.as_str(), "mail-1");
    assert_eq!(
        item.mailbox_address.as_ref().map(EmailAddress::as_str),
        Some("alice@example.com")
    );
    assert_eq!(item.subject, "(no subject)");
    assert_eq!(item.from_addr.as_deref(), Some("sender@example.com"));
    assert_eq!(item.preview.as_deref(), Some("Preview"));
    assert!(item.has_attachments);
}

#[tokio::test]
async fn email_notifications_async_are_actor_backed_and_owner_scoped() {
    let temp = tempdir().unwrap();
    write_identity_fixture(temp.path());
    let core = ImCore::new(
        ImCoreConfig {
            service_base_url: ServiceEndpoint::parse("https://example.test").unwrap(),
            did_domain: "awiki.test".to_string(),
            user_service_endpoint: None,
            message_service_endpoint: None,
            mail_service_endpoint: None,
            anp_service_endpoint: None,
            anp_service_did: None,
            ca_bundle: None,
            transport_policy: MessageTransportPolicy::HttpOnly,
        },
        ImCorePaths {
            identities: IdentityRegistryPaths {
                identity_root_dir: temp.path().join("identities"),
                registry_path: temp.path().join("identities").join("registry.json"),
                default_identity_path: Some(temp.path().join("identities").join("default")),
            },
            local_state: LocalStatePaths {
                sqlite_path: temp.path().join("local").join("im.sqlite"),
            },
            runtime: RuntimePaths {
                cache_dir: temp.path().join("cache"),
                temp_dir: temp.path().join("tmp"),
            },
        },
    )
    .unwrap();
    let client = core.client_async(IdentitySelector::Default).await.unwrap();

    let db = Connection::open(temp.path().join("local").join("im.sqlite")).unwrap();
    awiki_im_core::compat::local_state::ensure_schema(&db).unwrap();
    db.execute(
        r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, thread_id, direction, content_type, title, content, stored_at, metadata)
VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6, ?7, ?8, ?9)"#,
        (
            "mail-async",
            "alice-id",
            "did:example:alice",
            "mail:alice@example.com",
            "mail.notification",
            "[邮件] Async subject",
            "legacy content",
            "2026-05-21T00:00:00Z",
            r#"{"mailbox_address":"alice@example.com","from_addr":"sender@example.com","preview":"Async preview","has_attachments":true}"#,
        ),
    )
    .unwrap();
    drop(db);

    let page = client
        .email()
        .notifications_async(EmailNotificationQuery {
            limit: PageLimit::new(20).unwrap(),
        })
        .await
        .unwrap();

    assert_eq!(page.items.len(), 1);
    let item = &page.items[0];
    assert_eq!(item.id.as_str(), "mail-async");
    assert_eq!(
        item.mailbox_address.as_ref().map(EmailAddress::as_str),
        Some("alice@example.com")
    );
    assert_eq!(item.subject, "Async subject");
    assert_eq!(item.from_addr.as_deref(), Some("sender@example.com"));
    assert_eq!(item.preview.as_deref(), Some("Async preview"));
    assert!(item.has_attachments);
}

fn write_identity_fixture(root: &std::path::Path) {
    let identity_root = root.join("identities");
    let identity_dir = identity_root.join("alice");
    fs::create_dir_all(&identity_dir).unwrap();
    fs::create_dir_all(root.join("local")).unwrap();
    fs::write(identity_root.join("default"), "alice\n").unwrap();
    fs::write(
        identity_root.join("registry.json"),
        json!({
            "default_identity": "alice",
            "identities": [{
                "id": "alice-id",
                "did": "did:example:alice",
                "local_alias": "alice",
                "handle": "alice.awiki.test",
                "ready_for_auth": true,
                "ready_for_messaging": true,
                "missing": []
            }]
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        identity_dir.join("did.json"),
        r#"{"id":"did:example:alice","controller":"did:example:alice"}"#,
    )
    .unwrap();
    fs::write(identity_dir.join("private.key"), "key\n").unwrap();
    fs::write(
        identity_dir.join("auth.json"),
        r#"{"jwt_token":"test-token"}"#,
    )
    .unwrap();
}
