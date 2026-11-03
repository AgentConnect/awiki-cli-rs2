use super::*;
use im_core::prelude::*;

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

fn dart_core(inner: ImCore) -> Arc<DartImCore> {
    Arc::new(DartImCore {
        state: Arc::new(RwLock::new(DartImCoreState { inner: Some(inner) })),
        close_lock: tokio::sync::Mutex::new(()),
    })
}

#[tokio::test]
async fn close_waits_for_database_with_retained_internal_owner() {
    let root = tempfile::tempdir().unwrap();
    let inner = core_at(root.path());
    inner
        .bootstrap()
        .initialize_local_state_async()
        .await
        .unwrap();
    let retained_owner = inner.clone();
    let core = dart_core(inner);
    close_core(&core).await.unwrap();
    assert_eq!(core.clone_inner().err().unwrap().code, "object_closed");
    // 模拟尚未释放 Core 引用的内部任务；关闭完成后 actor 必须已终止。
    assert!(
        matches!(
            retained_owner
                .bootstrap()
                .initialize_local_state_async()
                .await,
            Err(im_core::ImError::LocalStateUnavailable { .. })
        ),
        "Core close returned while the database actor was still available"
    );
    // Windows 必须立即允许删除；不依赖 Dart GC、等待或删除重试。
    std::fs::remove_dir_all(root.path()).unwrap();
    close_core(&core).await.unwrap();
}

#[tokio::test]
async fn closing_unused_core_does_not_initialize_database() {
    let root = tempfile::tempdir().unwrap();
    let core = dart_core(core_at(root.path()));
    close_core(&core).await.unwrap();
    close_core(&core).await.unwrap();
    assert!(!root.path().join("state.sqlite").exists());
}

#[tokio::test]
async fn closing_core_preserves_an_independent_database() {
    let root = tempfile::tempdir().unwrap();
    let other_root = tempfile::tempdir().unwrap();
    let inner = core_at(root.path());
    inner
        .bootstrap()
        .initialize_local_state_async()
        .await
        .unwrap();
    let other = core_at(other_root.path());
    other
        .bootstrap()
        .initialize_local_state_async()
        .await
        .unwrap();
    let core = dart_core(inner);
    close_core(&core).await.unwrap();
    other
        .bootstrap()
        .initialize_local_state_async()
        .await
        .unwrap();
    other.shutdown_local_state_for_reset().await.unwrap();
}

#[tokio::test]
async fn concurrent_closes_both_wait_for_database_shutdown() {
    let root = tempfile::tempdir().unwrap();
    let inner = core_at(root.path());
    inner
        .bootstrap()
        .initialize_local_state_async()
        .await
        .unwrap();
    let retained_owner = inner.clone();
    let core = dart_core(inner);
    let (first, second) = tokio::join!(close_core(&core), close_core(&core));
    first.unwrap();
    second.unwrap();
    assert!(matches!(
        retained_owner
            .bootstrap()
            .initialize_local_state_async()
            .await,
        Err(im_core::ImError::LocalStateUnavailable { .. })
    ));
    std::fs::remove_dir_all(root.path()).unwrap();
}
