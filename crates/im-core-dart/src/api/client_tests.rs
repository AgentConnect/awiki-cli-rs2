use super::*;
use im_core::prelude::*;
use std::time::Duration;

fn core_at(root: &std::path::Path) -> ImCore {
    ImCore::new(
        ImCoreConfig::new(
            ServiceEndpoint::parse("http://127.0.0.1:9").unwrap(),
            "awiki.test",
        )
        .unwrap(),
        ImCorePaths {
            identities: IdentityRegistryPaths {
                identity_root_dir: root.join("identities"),
                registry_path: root.join("registry.json"),
                default_identity_path: None,
            },
            local_state: LocalStatePaths {
                sqlite_path: root.join("state.sqlite"),
            },
            runtime: RuntimePaths {
                cache_dir: root.join("cache"),
                temp_dir: root.join("tmp"),
            },
        },
    )
    .unwrap()
}

fn closed_client_retaining_core(core: ImCore) -> DartImClient {
    let (capability_source, _) = tokio::sync::watch::channel(None);
    DartImClient {
        state: Arc::new(RwLock::new(DartImClientState {
            core: Some(core),
            inner: None,
            capability_source,
        })),
        runtime_refresh_lock: tokio::sync::Mutex::new(()),
    }
}

#[tokio::test]
async fn close_releases_retained_core_without_waiting_for_dart_gc() {
    let root = tempfile::tempdir().unwrap();
    let core = core_at(root.path());
    core.bootstrap().migrate_local_state_async().await.unwrap();
    let client = Arc::new(closed_client_retaining_core(core));
    close_client(&client).unwrap();
    assert!(
        client.state.read().unwrap().core.is_none(),
        "Closed clients must release their Core owner"
    );
    assert_eq!(client.clone_core().err().unwrap().code, "object_closed");
    close_client(&client).unwrap();
    // The opaque client deliberately remains alive, as it can in the Dart VM.
    // Permit the actor thread to observe the last sender dropping, then require
    // Windows to release the SQLite files rather than waiting for GC.
    for attempt in 0..50 {
        match std::fs::remove_dir_all(root.path()) {
            Ok(()) => return,
            Err(error) if attempt < 49 && matches!(error.raw_os_error(), Some(32 | 145)) => {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Err(error) => panic!("Closed client retained local files: {error}"),
        }
    }
}

#[tokio::test]
async fn closing_client_preserves_a_separately_owned_core() {
    let root = tempfile::tempdir().unwrap();
    let owner = core_at(root.path());
    let client = Arc::new(closed_client_retaining_core(owner.clone()));
    close_client(&client).unwrap();
    assert!(client.state.read().unwrap().core.is_none());
    assert_eq!(client.clone_core().err().unwrap().code, "object_closed");
    owner.bootstrap().migrate_local_state_async().await.unwrap();
    owner.shutdown_local_state_for_reset().await.unwrap();
}
