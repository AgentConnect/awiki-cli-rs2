use std::sync::{Arc, RwLock};

use crate::dto::{
    config::{DartImCoreConfig, DartImCorePaths},
    error::DartImError,
};

pub struct DartImCore {
    state: Arc<RwLock<DartImCoreState>>,
}

struct DartImCoreState {
    inner: Option<im_core::ImCore>,
    default_service_did: Option<String>,
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

    pub(crate) fn default_service_did(&self) -> Result<Option<String>, DartImError> {
        let guard = self
            .state
            .read()
            .map_err(|_| DartImError::internal("core lock poisoned"))?;
        if guard.inner.is_none() {
            return Err(DartImError::object_closed("DartImCore"));
        }
        Ok(guard.default_service_did.clone())
    }
}

pub fn open_core(
    config: DartImCoreConfig,
    paths: DartImCorePaths,
) -> Result<Arc<DartImCore>, DartImError> {
    let default_service_did = config.anp_service_did.clone();
    let inner =
        im_core::ImCore::new(config.try_into()?, paths.try_into()?).map_err(DartImError::from)?;
    Ok(Arc::new(DartImCore {
        state: Arc::new(RwLock::new(DartImCoreState {
            inner: Some(inner),
            default_service_did,
        })),
    }))
}

pub fn close_core(core: Arc<DartImCore>) -> Result<(), DartImError> {
    let mut guard = core
        .state
        .write()
        .map_err(|_| DartImError::internal("core lock poisoned"))?;
    guard.inner = None;
    Ok(())
}

pub fn validate_paths(core: Arc<DartImCore>) -> Result<Vec<String>, DartImError> {
    core.with_inner(|inner| {
        let report = inner
            .bootstrap()
            .validate_paths()
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
    })
}
