use std::sync::{Arc, RwLock};

use crate::dto::{
    config::{DartImCoreConfig, DartImCoreOpenOptions, DartImCorePaths},
    error::DartImError,
};

pub struct DartImCore {
    state: Arc<RwLock<DartImCoreState>>,
    close_lock: tokio::sync::Mutex<()>,
}

struct DartImCoreState {
    inner: Option<im_core::ImCore>,
}

impl DartImCore {
    pub(crate) fn with_inner<T>(
        &self,
        f: impl FnOnce(&im_core::ImCore) -> Result<T, DartImError>,
    ) -> Result<T, DartImError> {
        let guard = self
            .state
            .read()
            .map_err(|_| DartImError::internal("core lock poisoned"))?;
        let inner = guard
            .inner
            .as_ref()
            .ok_or_else(|| DartImError::object_closed("DartImCore"))?;
        f(inner)
    }

    pub(crate) fn clone_inner(&self) -> Result<im_core::ImCore, DartImError> {
        let guard = self
            .state
            .read()
            .map_err(|_| DartImError::internal("core lock poisoned"))?;
        guard
            .inner
            .as_ref()
            .cloned()
            .ok_or_else(|| DartImError::object_closed("DartImCore"))
    }
}

pub async fn open_core(
    config: DartImCoreConfig,
    paths: DartImCorePaths,
) -> Result<Arc<DartImCore>, DartImError> {
    open_core_inner(config, paths, None).await
}

pub async fn open_core_with_options(
    config: DartImCoreConfig,
    paths: DartImCorePaths,
    options: DartImCoreOpenOptions,
) -> Result<Arc<DartImCore>, DartImError> {
    open_core_inner(config, paths, Some(options)).await
}

async fn open_core_inner(
    config: DartImCoreConfig,
    paths: DartImCorePaths,
    options: Option<DartImCoreOpenOptions>,
) -> Result<Arc<DartImCore>, DartImError> {
    let config = config.try_into()?;
    let paths = paths.try_into()?;
    let mut options: im_core::ImCoreOpenOptions = options
        .map(TryInto::try_into)
        .transpose()?
        .unwrap_or_default();
    options.network_runtime = crate::api::network::configured_runtime();
    let inner = im_core::ImCore::open_with_options(config, paths, options)
        .await
        .map_err(DartImError::from)?;
    inner
        .identities()
        .migrate_identity_custody_async()
        .await
        .map_err(DartImError::from)?;
    Ok(Arc::new(DartImCore {
        state: Arc::new(RwLock::new(DartImCoreState { inner: Some(inner) })),
        close_lock: tokio::sync::Mutex::new(()),
    }))
}

pub async fn open_core_with_optional_options(
    config: DartImCoreConfig,
    paths: DartImCorePaths,
    options: Option<DartImCoreOpenOptions>,
) -> Result<Arc<DartImCore>, DartImError> {
    open_core_inner(config, paths, options).await
}

pub async fn close_core(core: &Arc<DartImCore>) -> Result<(), DartImError> {
    // 重复关闭也须等待同一次 actor 终止；异步等待期间不持有状态 RwLock。
    let _close = core.close_lock.lock().await;
    let inner = core
        .state
        .write()
        .map_err(|_| DartImError::internal("core lock poisoned"))?
        .inner
        .take();
    if let Some(inner) = inner {
        #[cfg(feature = "sqlite")]
        inner
            .shutdown_local_state_for_reset()
            .await
            .map_err(DartImError::from)?;
        #[cfg(not(feature = "sqlite"))]
        inner.stop_backend_discovery();
    }
    Ok(())
}

pub async fn validate_paths(core: &Arc<DartImCore>) -> Result<Vec<String>, DartImError> {
    let inner = core.clone_inner()?;
    let report = inner
        .bootstrap()
        .validate_paths_async()
        .await
        .map_err(DartImError::from)?;
    let mut lines = report
        .checked
        .into_iter()
        .map(|check| {
            format!(
                "{}:{}:exists={}:readable={}:writable={}",
                check.kind,
                check.path,
                check.exists,
                check.readable,
                check
                    .writable
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            )
        })
        .collect::<Vec<_>>();
    lines.extend(
        report
            .warnings
            .into_iter()
            .map(|warning| format!("warning:{warning}")),
    );
    Ok(lines)
}

/// Safe versioned capability projection; does not expose credentials or exception text.
pub async fn public_backend_capabilities(
    instance: &Arc<DartImCore>,
    force: bool,
) -> Result<String, DartImError> {
    let snapshot = instance
        .clone_inner()?
        .backend_capabilities_async(force)
        .await
        .map_err(DartImError::from)?;
    Ok(serde_json::json!(snapshot).to_string())
}

#[cfg(all(test, feature = "sqlite"))]
#[path = "core_lifecycle_tests.rs"]
mod lifecycle_tests;
