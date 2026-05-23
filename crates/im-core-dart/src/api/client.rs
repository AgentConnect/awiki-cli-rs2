use std::sync::{Arc, RwLock};

use crate::dto::{error::DartImError, identity::DartIdentitySelector};

pub struct DartImClient {
    state: Arc<RwLock<DartImClientState>>,
}

struct DartImClientState {
    inner: Option<im_core::ImClient>,
    default_service_did: Option<String>,
}

impl DartImClient {
    pub(crate) fn new(inner: im_core::ImClient, default_service_did: Option<String>) -> Self {
        Self {
            state: Arc::new(RwLock::new(DartImClientState {
                inner: Some(inner),
                default_service_did,
            })),
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

    pub(crate) fn close(&self) -> Result<(), DartImError> {
        let mut guard = self
            .state
            .write()
            .map_err(|_| DartImError::internal("client lock poisoned"))?;
        guard.inner = None;
        Ok(())
    }

    pub(crate) fn default_service_did(&self) -> Result<Option<String>, DartImError> {
        let guard = self
            .state
            .read()
            .map_err(|_| DartImError::internal("client lock poisoned"))?;
        if guard.inner.is_none() {
            return Err(DartImError::object_closed("DartImClient"));
        }
        Ok(guard.default_service_did.clone())
    }
}

pub fn core_client(
    core: Arc<crate::api::core::DartImCore>,
    selector: DartIdentitySelector,
) -> Result<Arc<DartImClient>, DartImError> {
    let default_service_did = core.default_service_did()?;
    core.with_inner(|inner| {
        let client = inner
            .client(selector.try_into()?)
            .map_err(DartImError::from)?;
        Ok(Arc::new(DartImClient::new(client, default_service_did)))
    })
}

pub fn close_client(client: Arc<DartImClient>) -> Result<(), DartImError> {
    client.close()
}

pub fn current_identity(
    client: Arc<DartImClient>,
) -> Result<crate::dto::identity::DartIdentitySummary, DartImError> {
    client.with_inner(|inner| Ok(inner.current_identity().clone().into()))
}
