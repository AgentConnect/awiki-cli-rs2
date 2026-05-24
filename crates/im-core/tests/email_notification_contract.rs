use im_core::prelude::*;
use rusqlite::Connection;
use tempfile::tempdir;

#[test]
fn email_notifications_are_owner_scoped_and_legacy_compatible() {
    let temp = tempdir().unwrap();
    let db_path = temp.path().join("state.sqlite3");
    let db = Connection::open(&db_path).unwrap();
    im_core::compat::local_state::ensure_schema(&db).unwrap();
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

    let page = im_core::compat::local_state::list_email_notifications_for_test(
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
