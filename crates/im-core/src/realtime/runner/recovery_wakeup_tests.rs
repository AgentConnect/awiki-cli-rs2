use super::*;
use crate::internal::group_runtime::{journal, recovery};
use crate::internal::message_runtime::sync_dispatcher;

const GROUP: &str = "did:example:group";
const KEY: &str = "recovery:repair:did:example:group";

fn hint(client: &crate::core::ImClient) -> Value {
    json!({"method":"group_e2ee_reconcile_required", "params": {
        "recipient_did":client.did().as_str(), "group_did":GROUP
    }})
}

fn attempted(client: &crate::core::ImClient) -> bool {
    journal::load(client, KEY).unwrap().is_some_and(|value| {
        value["attempts"].as_u64().unwrap_or(0) > 0 && value["phase"] == "retry"
    })
}

async fn wait_for_attempt(client: &crate::core::ImClient) -> bool {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while !attempted(client) {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .is_ok()
}

async fn finish(client: &crate::core::ImClient) {
    recovery::record(client, KEY, GROUP, None, true).unwrap();
    let dispatcher = sync_dispatcher::for_client(client);
    sync_dispatcher::wake_client(client);
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while dispatcher.running_for_test() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn one_hint_runs_the_repair_executor_without_polling_and_preserves_backoff() {
    let fixture = TestClientFixture::new("group-hint-execution");
    let client = fixture
        .core_with_group_e2ee(true)
        .client(crate::identity::IdentitySelector::LocalAlias(
            "alice".into(),
        ))
        .unwrap();
    let mut warnings = vec![];
    assert!(normalize_group_e2ee_realtime_notification_async(
        &client,
        hint(&client),
        &mut warnings
    )
    .await
    .is_none());
    assert!(warnings.is_empty());
    let started = wait_for_attempt(&client).await;
    if started {
        // The production repair was actually attempted. This fixture lacks
        // encryption-preparation prerequisites; a wake must not mark it ready.
        let mut progress = journal::load(&client, KEY).unwrap().unwrap();
        assert_eq!(
            progress["status"]["reason"],
            "encryption_preparation_failed"
        );
        assert_eq!(progress["status"]["encryption_ready"], false);
        let attempts = progress["attempts"].clone();
        let next = chrono::Utc::now().timestamp() + 120;
        progress["next_attempt_at"] = json!(next);
        journal::save(&client, KEY, progress).unwrap();
        for _ in 0..3 {
            assert!(normalize_group_e2ee_realtime_notification_async(
                &client,
                hint(&client),
                &mut warnings
            )
            .await
            .is_none());
        }
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        let progress = journal::load(&client, KEY).unwrap().unwrap();
        assert_eq!(progress["attempts"], attempts);
        assert_eq!(progress["next_attempt_at"], next);
    }
    finish(&client).await;
    assert!(
        started,
        "a hint must start the executor, not only write a journal"
    );
}

#[tokio::test]
async fn reopening_a_client_resumes_the_persisted_group_journal_without_another_hint() {
    let fixture = TestClientFixture::new("group-hint-restart");
    let client = fixture
        .core_with_group_e2ee(true)
        .client(crate::identity::IdentitySelector::LocalAlias(
            "alice".into(),
        ))
        .unwrap();
    recovery::record(&client, KEY, GROUP, None, false).unwrap();
    drop(client);
    let core = fixture.core_with_group_e2ee(true);
    let client = core
        .client_async(crate::identity::IdentitySelector::LocalAlias(
            "alice".into(),
        ))
        .await
        .unwrap();
    let started = wait_for_attempt(&client).await;
    finish(&client).await;
    assert!(
        started,
        "client restart must resume pending group work without host polling"
    );
}

#[tokio::test]
async fn hints_arriving_while_recovery_is_running_are_not_lost() {
    let fixture = TestClientFixture::new("group-hint-during-repair");
    let client = fixture
        .core_with_group_e2ee(true)
        .client(crate::identity::IdentitySelector::LocalAlias(
            "alice".into(),
        ))
        .unwrap();
    let lock = client
        .core_inner()
        .group_lifecycle_lock(client.current_identity().id.as_str(), "group-recovery");
    let guard = lock.lock().await;
    let mut warnings = vec![];
    normalize_group_e2ee_realtime_notification_async(&client, hint(&client), &mut warnings).await;
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    let mut second = hint(&client);
    second["params"]["group_did"] = json!("did:example:second-group");
    normalize_group_e2ee_realtime_notification_async(&client, second, &mut warnings).await;
    drop(guard);
    let second_key = "recovery:repair:did:example:second-group";
    let progressed = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            if attempted(&client)
                && journal::load(&client, second_key)
                    .unwrap()
                    .is_some_and(|value| {
                        value["attempts"].as_u64().unwrap_or(0) > 0 && value["phase"] == "retry"
                    })
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .is_ok();
    recovery::record(&client, second_key, "did:example:second-group", None, true).unwrap();
    finish(&client).await;
    assert!(
        progressed,
        "both hints must reach the executor without a periodic host sync"
    );
}

#[test]
fn blocking_realtime_drives_group_recovery_while_the_socket_is_idle() {
    let fixture = TestClientFixture::new("group-hint-blocking");
    let client = fixture
        .core_with_group_e2ee(true)
        .client(crate::identity::IdentitySelector::LocalAlias(
            "alice".into(),
        ))
        .unwrap();
    let mut projector = AsyncFirstSecureRealtimeNotificationProjector::new(&client);
    let projection = projector.project(hint(&client));
    assert!(projection.event.is_none() && projection.additional_events.is_empty());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while !attempted(&client) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let started = attempted(&client);
    projector
        .runtime
        .as_ref()
        .unwrap()
        .block_on(finish(&client));
    assert!(
        started,
        "blocking realtime must drive background work without another WS frame"
    );
}
