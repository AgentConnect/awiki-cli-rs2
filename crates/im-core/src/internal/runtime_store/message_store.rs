use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;

const DEFAULT_LIMIT: u32 = 100;
const MAX_LIMIT: u32 = 200;
const PATCH_BUFFER: usize = 256;

fn subscribe_before_seed<T: Clone, F>(
    sender: &broadcast::Sender<T>,
    projection: &Mutex<()>,
    seed: F,
) -> crate::ImResult<(broadcast::Receiver<T>, Vec<T>)>
where
    F: FnOnce() -> crate::ImResult<Vec<T>>,
{
    let receiver = sender.subscribe();
    let _projection_guard = projection.lock().expect("message projection lock poisoned");
    let initial = seed()?;
    Ok((receiver, initial))
}

#[derive(Debug)]
pub(crate) struct MessageStore {
    owner_identity_id: String,
    owner_did: String,
    projection: Mutex<()>,
    state: Mutex<MessageStoreState>,
    sender: broadcast::Sender<crate::messages::ThreadMessageStorePatch>,
}

#[derive(Debug, Default)]
struct MessageStoreState {
    version: u64,
    threads: HashMap<ThreadKey, ThreadState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ThreadState {
    limit: u32,
    items: Vec<crate::messages::Message>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ThreadKey {
    kind: String,
    id: String,
}

impl MessageStore {
    pub(crate) fn new_for_client(client: &crate::core::ImClient) -> Arc<Self> {
        let (sender, _) = broadcast::channel(PATCH_BUFFER);
        Arc::new(Self {
            owner_identity_id: client.current_identity().id.as_str().to_owned(),
            owner_did: client.did().as_str().to_owned(),
            projection: Mutex::new(()),
            state: Mutex::new(MessageStoreState::default()),
            sender,
        })
    }

    pub(crate) fn watch_for_client(
        self: &Arc<Self>,
        client: &crate::core::ImClient,
        thread: crate::messages::ThreadRef,
        limit: Option<u32>,
    ) -> crate::ImResult<crate::messages::ThreadMessagePatchSession> {
        self.ensure_owner(client)?;
        let limit = normalized_limit(limit);
        let (receiver, initial) = subscribe_before_seed(&self.sender, &self.projection, || {
            Ok(vec![self.repair_thread_from_client_locked(
                client,
                thread.clone(),
                limit,
            )?])
        })?;
        Ok(crate::messages::ThreadMessagePatchSession::new(
            self.clone(),
            receiver,
            initial,
            thread,
            limit,
        ))
    }

    pub(crate) fn repair_thread_from_client(
        &self,
        client: &crate::core::ImClient,
        thread: crate::messages::ThreadRef,
        limit: u32,
    ) -> crate::ImResult<crate::messages::ThreadMessageStorePatch> {
        self.ensure_owner(client)?;
        let limit = normalized_limit(Some(limit));
        let _projection_guard = self
            .projection
            .lock()
            .expect("message projection lock poisoned");
        self.repair_thread_from_client_locked(client, thread, limit)
    }

    fn repair_thread_from_client_locked(
        &self,
        client: &crate::core::ImClient,
        thread: crate::messages::ThreadRef,
        limit: u32,
    ) -> crate::ImResult<crate::messages::ThreadMessageStorePatch> {
        let items = committed_thread_items(client, &thread, limit)?;
        Ok(self.replace_thread_items(&thread, limit, items))
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
        self.emit_patches_for_tracked_threads(client, "committed_sync");
    }

    pub(crate) fn on_committed_local_projection(
        &self,
        client: &crate::core::ImClient,
        reason: &str,
    ) {
        if self.ensure_owner(client).is_err() {
            return;
        }
        self.emit_patches_for_tracked_threads(client, reason);
    }

    pub(crate) fn repair_required_patch(
        &self,
        thread: &crate::messages::ThreadRef,
        limit: u32,
        reason: &str,
    ) -> crate::messages::ThreadMessageStorePatch {
        let key = ThreadKey::from_thread(thread);
        let mut state = self.state.lock().expect("message store lock poisoned");
        state.version = state.version.saturating_add(1);
        state
            .threads
            .entry(key.clone())
            .or_insert_with(|| ThreadState {
                limit: normalized_limit(Some(limit)),
                items: Vec::new(),
            })
            .limit = normalized_limit(Some(limit));
        crate::messages::ThreadMessageStorePatch::RepairRequired {
            owner_identity_id: self.owner_identity_id.clone(),
            owner_did: self.owner_did.clone(),
            version: state.version,
            conversation_identity: Some(
                crate::messages::ConversationIdentity::from_storage_parts_for_owner(
                    key.kind.clone(),
                    key.id.clone(),
                    &self.owner_did,
                ),
            ),
            thread_kind: key.kind,
            thread_id: key.id,
            reason: reason.to_owned(),
        }
    }

    #[cfg(test)]
    pub(crate) fn version_for_test(&self) -> u64 {
        self.state
            .lock()
            .expect("message store lock poisoned")
            .version
    }

    fn replace_thread_items(
        &self,
        thread: &crate::messages::ThreadRef,
        limit: u32,
        items: Vec<crate::messages::Message>,
    ) -> crate::messages::ThreadMessageStorePatch {
        let key = ThreadKey::from_thread(thread);
        let mut state = self.state.lock().expect("message store lock poisoned");
        state.version = state.version.saturating_add(1);
        state.threads.insert(
            key.clone(),
            ThreadState {
                limit: normalized_limit(Some(limit)),
                items: items.clone(),
            },
        );
        crate::messages::ThreadMessageStorePatch::Reset {
            owner_identity_id: self.owner_identity_id.clone(),
            owner_did: self.owner_did.clone(),
            version: state.version,
            conversation_identity: Some(
                crate::messages::ConversationIdentity::from_storage_parts_for_owner(
                    key.kind.clone(),
                    key.id.clone(),
                    &self.owner_did,
                ),
            ),
            thread_kind: key.kind,
            thread_id: key.id,
            items,
        }
    }

    fn emit_patches_for_tracked_threads(&self, client: &crate::core::ImClient, reason: &str) {
        let _projection_guard = self
            .projection
            .lock()
            .expect("message projection lock poisoned");
        let tracked = {
            let state = self.state.lock().expect("message store lock poisoned");
            state
                .threads
                .iter()
                .map(|(key, thread_state)| (key.clone(), thread_state.limit))
                .collect::<Vec<_>>()
        };
        for (key, limit) in tracked {
            let thread = match key.to_thread_ref() {
                Ok(thread) => thread,
                Err(_) => {
                    let _ = self.sender.send(self.repair_required_for_key(
                        key,
                        limit,
                        "tracked_thread_invalid",
                    ));
                    continue;
                }
            };
            let patch = match committed_thread_items(client, &thread, limit) {
                Ok(items) => self.diff_thread_items(&thread, limit, items),
                Err(_) => Some(self.repair_required_patch(&thread, limit, reason)),
            };
            if let Some(patch) = patch {
                let _ = self.sender.send(patch);
            }
        }
    }

    fn diff_thread_items(
        &self,
        thread: &crate::messages::ThreadRef,
        limit: u32,
        next_items: Vec<crate::messages::Message>,
    ) -> Option<crate::messages::ThreadMessageStorePatch> {
        let key = ThreadKey::from_thread(thread);
        let mut state = self.state.lock().expect("message store lock poisoned");
        let normalized_limit = normalized_limit(Some(limit));
        let previous_items = state
            .threads
            .get(&key)
            .map(|thread_state| thread_state.items.clone())
            .unwrap_or_default();
        if previous_items == next_items {
            if let Some(thread_state) = state.threads.get_mut(&key) {
                thread_state.limit = normalized_limit;
            }
            return None;
        }
        state.version = state.version.saturating_add(1);
        let version = state.version;
        state.threads.insert(
            key.clone(),
            ThreadState {
                limit: normalized_limit,
                items: next_items.clone(),
            },
        );
        Some(
            diff_patch(
                &self.owner_identity_id,
                &self.owner_did,
                version,
                &key,
                &previous_items,
                &next_items,
            )
            .unwrap_or_else(|| crate::messages::ThreadMessageStorePatch::Reset {
                owner_identity_id: self.owner_identity_id.clone(),
                owner_did: self.owner_did.clone(),
                version,
                conversation_identity: Some(
                    crate::messages::ConversationIdentity::from_storage_parts_for_owner(
                        key.kind.clone(),
                        key.id.clone(),
                        &self.owner_did,
                    ),
                ),
                thread_kind: key.kind,
                thread_id: key.id,
                items: next_items,
            }),
        )
    }

    fn repair_required_for_key(
        &self,
        key: ThreadKey,
        limit: u32,
        reason: &str,
    ) -> crate::messages::ThreadMessageStorePatch {
        let mut state = self.state.lock().expect("message store lock poisoned");
        state.version = state.version.saturating_add(1);
        state
            .threads
            .entry(key.clone())
            .or_insert_with(|| ThreadState {
                limit: normalized_limit(Some(limit)),
                items: Vec::new(),
            });
        crate::messages::ThreadMessageStorePatch::RepairRequired {
            owner_identity_id: self.owner_identity_id.clone(),
            owner_did: self.owner_did.clone(),
            version: state.version,
            conversation_identity: Some(
                crate::messages::ConversationIdentity::from_storage_parts_for_owner(
                    key.kind.clone(),
                    key.id.clone(),
                    &self.owner_did,
                ),
            ),
            thread_kind: key.kind,
            thread_id: key.id,
            reason: reason.to_owned(),
        }
    }

    fn ensure_owner(&self, client: &crate::core::ImClient) -> crate::ImResult<()> {
        if client.current_identity().id.as_str() != self.owner_identity_id
            || client.did().as_str() != self.owner_did
        {
            return Err(crate::ImError::invalid_input(
                Some("client".to_owned()),
                "message store owner does not match client identity",
            ));
        }
        Ok(())
    }
}

impl ThreadKey {
    fn from_thread(thread: &crate::messages::ThreadRef) -> Self {
        let (kind, id) = crate::messages::thread_ref_parts(thread);
        Self { kind, id }
    }

    fn to_thread_ref(&self) -> crate::ImResult<crate::messages::ThreadRef> {
        match self.kind.as_str() {
            "direct" => {
                crate::ids::PeerRef::parse(&self.id, "").map(crate::messages::ThreadRef::Direct)
            }
            "group" => crate::ids::GroupRef::parse(&self.id).map(crate::messages::ThreadRef::Group),
            "thread" => {
                crate::ids::ThreadId::parse(&self.id).map(crate::messages::ThreadRef::Thread)
            }
            _ => Err(crate::ImError::invalid_input(
                Some("thread_kind".to_owned()),
                "unsupported message store thread kind",
            )),
        }
    }
}

fn diff_patch(
    owner_identity_id: &str,
    owner_did: &str,
    version: u64,
    key: &ThreadKey,
    previous: &[crate::messages::Message],
    next: &[crate::messages::Message],
) -> Option<crate::messages::ThreadMessageStorePatch> {
    if previous == next {
        return None;
    }
    let previous_keys = previous.iter().map(message_key).collect::<Vec<_>>();
    let next_keys = next.iter().map(message_key).collect::<Vec<_>>();
    let removed = previous_keys
        .iter()
        .filter(|message_id| !next_keys.contains(message_id))
        .collect::<Vec<_>>();
    let added_or_changed = next
        .iter()
        .enumerate()
        .filter(|(_, message)| {
            let key = message_key(message);
            previous
                .iter()
                .find(|previous| message_key(previous) == key)
                != Some(*message)
        })
        .collect::<Vec<_>>();

    if removed.len() == 1 && added_or_changed.is_empty() {
        return Some(crate::messages::ThreadMessageStorePatch::Remove {
            owner_identity_id: owner_identity_id.to_owned(),
            owner_did: owner_did.to_owned(),
            version,
            conversation_identity: Some(
                crate::messages::ConversationIdentity::from_storage_parts_for_owner(
                    key.kind.clone(),
                    key.id.clone(),
                    owner_did,
                ),
            ),
            thread_kind: key.kind.clone(),
            thread_id: key.id.clone(),
            message_id: (*removed[0]).clone(),
        });
    }
    if removed.is_empty() && added_or_changed.len() == 1 {
        let (index, message) = added_or_changed[0];
        return Some(crate::messages::ThreadMessageStorePatch::Upsert {
            owner_identity_id: owner_identity_id.to_owned(),
            owner_did: owner_did.to_owned(),
            version,
            conversation_identity: Some(
                crate::messages::ConversationIdentity::from_storage_parts_for_owner(
                    key.kind.clone(),
                    key.id.clone(),
                    owner_did,
                ),
            ),
            thread_kind: key.kind.clone(),
            thread_id: key.id.clone(),
            message: message.clone(),
            index: u32::try_from(index).unwrap_or(u32::MAX),
        });
    }
    None
}

fn message_key(message: &crate::messages::Message) -> String {
    message.id.as_str().to_owned()
}

fn normalized_limit(limit: Option<u32>) -> u32 {
    limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
}

fn committed_thread_items(
    client: &crate::core::ImClient,
    thread: &crate::messages::ThreadRef,
    limit: u32,
) -> crate::ImResult<Vec<crate::messages::Message>> {
    #[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
    {
        let connection = crate::internal::local_state::open_writable(
            &client.core_inner().sdk_paths().local_state.sqlite_path,
        )?;
        let records =
            crate::internal::local_state::messages::list_messages_for_thread_ref_for_owner_identity(
                &connection,
                client.current_identity().id.as_str(),
                client.did().as_str(),
                thread,
                i64::from(normalized_limit(Some(limit))),
                None,
            )?;
        records
            .records
            .iter()
            .map(crate::internal::message_runtime::conversations::message_from_record)
            .collect::<crate::ImResult<Vec<_>>>()
    }
    #[cfg(not(all(feature = "sqlite", any(feature = "blocking", test))))]
    {
        let _ = (client, thread, limit);
        Err(crate::ImError::unsupported("message-store-local-history"))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[tokio::test]
    async fn subscribe_before_seed_keeps_exactly_one_commit_during_seed() {
        let (sender, _) = tokio::sync::broadcast::channel(4);
        let projection = std::sync::Mutex::new(());
        let (mut receiver, initial) = super::subscribe_before_seed(&sender, &projection, || {
            sender.send("committed").unwrap();
            Ok(vec!["seed"])
        })
        .unwrap();

        assert_eq!(initial, vec!["seed"]);
        assert_eq!(receiver.recv().await.unwrap(), "committed");
        assert!(matches!(
            receiver.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test]
    async fn seed_version_fences_the_commit_already_represented_by_seed() {
        let fixture = Fixture::new("message-store-seed-fence");
        let client = fixture.client();
        let store = client.message_store();
        let thread = crate::messages::ThreadRef::Direct(
            crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
        );
        let receiver = store.sender.subscribe();
        let during_seed = store.repair_required_patch(&thread, 100, "during_seed");
        store.sender.send(during_seed).unwrap();
        let seed = store.repair_required_patch(&thread, 100, "seed_after_commit");
        let mut session = crate::messages::ThreadMessagePatchSession::new(
            store.clone(),
            receiver,
            vec![seed],
            thread.clone(),
            100,
        );

        assert!(matches!(
            session.next_patch().await,
            Some(crate::messages::ThreadMessageStorePatch::RepairRequired {
                version: 2,
                reason,
                ..
            }) if reason == "seed_after_commit"
        ));
        let after_seed = store.repair_required_patch(&thread, 100, "after_seed");
        store.sender.send(after_seed).unwrap();
        assert!(matches!(
            session.next_patch().await,
            Some(crate::messages::ThreadMessageStorePatch::RepairRequired {
                version: 3,
                reason,
                ..
            }) if reason == "after_seed"
        ));
    }

    #[tokio::test]
    async fn seed_first_serializes_commit_projection_without_regression() {
        let fixture = Fixture::new("message-store-seed-first-race");
        let client = fixture.client();
        let store = client.message_store();
        let thread = crate::messages::ThreadRef::Direct(
            crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
        );
        let old_items = vec![message("m1", "old")];
        let committed_items = vec![message("m1", "new")];
        let canonical = std::sync::Arc::new(std::sync::Mutex::new(old_items.clone()));
        let seed_read = std::sync::Arc::new(std::sync::Barrier::new(2));
        let release_seed = std::sync::Arc::new(std::sync::Barrier::new(2));

        let seed_store = store.clone();
        let seed_thread = thread.clone();
        let seed_canonical = canonical.clone();
        let seed_read_worker = seed_read.clone();
        let release_seed_worker = release_seed.clone();
        let seed_worker = std::thread::spawn(move || {
            super::subscribe_before_seed(&seed_store.sender, &seed_store.projection, || {
                let items = seed_canonical
                    .lock()
                    .expect("canonical message fixture lock poisoned")
                    .clone();
                seed_read_worker.wait();
                release_seed_worker.wait();
                Ok(vec![seed_store.replace_thread_items(
                    &seed_thread,
                    100,
                    items,
                )])
            })
            .unwrap()
        });

        seed_read.wait();
        *canonical
            .lock()
            .expect("canonical message fixture lock poisoned") = committed_items.clone();

        let callback_store = store.clone();
        let callback_thread = thread.clone();
        let callback_canonical = canonical.clone();
        let (callback_done_tx, callback_done_rx) = std::sync::mpsc::channel();
        let callback_worker = std::thread::spawn(move || {
            let _projection_guard = callback_store
                .projection
                .lock()
                .expect("message projection lock poisoned");
            let items = callback_canonical
                .lock()
                .expect("canonical message fixture lock poisoned")
                .clone();
            if let Some(patch) = callback_store.diff_thread_items(&callback_thread, 100, items) {
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

        let mut session = crate::messages::ThreadMessagePatchSession::new(
            store.clone(),
            receiver,
            initial,
            thread.clone(),
            100,
        );
        assert!(matches!(
            session.next_patch().await,
            Some(crate::messages::ThreadMessageStorePatch::Reset { items, .. })
                if items == old_items
        ));
        assert!(matches!(
            session.next_patch().await,
            Some(crate::messages::ThreadMessageStorePatch::Upsert { message, .. })
                if message == committed_items[0]
        ));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), session.next_patch())
                .await
                .is_err(),
            "the commit represented after the seed must be delivered exactly once"
        );
        let key = super::ThreadKey::from_thread(&thread);
        assert_eq!(
            store
                .state
                .lock()
                .expect("message store lock poisoned")
                .threads
                .get(&key)
                .expect("tracked thread")
                .items,
            committed_items,
            "the callback must re-read canonical state after the seed and must not regress it"
        );
    }

    #[tokio::test]
    async fn callback_first_is_fenced_by_newer_seed_without_duplicate_delivery() {
        let fixture = Fixture::new("message-store-callback-first-race");
        let client = fixture.client();
        let store = client.message_store();
        let thread = crate::messages::ThreadRef::Direct(
            crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
        );
        let committed_items = vec![message("m1", "new")];
        let canonical = std::sync::Arc::new(std::sync::Mutex::new(committed_items.clone()));
        let receiver = store.sender.subscribe();
        let callback_locked = std::sync::Arc::new(std::sync::Barrier::new(2));
        let release_callback = std::sync::Arc::new(std::sync::Barrier::new(2));

        let callback_store = store.clone();
        let callback_thread = thread.clone();
        let callback_canonical = canonical.clone();
        let callback_locked_worker = callback_locked.clone();
        let release_callback_worker = release_callback.clone();
        let callback_worker = std::thread::spawn(move || {
            let _projection_guard = callback_store
                .projection
                .lock()
                .expect("message projection lock poisoned");
            let items = callback_canonical
                .lock()
                .expect("canonical message fixture lock poisoned")
                .clone();
            if let Some(patch) = callback_store.diff_thread_items(&callback_thread, 100, items) {
                let _ = callback_store.sender.send(patch);
            }
            callback_locked_worker.wait();
            release_callback_worker.wait();
        });

        callback_locked.wait();
        let seed_store = store.clone();
        let seed_thread = thread.clone();
        let seed_canonical = canonical.clone();
        let seed_started = std::sync::Arc::new(std::sync::Barrier::new(2));
        let seed_started_worker = seed_started.clone();
        let seed_worker = std::thread::spawn(move || {
            seed_started_worker.wait();
            let _projection_guard = seed_store
                .projection
                .lock()
                .expect("message projection lock poisoned");
            let items = seed_canonical
                .lock()
                .expect("canonical message fixture lock poisoned")
                .clone();
            vec![seed_store.replace_thread_items(&seed_thread, 100, items)]
        });
        seed_started.wait();
        release_callback.wait();
        callback_worker.join().unwrap();
        let initial = seed_worker.join().unwrap();

        let mut session = crate::messages::ThreadMessagePatchSession::new(
            store.clone(),
            receiver,
            initial,
            thread.clone(),
            100,
        );
        assert!(matches!(
            session.next_patch().await,
            Some(crate::messages::ThreadMessageStorePatch::Reset {
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
        let key = super::ThreadKey::from_thread(&thread);
        assert_eq!(
            store
                .state
                .lock()
                .expect("message store lock poisoned")
                .threads
                .get(&key)
                .expect("tracked thread")
                .items,
            committed_items,
            "a callback that wins the projection lock must remain represented by the seed"
        );
    }

    #[test]
    fn message_store_diff_emits_upsert_for_single_changed_message() {
        let key = super::ThreadKey {
            kind: "direct".to_owned(),
            id: "did:example:bob".to_owned(),
        };
        let old = message("m1", "old");
        let new = message("m1", "new");
        let patch = super::diff_patch(
            "owner-id",
            "did:example:alice",
            1,
            &key,
            &[old],
            &[new.clone()],
        )
        .unwrap();
        assert_eq!(
            patch,
            crate::messages::ThreadMessageStorePatch::Upsert {
                owner_identity_id: "owner-id".to_owned(),
                owner_did: "did:example:alice".to_owned(),
                version: 1,
                conversation_identity: Some(
                    crate::messages::ConversationIdentity::from_storage_parts_for_owner(
                        "direct",
                        "did:example:bob",
                        "did:example:alice",
                    )
                ),
                thread_kind: "direct".to_owned(),
                thread_id: "did:example:bob".to_owned(),
                message: new,
                index: 0,
            }
        );
    }

    #[test]
    fn message_store_diff_emits_remove_for_single_removed_message() {
        let key = super::ThreadKey {
            kind: "direct".to_owned(),
            id: "did:example:bob".to_owned(),
        };
        let patch = super::diff_patch(
            "owner-id",
            "did:example:alice",
            7,
            &key,
            &[message("m1", "gone")],
            &[],
        )
        .unwrap();
        assert_eq!(
            patch,
            crate::messages::ThreadMessageStorePatch::Remove {
                owner_identity_id: "owner-id".to_owned(),
                owner_did: "did:example:alice".to_owned(),
                version: 7,
                conversation_identity: Some(
                    crate::messages::ConversationIdentity::from_storage_parts_for_owner(
                        "direct",
                        "did:example:bob",
                        "did:example:alice",
                    )
                ),
                thread_kind: "direct".to_owned(),
                thread_id: "did:example:bob".to_owned(),
                message_id: "m1".to_owned(),
            }
        );
    }

    #[test]
    fn message_store_diff_falls_back_to_reset_for_multi_message_change() {
        let key = super::ThreadKey {
            kind: "group".to_owned(),
            id: "group:dev".to_owned(),
        };
        assert!(super::diff_patch(
            "owner-id",
            "did:example:alice",
            3,
            &key,
            &[message("m1", "one")],
            &[message("m2", "two"), message("m3", "three")],
        )
        .is_none());
    }

    #[test]
    fn message_store_diff_has_no_patch_for_unchanged_items() {
        let key = super::ThreadKey {
            kind: "group".to_owned(),
            id: "group:dev".to_owned(),
        };
        let items = vec![message("m1", "one")];

        assert!(
            super::diff_patch("owner-id", "did:example:alice", 3, &key, &items, &items,).is_none()
        );
    }

    #[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
    #[tokio::test]
    async fn message_store_repair_reads_committed_thread_history() {
        let fixture = Fixture::new("message-store-repair");
        let client = fixture.client();
        let message = message("m1", "hello");
        crate::internal::message_runtime::local_projection::persist_messages(
            &client,
            &[message.clone()],
        )
        .unwrap();

        let patch = client
            .message_store()
            .repair_thread_from_client(
                &client,
                crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                100,
            )
            .unwrap();

        match patch {
            crate::messages::ThreadMessageStorePatch::Reset { items, .. } => {
                assert_eq!(items, vec![message]);
            }
            other => panic!("unexpected patch: {other:?}"),
        }
    }

    #[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
    #[tokio::test]
    async fn message_store_emits_patch_after_local_projection_commit() {
        let fixture = Fixture::new("message-store-local-send");
        let client = fixture.client();
        let store = client.message_store();
        let mut session = store
            .watch_for_client(
                &client,
                crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                Some(100),
            )
            .unwrap();
        assert!(matches!(
            session.next_patch().await,
            Some(crate::messages::ThreadMessageStorePatch::Reset { .. })
        ));
        crate::internal::message_runtime::local_projection::persist_messages(
            &client,
            &[message("m2", "new")],
        )
        .unwrap();
        client.emit_committed_message_projection("local_send");

        match session.next_patch().await {
            Some(crate::messages::ThreadMessageStorePatch::Upsert { message, .. }) => {
                assert_eq!(message.id.as_str(), "m2");
            }
            other => panic!("unexpected patch: {other:?}"),
        }
    }

    #[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
    #[tokio::test]
    async fn message_store_ignores_unchanged_local_projection_commit() {
        let fixture = Fixture::new("message-store-unchanged-commit");
        let client = fixture.client();
        let store = client.message_store();
        let mut session = store
            .watch_for_client(
                &client,
                crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                Some(100),
            )
            .unwrap();
        assert!(matches!(
            session.next_patch().await,
            Some(crate::messages::ThreadMessageStorePatch::Reset { .. })
        ));
        let version_before = store.version_for_test();

        client.emit_committed_message_projection("unchanged_projection");

        assert_eq!(store.version_for_test(), version_before);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), session.next_patch())
                .await
                .is_err(),
            "unchanged projection must not emit a patch"
        );
    }

    fn message(id: &str, text: &str) -> crate::messages::Message {
        crate::messages::Message {
            id: crate::ids::MessageId::parse(id).unwrap(),
            thread: crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
            direction: crate::messages::MessageDirection::Outgoing,
            sender: crate::ids::PeerRef::parse("did:example:alice", "").unwrap(),
            receiver: Some(crate::ids::PeerRef::parse("did:example:bob", "").unwrap()),
            group: None,
            body: crate::messages::MessageBodyView::Text {
                text: text.to_owned(),
                kind: crate::messages::MessageKind::Text,
            },
            sent_at: Some("2026-06-27T00:00:00Z".to_owned()),
            received_at: None,
            metadata: crate::messages::MessageMetadata {
                server_sequence: Some(1),
                content_type: Some("text/plain".to_owned()),
                conversation_identity: Some(
                    crate::messages::ConversationIdentity::from_thread_ref_for_owner(
                        &crate::messages::ThreadRef::Direct(
                            crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                        ),
                        "did:example:alice",
                    ),
                ),
                ..crate::messages::MessageMetadata::default()
            },
        }
    }

    #[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
    struct Fixture {
        root: std::path::PathBuf,
    }

    #[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
    impl Fixture {
        fn new(prefix: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir()
                .join(format!("im-core-{prefix}-{}-{nanos}", std::process::id()));
            let identity_root = root.join("identities");
            let identity_dir = identity_root.join("alice");
            std::fs::create_dir_all(&identity_dir).unwrap();
            std::fs::create_dir_all(root.join("local")).unwrap();
            std::fs::write(identity_root.join("default"), "alice\n").unwrap();
            std::fs::write(
                identity_root.join("registry.json"),
                json!({
                    "default_identity": "alice",
                    "identities": [{
                        "id": "alice-id",
                        "did": "did:example:alice",
                        "local_alias": "alice",
                        "ready_for_auth": true,
                        "ready_for_messaging": true,
                        "missing": []
                    }]
                })
                .to_string(),
            )
            .unwrap();
            std::fs::write(identity_dir.join("did.json"), "{}").unwrap();
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
                    identities: crate::IdentityRegistryPaths {
                        identity_root_dir: self.root.join("identities"),
                        registry_path: self.root.join("identities").join("registry.json"),
                        default_identity_path: Some(self.root.join("identities").join("default")),
                    },
                    local_state: crate::LocalStatePaths {
                        sqlite_path: self.sqlite_path(),
                    },
                    runtime: crate::RuntimePaths {
                        cache_dir: self.root.join("cache"),
                        temp_dir: self.root.join("tmp"),
                    },
                },
            )
            .unwrap()
            .client(crate::identity::IdentitySelector::LocalAlias(
                "alice".to_owned(),
            ))
            .unwrap()
        }

        fn sqlite_path(&self) -> std::path::PathBuf {
            self.root.join("local").join("im.sqlite")
        }
    }
}
