use std::sync::{Arc, RwLock};

use crate::dto::{error::DartImError, identity::DartIdentitySelector};

pub struct DartImClient {
    state: Arc<RwLock<DartImClientState>>,
    runtime_refresh_lock: tokio::sync::Mutex<()>,
}

struct DartImClientState {
    core: Option<im_core::ImCore>,
    inner: Option<im_core::ImClient>,
}

impl DartImClient {
    #[cfg(test)]
    pub(crate) fn new(inner: im_core::ImClient) -> Self {
        Self {
            state: Arc::new(RwLock::new(DartImClientState {
                core: None,
                inner: Some(inner),
            })),
            runtime_refresh_lock: tokio::sync::Mutex::new(()),
        }
    }

    pub(crate) fn new_with_core(core: im_core::ImCore, inner: im_core::ImClient) -> Self {
        Self {
            state: Arc::new(RwLock::new(DartImClientState {
                core: Some(core),
                inner: Some(inner),
            })),
            runtime_refresh_lock: tokio::sync::Mutex::new(()),
        }
    }

    pub(crate) fn with_inner<T>(
        &self,
        f: impl FnOnce(&im_core::ImClient) -> Result<T, DartImError>,
    ) -> Result<T, DartImError> {
        let guard = self
            .state
            .read()
            .map_err(|_| DartImError::internal("client lock poisoned"))?;
        let inner = guard
            .inner
            .as_ref()
            .ok_or_else(|| DartImError::object_closed("DartImClient"))?;
        f(inner)
    }

    pub(crate) fn clone_inner(&self) -> Result<im_core::ImClient, DartImError> {
        let guard = self
            .state
            .read()
            .map_err(|_| DartImError::internal("client lock poisoned"))?;
        guard
            .inner
            .as_ref()
            .cloned()
            .ok_or_else(|| DartImError::object_closed("DartImClient"))
    }

    pub(crate) fn clone_core(&self) -> Result<im_core::ImCore, DartImError> {
        let guard = self
            .state
            .read()
            .map_err(|_| DartImError::internal("client lock poisoned"))?;
        if guard.inner.is_none() {
            return Err(DartImError::object_closed("DartImClient"));
        }
        guard
            .core
            .clone()
            .ok_or_else(|| DartImError::internal("client Core handle is unavailable"))
    }

    pub(crate) fn replace_inner(
        &self,
        expected_identity_id: &im_core::ids::IdentityId,
        inner: im_core::ImClient,
    ) -> Result<bool, DartImError> {
        let mut guard = self
            .state
            .write()
            .map_err(|_| DartImError::internal("client lock poisoned"))?;
        let current = guard
            .inner
            .as_ref()
            .ok_or_else(|| DartImError::object_closed("DartImClient"))?;
        if &current.current_identity().id != expected_identity_id {
            return Err(DartImError::internal(
                "refreshed client identity does not match the active client",
            ));
        }
        let (refreshed, authorization_context_changed) = current
            .refresh_runtime_from(inner)
            .map_err(DartImError::from)?;
        guard.inner = Some(refreshed);
        Ok(authorization_context_changed)
    }

    pub(crate) async fn lock_runtime_refresh(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.runtime_refresh_lock.lock().await
    }

    pub(crate) fn close(&self) -> Result<(), DartImError> {
        let mut guard = self
            .state
            .write()
            .map_err(|_| DartImError::internal("client lock poisoned"))?;
        guard.inner = None;
        Ok(())
    }
}

pub async fn core_client(
    core: &Arc<crate::api::core::DartImCore>,
    selector: DartIdentitySelector,
) -> Result<Arc<DartImClient>, DartImError> {
    let inner = core.clone_inner()?;
    let client = inner
        .client_async(selector.try_into()?)
        .await
        .map_err(DartImError::from)?;
    Ok(Arc::new(DartImClient::new_with_core(inner, client)))
}

pub fn close_client(client: &Arc<DartImClient>) -> Result<(), DartImError> {
    client.close()
}

pub async fn current_identity(
    client: &Arc<DartImClient>,
) -> Result<crate::dto::identity::DartIdentitySummary, DartImError> {
    let inner = client.clone_inner()?;
    Ok(inner.current_identity().clone().into())
}
