use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;

const DEFAULT_LIMIT: u32 = 200;
const PATCH_BUFFER: usize = 128;

fn subscribe_before_seed<T: Clone, F>(
    sender: &broadcast::Sender<T>,
    projection: &Mutex<()>,
    seed: F,
) -> crate::ImResult<(broadcast::Receiver<T>, Vec<T>)>
where
    F: FnOnce() -> crate::ImResult<Vec<T>>,
{
    let receiver = sender.subscribe();
    let _projection_guard = projection
        .lock()
        .expect("conversation projection lock poisoned");
    let initial = seed()?;
    Ok((receiver, initial))
}

#[derive(Debug)]
pub(crate) struct ConversationStore {
    owner_identity_id: String,
    owner_did: String,
    projection: Mutex<()>,
    state: Mutex<ConversationStoreState>,
    sender: broadcast::Sender<crate::messages::ConversationStorePatch>,
}

#[derive(Debug, Default)]
struct ConversationStoreState {
    version: u64,
    items: Vec<crate::messages::ConversationSnapshotItem>,
    unread_total: u32,
}

impl ConversationStore {
    pub(crate) fn new_for_client(client: &crate::core::ImClient) -> Arc<Self> {
        let (sender, _) = broadcast::channel(PATCH_BUFFER);
        Arc::new(Self {
            owner_identity_id: client.current_identity().id.as_str().to_owned(),
            owner_did: client.did().as_str().to_owned(),
            projection: Mutex::new(()),
            state: Mutex::new(ConversationStoreState::default()),
            sender,
        })
    }

    pub(crate) fn watch_for_client(
        self: &Arc<Self>,
        client: &crate::core::ImClient,
    ) -> crate::ImResult<crate::messages::ConversationPatchSession> {
        self.ensure_owner(client)?;
        let (receiver, initial) = subscribe_before_seed(&self.sender, &self.projection, || {
            Ok(vec![self.repair_from_client_locked(client)?])
        })?;
        Ok(crate::messages::ConversationPatchSession::new(
            self.clone(),
            receiver,
            initial,
        ))
    }

    pub(crate) fn repair_from_client(
        &self,
        client: &crate::core::ImClient,
    ) -> crate::ImResult<crate::messages::ConversationStorePatch> {
        self.ensure_owner(client)?;
        let _projection_guard = self
            .projection
            .lock()
            .expect("conversation projection lock poisoned");
        self.repair_from_client_locked(client)
    }

    fn repair_from_client_locked(
        &self,
        client: &crate::core::ImClient,
    ) -> crate::ImResult<crate::messages::ConversationStorePatch> {
        let items = committed_items(client)?;
        let unread_total = unread_total(&items);
        Ok(self.replace_items(items, unread_total))
    }

    pub(crate) fn on_committed_sync_invalidation(
        &self,
        client: &crate::core::ImClient,
        invalidation: &crate::internal::local_state::sync_state::SyncDeltaInvalidation,
    ) {
        if self.ensure_owner(client).is_err()
            || invalidation.owner_identity_id != self.owner_identity_id
            || invalidation.owner_did != self.owner_did
            || !invalidation.has_changes()
        {
            return;
        }
        let _projection_guard = self
            .projection
            .lock()
            .expect("conversation projection lock poisoned");
        let patch = match committed_items(client) {
            Ok(items) => self.diff_committed_items(items),
            Err(_) => Some(self.repair_required_patch("committed_projection_unavailable")),
        };
        if let Some(patch) = patch {
            let _ = self.sender.send(patch);
        }
    }

    pub(crate) fn on_committed_local_projection(
        &self,
        client: &crate::core::ImClient,
        reason: &str,
    ) {
        if self.ensure_owner(client).is_err() {
            return;
        }
        let _projection_guard = self
            .projection
            .lock()
            .expect("conversation projection lock poisoned");
        let patch = match committed_items(client) {
            Ok(items) => self.diff_committed_items(items),
            Err(_) => Some(self.repair_required_patch(reason)),
        };
        if let Some(patch) = patch {
            let _ = self.sender.send(patch);
        }
    }

    pub(crate) fn repair_required_patch(
        &self,
        reason: &str,
    ) -> crate::messages::ConversationStorePatch {
        let mut state = self.state.lock().expect("conversation store lock poisoned");
        state.version = state.version.saturating_add(1);
        crate::messages::ConversationStorePatch::RepairRequired {
            owner_identity_id: self.owner_identity_id.clone(),
            owner_did: self.owner_did.clone(),
            version: state.version,
            unread_total: state.unread_total,
            reason: reason.to_owned(),
        }
    }

    #[cfg(test)]
    pub(crate) fn version_for_test(&self) -> u64 {
        self.state
            .lock()
            .expect("conversation store lock poisoned")
            .version
    }

    fn replace_items(
        &self,
        items: Vec<crate::messages::ConversationSnapshotItem>,
        unread_total: u32,
    ) -> crate::messages::ConversationStorePatch {
        let mut state = self.state.lock().expect("conversation store lock poisoned");
        state.version = state.version.saturating_add(1);
        state.items = items.clone();
        state.unread_total = unread_total;
        crate::messages::ConversationStorePatch::Reset {
            owner_identity_id: self.owner_identity_id.clone(),
            owner_did: self.owner_did.clone(),
            version: state.version,
            unread_total,
            items,
        }
    }

    fn diff_committed_items(
        &self,
        next_items: Vec<crate::messages::ConversationSnapshotItem>,
    ) -> Option<crate::messages::ConversationStorePatch> {
        let next_unread_total = unread_total(&next_items);
        let mut state = self.state.lock().expect("conversation store lock poisoned");
        if state.items == next_items {
            return None;
        }
        state.version = state.version.saturating_add(1);
        let version = state.version;
        let previous_items = std::mem::replace(&mut state.items, next_items.clone());
        state.unread_total = next_unread_total;

        let patch = diff_patch(
            &self.owner_identity_id,
            &self.owner_did,
            version,
            next_unread_total,
            &previous_items,
            &next_items,
        );
        Some(
            patch.unwrap_or_else(|| crate::messages::ConversationStorePatch::Reset {
                owner_identity_id: self.owner_identity_id.clone(),
                owner_did: self.owner_did.clone(),
                version,
                unread_total: next_unread_total,
                items: next_items,
            }),
        )
    }

    fn ensure_owner(&self, client: &crate::core::ImClient) -> crate::ImResult<()> {
        if client.current_identity().id.as_str() != self.owner_identity_id
            || client.did().as_str() != self.owner_did
        {
            return Err(crate::ImError::invalid_input(
                Some("client".to_owned()),
                "conversation store owner does not match client identity",
            ));
        }
        Ok(())
    }
}

fn diff_patch(
    owner_identity_id: &str,
    owner_did: &str,
    version: u64,
    unread_total: u32,
    previous: &[crate::messages::ConversationSnapshotItem],
    next: &[crate::messages::ConversationSnapshotItem],
) -> Option<crate::messages::ConversationStorePatch> {
    if previous == next {
        return None;
    }
    let previous_keys = previous.iter().map(item_key).collect::<Vec<_>>();
    let next_keys = next.iter().map(item_key).collect::<Vec<_>>();
    let removed = previous_keys
        .iter()
        .filter(|key| !next_keys.contains(key))
        .collect::<Vec<_>>();
    let added_or_changed = next
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            let key = item_key(item);
            previous.iter().find(|previous| item_key(previous) == key) != Some(*item)
        })
        .collect::<Vec<_>>();

    if removed.len() == 1 && added_or_changed.is_empty() {
        return Some(crate::messages::ConversationStorePatch::Remove {
            owner_identity_id: owner_identity_id.to_owned(),
            owner_did: owner_did.to_owned(),
            version,
            unread_total,
            conversation_id: removed[0].clone(),
        });
    }
    if removed.is_empty() && added_or_changed.len() == 1 {
        let (index, item) = added_or_changed[0];
        return Some(crate::messages::ConversationStorePatch::Upsert {
            owner_identity_id: owner_identity_id.to_owned(),
            owner_did: owner_did.to_owned(),
            version,
            unread_total,
            item: item.clone(),
            index: u32::try_from(index).unwrap_or(u32::MAX),
        });
    }
    None
}

fn item_key(item: &crate::messages::ConversationSnapshotItem) -> String {
    item.conversation_id.clone()
}

fn unread_total(items: &[crate::messages::ConversationSnapshotItem]) -> u32 {
    items
        .iter()
        .fold(0u32, |sum, item| sum.saturating_add(item.unread_count))
}

fn committed_items(
    client: &crate::core::ImClient,
) -> crate::ImResult<Vec<crate::messages::ConversationSnapshotItem>> {
    let page =
        crate::internal::message_runtime::conversations::MessageConversationRuntime::new(client)
            .conversations(crate::messages::ConversationQuery {
                limit: crate::ids::PageLimit(DEFAULT_LIMIT),
                cursor: None,
                include_groups: true,
                include_direct: true,
                unread_only: false,
            })?;
    Ok(page
        .items
        .iter()
        .map(crate::internal::message_runtime::conversations::snapshot_item_from_conversation)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn subscribe_before_seed_keeps_exactly_one_commit_during_seed() {
        let (sender, _) = broadcast::channel(4);
        let projection = Mutex::new(());
        let (mut receiver, initial) = subscribe_before_seed(&sender, &projection, || {
            sender.send("committed").unwrap();
            Ok(vec!["seed"])
        })
        .unwrap();

        assert_eq!(initial, vec!["seed"]);
        assert_eq!(receiver.recv().await.unwrap(), "committed");
        assert!(matches!(
            receiver.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test]
    async fn seed_version_fences_the_commit_already_represented_by_seed() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let store = ConversationStore::new_for_client(&client);
        let receiver = store.sender.subscribe();
        let during_seed = store.repair_required_patch("during_seed");
        store.sender.send(during_seed).unwrap();
        let seed = store.repair_required_patch("seed_after_commit");
        let mut session =
            crate::messages::ConversationPatchSession::new(store.clone(), receiver, vec![seed]);

        assert!(matches!(
            session.next_patch().await,
            Some(crate::messages::ConversationStorePatch::RepairRequired {
                version: 2,
                reason,
                ..
            }) if reason == "seed_after_commit"
        ));
        let after_seed = store.repair_required_patch("after_seed");
        store.sender.send(after_seed).unwrap();
        assert!(matches!(
            session.next_patch().await,
            Some(crate::messages::ConversationStorePatch::RepairRequired {
                version: 3,
                reason,
                ..
            }) if reason == "after_seed"
        ));
    }

    #[tokio::test]
    async fn watch_ignores_stale_redb_and_delivers_one_canonical_initial_reset() {
        let fixture = Fixture::new();
        let client = fixture.client();
        crate::internal::message_runtime::local_projection::persist_direct_outgoing_result(
            &client,
            "did:example:bob",
            None,
            None,
            &send_result("canonical-live"),
        )
        .unwrap();
        crate::internal::snapshot::conversation_snapshot::save_for_client(
            &client,
            vec![item(
                "dm:persona:stale",
                "direct",
                "did:example:stale",
                "stale-redb",
                1,
            )],
        )
        .unwrap();
        assert_eq!(
            crate::internal::snapshot::conversation_snapshot::load_for_client(&client)
                .unwrap()
                .unwrap()
                .items[0]
                .last_message
                .as_ref()
                .unwrap()
                .id,
            "stale-redb"
        );

        let mut session = client.messages().watch_conversation_patches().unwrap();
        let initial = session.next_patch().await.expect("canonical initial reset");

        assert!(matches!(
            initial,
            crate::messages::ConversationStorePatch::Reset { items, .. }
                if items.len() == 1
                    && items[0].last_message.as_ref().map(|message| message.id.as_str())
                        == Some("canonical-live")
        ));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), session.next_patch())
                .await
                .is_err(),
            "bound watch must not deliver a second redb-derived initial reset"
        );
    }

    #[tokio::test]
    async fn seed_first_serializes_commit_projection_without_regression() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let store = ConversationStore::new_for_client(&client);
        let old_items = vec![item("dm:persona:a", "direct", "a", "old", 1)];
        let committed_items = vec![item("dm:persona:a", "direct", "a", "new", 0)];
        let canonical = Arc::new(Mutex::new(old_items.clone()));
        let seed_read = Arc::new(std::sync::Barrier::new(2));
        let release_seed = Arc::new(std::sync::Barrier::new(2));

        let seed_store = store.clone();
        let seed_canonical = canonical.clone();
        let seed_read_worker = seed_read.clone();
        let release_seed_worker = release_seed.clone();
        let seed_worker = std::thread::spawn(move || {
            subscribe_before_seed(&seed_store.sender, &seed_store.projection, || {
                let items = seed_canonical
                    .lock()
                    .expect("canonical conversation fixture lock poisoned")
                    .clone();
                seed_read_worker.wait();
                release_seed_worker.wait();
                let unread_total = unread_total(&items);
                Ok(vec![seed_store.replace_items(items, unread_total)])
            })
            .unwrap()
        });

        seed_read.wait();
        *canonical
            .lock()
            .expect("canonical conversation fixture lock poisoned") = committed_items.clone();

        let callback_store = store.clone();
        let callback_canonical = canonical.clone();
        let (callback_done_tx, callback_done_rx) = std::sync::mpsc::channel();
        let callback_worker = std::thread::spawn(move || {
            let _projection_guard = callback_store
                .projection
                .lock()
                .expect("conversation projection lock poisoned");
            let items = callback_canonical
                .lock()
                .expect("canonical conversation fixture lock poisoned")
                .clone();
            if let Some(patch) = callback_store.diff_committed_items(items) {
                let _ = callback_store.sender.send(patch);
            }
            callback_done_tx.send(()).unwrap();
        });

        assert!(
            callback_done_rx
                .recv_timeout(std::time::Duration::from_millis(50))
                .is_err(),
            "commit callback must not overtake a seed holding the projection lock"
        );
        release_seed.wait();
        let (receiver, initial) = seed_worker.join().unwrap();
        callback_done_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        callback_worker.join().unwrap();

        let mut session =
            crate::messages::ConversationPatchSession::new(store.clone(), receiver, initial);
        assert!(matches!(
            session.next_patch().await,
            Some(crate::messages::ConversationStorePatch::Reset { items, .. })
                if items == old_items
        ));
        assert!(matches!(
            session.next_patch().await,
            Some(crate::messages::ConversationStorePatch::Upsert { item, .. })
                if item == committed_items[0]
        ));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), session.next_patch())
                .await
                .is_err(),
            "the commit represented after the seed must be delivered exactly once"
        );
        assert_eq!(
            store
                .state
                .lock()
                .expect("conversation store lock poisoned")
                .items,
            committed_items,
            "the callback must re-read canonical state after the seed and must not regress it"
        );
    }

    #[tokio::test]
    async fn callback_first_is_fenced_by_newer_seed_without_duplicate_delivery() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let store = ConversationStore::new_for_client(&client);
        let committed_items = vec![item("dm:persona:a", "direct", "a", "new", 0)];
        let canonical = Arc::new(Mutex::new(committed_items.clone()));
        let receiver = store.sender.subscribe();
        let callback_locked = Arc::new(std::sync::Barrier::new(2));
        let release_callback = Arc::new(std::sync::Barrier::new(2));

        let callback_store = store.clone();
        let callback_canonical = canonical.clone();
        let callback_locked_worker = callback_locked.clone();
        let release_callback_worker = release_callback.clone();
        let callback_worker = std::thread::spawn(move || {
            let _projection_guard = callback_store
                .projection
                .lock()
                .expect("conversation projection lock poisoned");
            let items = callback_canonical
                .lock()
                .expect("canonical conversation fixture lock poisoned")
                .clone();
            if let Some(patch) = callback_store.diff_committed_items(items) {
                let _ = callback_store.sender.send(patch);
            }
            callback_locked_worker.wait();
            release_callback_worker.wait();
        });

        callback_locked.wait();
        let seed_store = store.clone();
        let seed_canonical = canonical.clone();
        let seed_started = Arc::new(std::sync::Barrier::new(2));
        let seed_started_worker = seed_started.clone();
        let seed_worker = std::thread::spawn(move || {
            seed_started_worker.wait();
            let _projection_guard = seed_store
                .projection
                .lock()
                .expect("conversation projection lock poisoned");
            let items = seed_canonical
                .lock()
                .expect("canonical conversation fixture lock poisoned")
                .clone();
            let unread_total = unread_total(&items);
            vec![seed_store.replace_items(items, unread_total)]
        });
        seed_started.wait();
        release_callback.wait();
        callback_worker.join().unwrap();
        let initial = seed_worker.join().unwrap();

        let mut session =
            crate::messages::ConversationPatchSession::new(store.clone(), receiver, initial);
        assert!(matches!(
            session.next_patch().await,
            Some(crate::messages::ConversationStorePatch::Reset {
                version: 2,
                items,
                ..
            }) if items == committed_items
        ));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), session.next_patch())
                .await
                .is_err(),
            "the older queued callback patch must be fenced by the seed"
        );
        assert_eq!(
            store
                .state
                .lock()
                .expect("conversation store lock poisoned")
                .items,
            committed_items,
            "a callback that wins the projection lock must remain represented by the seed"
        );
    }

    #[test]
    fn conversation_store_diff_emits_upsert_for_single_changed_item() {
        let previous = vec![item("dm:persona:a", "direct", "a-old-did", "old", 1)];
        let next = vec![item("dm:persona:a", "direct", "a-new-did", "new", 0)];

        let patch = diff_patch("owner-id", "did:example:owner", 2, 0, &previous, &next)
            .expect("single upsert patch");

        match patch {
            crate::messages::ConversationStorePatch::Upsert {
                owner_identity_id,
                owner_did,
                version,
                unread_total,
                item,
                index,
            } => {
                assert_eq!(owner_identity_id, "owner-id");
                assert_eq!(owner_did, "did:example:owner");
                assert_eq!(version, 2);
                assert_eq!(unread_total, 0);
                assert_eq!(index, 0);
                assert_eq!(item.last_message.unwrap().id, "new");
            }
            other => panic!("expected upsert, got {other:?}"),
        }
    }

    #[test]
    fn conversation_store_diff_emits_remove_for_single_removed_item() {
        let previous = vec![item("dm:persona:a", "direct", "a", "old", 1)];
        let next = Vec::new();

        let patch = diff_patch("owner-id", "did:example:owner", 3, 0, &previous, &next)
            .expect("single remove patch");

        match patch {
            crate::messages::ConversationStorePatch::Remove {
                conversation_id,
                version,
                ..
            } => {
                assert_eq!(version, 3);
                assert_eq!(conversation_id, "dm:persona:a");
            }
            other => panic!("expected remove, got {other:?}"),
        }
    }

    #[test]
    fn conversation_store_diff_falls_back_to_reset_for_multi_item_change() {
        let previous = vec![item("dm:persona:a", "direct", "a", "old", 1)];
        let next = vec![
            item("dm:persona:a", "direct", "a", "new", 0),
            item("dm:persona:b", "direct", "b", "other", 1),
        ];

        assert!(
            diff_patch("owner-id", "did:example:owner", 4, 1, &previous, &next).is_none(),
            "multi item changes require reset"
        );
    }

    #[test]
    fn conversation_store_diff_has_no_patch_for_unchanged_items() {
        let items = vec![item("dm:persona:a", "direct", "a", "same", 1)];

        assert!(
            diff_patch("owner-id", "did:example:owner", 4, 1, &items, &items,).is_none(),
            "unchanged items must not produce a patch"
        );
    }

    #[tokio::test]
    async fn conversation_patch_session_reports_repair_required_on_lag() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let store = ConversationStore::new_for_client(&client);
        let mut session = crate::messages::ConversationPatchSession::new(
            store.clone(),
            store.sender.subscribe(),
            Vec::new(),
        );
        for index in 0..(PATCH_BUFFER + 1) {
            let _ = store
                .sender
                .send(store.repair_required_patch(&format!("overflow-{index}")));
        }

        let patch = session.next_patch().await.expect("repair patch");

        assert!(matches!(
            patch,
            crate::messages::ConversationStorePatch::RepairRequired { .. }
        ));
    }

    #[tokio::test]
    async fn conversation_store_emits_patch_after_local_send_projection_commit() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let mut session = client.messages().watch_conversation_patches().unwrap();
        let _initial_hydrate = session.next_patch().await.unwrap();
        let sdk_result = send_result("msg-local-send");
        crate::internal::message_runtime::local_projection::persist_direct_outgoing_result(
            &client,
            "did:example:bob",
            None,
            None,
            &sdk_result,
        )
        .unwrap();

        client.emit_committed_conversation_projection("local_send");
        let patch = session.next_patch().await.unwrap();

        match patch {
            crate::messages::ConversationStorePatch::Upsert {
                owner_identity_id,
                owner_did,
                item,
                ..
            } => {
                assert_eq!(owner_identity_id, "alice-id");
                assert_eq!(owner_did, "did:example:alice");
                assert_eq!(item.thread_id, "did:example:bob");
                assert_eq!(item.last_message.unwrap().id, "msg-local-send");
            }
            other => panic!("expected local send upsert patch, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn conversation_store_ignores_unchanged_local_projection_commit() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let store = client.conversation_store();
        let mut session = client.messages().watch_conversation_patches().unwrap();
        let _initial_hydrate = session.next_patch().await.unwrap();
        let version_before = store.version_for_test();

        client.emit_committed_conversation_projection("unchanged_projection");

        assert_eq!(store.version_for_test(), version_before);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), session.next_patch())
                .await
                .is_err(),
            "unchanged projection must not emit a patch"
        );
    }

    fn item(
        conversation_id: &str,
        thread_kind: &str,
        thread_id: &str,
        message_id: &str,
        unread_count: u32,
    ) -> crate::messages::ConversationSnapshotItem {
        crate::messages::ConversationSnapshotItem {
            conversation_id: conversation_id.to_owned(),
            peer_persona_id: None,
            canonical_group_did: None,
            resolution_state: crate::messages::ConversationResolutionState::LegacyUnresolved,
            thread_kind: thread_kind.to_owned(),
            thread_id: thread_id.to_owned(),
            title: None,
            conversation_identity: None,
            participants: Vec::new(),
            last_message: Some(crate::messages::ConversationSnapshotMessage {
                id: message_id.to_owned(),
                thread_kind: thread_kind.to_owned(),
                thread_id: thread_id.to_owned(),
                conversation_identity: None,
                direction: "incoming".to_owned(),
                sender: "did:example:bob".to_owned(),
                receiver: Some("did:example:alice".to_owned()),
                group: None,
                body: crate::messages::ConversationSnapshotMessageBody {
                    text: Some(message_id.to_owned()),
                    kind: Some("text".to_owned()),
                    payload_json: None,
                    unsupported_content_type: None,
                },
                sent_at: Some("2026-06-27T00:00:00Z".to_owned()),
                received_at: None,
                server_sequence: Some(1),
                content_type: Some("text/plain".to_owned()),
                attributes: Vec::new(),
            }),
            unread_count,
            unread_mention_count: 0,
            first_unread_mention_message_id: None,
            message_count: 1,
            last_message_at: Some("2026-06-27T00:00:00Z".to_owned()),
            activity_at: Some("2026-06-27T00:00:00Z".to_owned()),
        }
    }

    fn send_result(message_id: &str) -> crate::messages::SendMessageResult {
        crate::messages::SendMessageResult {
            message: crate::messages::Message {
                id: crate::ids::MessageId::parse(message_id).unwrap(),
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                direction: crate::messages::MessageDirection::Outgoing,
                sender: crate::ids::PeerRef::parse("did:example:alice", "").unwrap(),
                receiver: Some(crate::ids::PeerRef::parse("did:example:bob", "").unwrap()),
                group: None,
                body: crate::messages::MessageBodyView::Text {
                    text: "sent".to_owned(),
                    kind: crate::messages::MessageKind::Text,
                },
                sent_at: Some("2026-06-27T00:00:00Z".to_owned()),
                received_at: None,
                metadata: crate::messages::MessageMetadata {
                    server_sequence: Some(42),
                    ..crate::messages::MessageMetadata::default()
                },
            },
            delivery: crate::messages::DeliveryState::Accepted,
            warnings: Vec::new(),
        }
    }

    struct Fixture {
        root: tempfile::TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let identities = root.path().join("identities");
            std::fs::create_dir_all(&identities).unwrap();
            std::fs::create_dir_all(root.path().join("local")).unwrap();
            std::fs::write(
                identities.join("registry.json"),
                r#"{
                  "identities": [{
                    "id": "alice-id",
                    "did": "did:example:alice",
                    "local_alias": "alice",
                    "state": "ready",
                    "profile": {},
                    "auth": { "kind": "bearer", "access_token": "test-token" },
                    "created_at": "2026-05-21T00:00:00Z",
                    "updated_at": "2026-05-21T00:00:00Z",
                    "ready_for_auth": true,
                    "ready_for_messaging": true,
                    "missing": []
                  }]
                }"#,
            )
            .unwrap();
            Self { root }
        }

        fn client(&self) -> crate::core::ImClient {
            crate::core::ImCore::new(
                crate::ImCoreConfig {
                    service_base_url: crate::ServiceEndpoint::parse("https://example.test")
                        .unwrap(),
                    did_domain: "awiki.test".to_owned(),
                    user_service_endpoint: None,
                    message_service_endpoint: None,
                    mail_service_endpoint: None,
                    anp_service_endpoint: None,
                    anp_service_did: None,
                    ca_bundle: None,
                    transport_policy: crate::MessageTransportPolicy::HttpOnly,
                },
                crate::ImCorePaths {
                    identities: crate::paths::IdentityRegistryPaths {
                        identity_root_dir: self.root.path().join("identities"),
                        registry_path: self.root.path().join("identities").join("registry.json"),
                        default_identity_path: Some(
                            self.root.path().join("identities").join("default"),
                        ),
                    },
                    local_state: crate::paths::LocalStatePaths {
                        sqlite_path: self.root.path().join("local").join("im.sqlite"),
                    },
                    runtime: crate::paths::RuntimePaths {
                        cache_dir: self.root.path().join("cache"),
                        temp_dir: self.root.path().join("tmp"),
                    },
                },
            )
            .unwrap()
            .client(crate::identity::IdentitySelector::LocalAlias(
                "alice".to_owned(),
            ))
            .unwrap()
        }
    }
}
