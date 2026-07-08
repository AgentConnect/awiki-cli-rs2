use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use redb::{Database, TableDefinition};

use crate::messages::{ConversationListSnapshot, ConversationSnapshotItem};

const SNAPSHOT_TABLE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("conversation_snapshots_v1");
const SNAPSHOT_FILE_NAME: &str = "conversation_snapshots.redb";
const FORMAT_VERSION: u32 = 1;
const MAX_SNAPSHOT_ITEMS: usize = 200;

#[derive(Debug, Clone)]
pub(crate) struct ConversationSnapshotInput {
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) im_schema_version: i64,
    pub(crate) unread_total: u32,
    pub(crate) items: Vec<ConversationSnapshotItem>,
}

pub(crate) fn load_for_client(
    client: &crate::core::ImClient,
) -> crate::ImResult<Option<ConversationListSnapshot>> {
    load_for_owner(
        snapshot_path(client.core_inner().sdk_paths()),
        client.current_identity().id.as_str(),
        client.did().as_str(),
        crate::internal::local_state::schema::SCHEMA_VERSION,
    )
}

pub(crate) fn save_for_client(
    client: &crate::core::ImClient,
    mut items: Vec<ConversationSnapshotItem>,
) -> crate::ImResult<()> {
    if items.len() > MAX_SNAPSHOT_ITEMS {
        items.truncate(MAX_SNAPSHOT_ITEMS);
    }
    let unread_total = items
        .iter()
        .map(|item| item.unread_count)
        .fold(0u32, u32::saturating_add);
    save(
        snapshot_path(client.core_inner().sdk_paths()),
        ConversationSnapshotInput {
            owner_identity_id: client.current_identity().id.as_str().to_owned(),
            owner_did: client.did().as_str().to_owned(),
            im_schema_version: crate::internal::local_state::schema::SCHEMA_VERSION,
            unread_total,
            items,
        },
    )
}

pub(crate) fn clear_for_client(client: &crate::core::ImClient) -> crate::ImResult<()> {
    clear_for_owner(
        snapshot_path(client.core_inner().sdk_paths()),
        client.current_identity().id.as_str(),
    )
}

pub(crate) fn load_for_owner(
    path: PathBuf,
    owner_identity_id: &str,
    owner_did: &str,
    expected_im_schema_version: i64,
) -> crate::ImResult<Option<ConversationListSnapshot>> {
    let owner_identity_id = require_non_empty("owner_identity_id", owner_identity_id)?;
    let owner_did = require_non_empty("owner_did", owner_did)?;
    if !path.exists() {
        return Ok(None);
    }
    let Some(bytes) = read_payload(&path, &owner_identity_id)? else {
        return Ok(None);
    };
    let snapshot: ConversationListSnapshot = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => {
            let _ = clear_for_owner(path, &owner_identity_id);
            return Ok(None);
        }
    };
    if snapshot.format_version != FORMAT_VERSION
        || snapshot.im_schema_version != expected_im_schema_version
        || snapshot.owner_identity_id != owner_identity_id
        || snapshot.owner_did != owner_did
    {
        let _ = clear_for_owner(path, &owner_identity_id);
        return Ok(None);
    }
    Ok(Some(snapshot))
}

pub(crate) fn save(path: PathBuf, input: ConversationSnapshotInput) -> crate::ImResult<()> {
    let owner_identity_id = require_non_empty("owner_identity_id", &input.owner_identity_id)?;
    let owner_did = require_non_empty("owner_did", &input.owner_did)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let snapshot = ConversationListSnapshot {
        format_version: FORMAT_VERSION,
        im_schema_version: input.im_schema_version,
        owner_identity_id: owner_identity_id.clone(),
        owner_did,
        generated_at_ms: now_millis(),
        summary_version: None,
        unread_total: input.unread_total,
        items: input.items,
    };
    let bytes = serde_json::to_vec(&snapshot).map_err(|err| crate::ImError::Serialization {
        detail: err.to_string(),
    })?;
    let db = open_or_create(&path)?;
    let write_txn = db.begin_write().map_err(snapshot_unavailable)?;
    {
        let mut table = write_txn
            .open_table(SNAPSHOT_TABLE)
            .map_err(snapshot_unavailable)?;
        table
            .insert(owner_identity_id.as_str(), bytes.as_slice())
            .map_err(snapshot_unavailable)?;
    }
    write_txn.commit().map_err(snapshot_unavailable)?;
    Ok(())
}

pub(crate) fn clear_for_owner(path: PathBuf, owner_identity_id: &str) -> crate::ImResult<()> {
    let owner_identity_id = require_non_empty("owner_identity_id", owner_identity_id)?;
    if !path.exists() {
        return Ok(());
    }
    let db = match Database::open(&path) {
        Ok(db) => db,
        Err(_) => {
            remove_file_if_exists(&path)?;
            return Ok(());
        }
    };
    let write_txn = db.begin_write().map_err(snapshot_unavailable)?;
    {
        let mut table = write_txn
            .open_table(SNAPSHOT_TABLE)
            .map_err(snapshot_unavailable)?;
        let _ = table
            .remove(owner_identity_id.as_str())
            .map_err(snapshot_unavailable)?;
    }
    write_txn.commit().map_err(snapshot_unavailable)?;
    Ok(())
}

fn read_payload(path: &Path, owner_identity_id: &str) -> crate::ImResult<Option<Vec<u8>>> {
    let db = match Database::open(path) {
        Ok(db) => db,
        Err(_) => {
            remove_file_if_exists(path)?;
            return Ok(None);
        }
    };
    let read_txn = match db.begin_read() {
        Ok(txn) => txn,
        Err(_) => {
            remove_file_if_exists(path)?;
            return Ok(None);
        }
    };
    let table = match read_txn.open_table(SNAPSHOT_TABLE) {
        Ok(table) => table,
        Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
        Err(_) => {
            remove_file_if_exists(path)?;
            return Ok(None);
        }
    };
    match table.get(owner_identity_id) {
        Ok(Some(value)) => Ok(Some(value.value().to_vec())),
        Ok(None) => Ok(None),
        Err(_) => {
            remove_file_if_exists(path)?;
            Ok(None)
        }
    }
}

fn open_or_create(path: &Path) -> crate::ImResult<Database> {
    match Database::open(path) {
        Ok(db) => Ok(db),
        Err(redb::DatabaseError::Storage(redb::StorageError::Io(err)))
            if err.kind() == std::io::ErrorKind::NotFound =>
        {
            Database::create(path).map_err(snapshot_unavailable)
        }
        Err(err) if is_corrupt_database_error(&err) => {
            remove_file_if_exists(path)?;
            Database::create(path).map_err(snapshot_unavailable)
        }
        Err(err) => Err(snapshot_unavailable(err)),
    }
}

fn is_corrupt_database_error(err: &redb::DatabaseError) -> bool {
    matches!(
        err,
        redb::DatabaseError::Storage(redb::StorageError::Corrupted(_))
    )
}

fn snapshot_path(paths: &crate::paths::ImCorePaths) -> PathBuf {
    paths.runtime.cache_dir.join(SNAPSHOT_FILE_NAME)
}

fn require_non_empty(field: &'static str, value: &str) -> crate::ImResult<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            format!("{field} is required"),
        ));
    }
    Ok(value.to_owned())
}

fn remove_file_if_exists(path: &Path) -> crate::ImResult<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

fn snapshot_unavailable(err: impl std::fmt::Display) -> crate::ImError {
    crate::ImError::LocalStateUnavailable {
        detail: format!("conversation snapshot cache unavailable: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_roundtrip_loads_matching_owner_and_schema() {
        let path = unique_path("roundtrip");
        let item = item("msg-1");
        save(
            path.clone(),
            ConversationSnapshotInput {
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:example:alice".to_owned(),
                im_schema_version: 20,
                unread_total: 2,
                items: vec![item.clone()],
            },
        )
        .unwrap();

        let loaded = load_for_owner(path, "alice-id", "did:example:alice", 20)
            .unwrap()
            .unwrap();

        assert_eq!(loaded.owner_identity_id, "alice-id");
        assert_eq!(loaded.unread_total, 2);
        assert_eq!(loaded.items, vec![item]);
    }

    #[test]
    fn snapshot_owner_or_schema_mismatch_discards_payload() {
        let path = unique_path("mismatch");
        save(
            path.clone(),
            ConversationSnapshotInput {
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:example:alice".to_owned(),
                im_schema_version: 20,
                unread_total: 0,
                items: vec![item("msg-1")],
            },
        )
        .unwrap();

        assert!(
            load_for_owner(path.clone(), "alice-id", "did:example:alice", 21)
                .unwrap()
                .is_none()
        );
        assert!(load_for_owner(path, "alice-id", "did:example:alice", 20)
            .unwrap()
            .is_none());
    }

    #[test]
    fn snapshot_corrupt_file_is_cache_miss() {
        let path = unique_path("corrupt");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"not a redb database").unwrap();

        let loaded = load_for_owner(path.clone(), "alice-id", "did:example:alice", 20).unwrap();

        assert!(loaded.is_none());
        assert!(!path.exists());
    }

    #[test]
    fn snapshot_clear_removes_only_owner_payload() {
        let path = unique_path("clear");
        save(
            path.clone(),
            ConversationSnapshotInput {
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:example:alice".to_owned(),
                im_schema_version: 20,
                unread_total: 0,
                items: vec![item("alice-msg")],
            },
        )
        .unwrap();
        save(
            path.clone(),
            ConversationSnapshotInput {
                owner_identity_id: "bob-id".to_owned(),
                owner_did: "did:example:bob".to_owned(),
                im_schema_version: 20,
                unread_total: 0,
                items: vec![item("bob-msg")],
            },
        )
        .unwrap();

        clear_for_owner(path.clone(), "alice-id").unwrap();

        assert!(
            load_for_owner(path.clone(), "alice-id", "did:example:alice", 20)
                .unwrap()
                .is_none()
        );
        assert!(load_for_owner(path, "bob-id", "did:example:bob", 20)
            .unwrap()
            .is_some());
    }

    fn item(message_id: &str) -> ConversationSnapshotItem {
        ConversationSnapshotItem {
            thread_kind: "direct".to_owned(),
            thread_id: "did:example:bob".to_owned(),
            conversation_identity: None,
            participants: vec!["did:example:bob".to_owned()],
            last_message: Some(crate::messages::ConversationSnapshotMessage {
                id: message_id.to_owned(),
                thread_kind: "direct".to_owned(),
                thread_id: "did:example:bob".to_owned(),
                conversation_identity: None,
                direction: "incoming".to_owned(),
                sender: "did:example:bob".to_owned(),
                receiver: Some("did:example:alice".to_owned()),
                group: None,
                body: crate::messages::ConversationSnapshotMessageBody {
                    text: Some("hello".to_owned()),
                    kind: Some("Text".to_owned()),
                    payload_json: None,
                    unsupported_content_type: None,
                },
                sent_at: Some("2026-06-27T00:00:00Z".to_owned()),
                received_at: None,
                server_sequence: Some(10),
                content_type: Some("text/plain".to_owned()),
                attributes: Vec::new(),
            }),
            unread_count: 2,
            unread_mention_count: 0,
            first_unread_mention_message_id: None,
            message_count: 3,
            last_message_at: Some("2026-06-27T00:00:00Z".to_owned()),
        }
    }

    fn unique_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir()
            .join(format!(
                "im-core-conversation-snapshot-{name}-{}-{nanos}",
                std::process::id()
            ))
            .join(SNAPSHOT_FILE_NAME)
    }
}
