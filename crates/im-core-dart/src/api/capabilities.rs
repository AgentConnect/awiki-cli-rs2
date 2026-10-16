use crate::dto::error::DartImError;
use std::sync::Arc;

pub struct DartCapabilitySubscription {
    updates: tokio::sync::Mutex<
        tokio::sync::watch::Receiver<im_core::backend_capabilities::CapabilityState>,
    >,
    closed: tokio::sync::watch::Sender<bool>,
}
impl DartCapabilitySubscription {
    fn new(
        mut updates: tokio::sync::watch::Receiver<im_core::backend_capabilities::CapabilityState>,
    ) -> Arc<Self> {
        updates.mark_changed();
        let (closed, _) = tokio::sync::watch::channel(false);
        Arc::new(Self {
            updates: tokio::sync::Mutex::new(updates),
            closed,
        })
    }
}
pub async fn public_capability_snapshot(
    instance: &Arc<crate::api::core::DartImCore>,
) -> Result<String, DartImError> {
    Ok(serde_json::json!(
        instance
            .clone_inner()?
            .backend_capabilities_snapshot_async()
            .await
    )
    .to_string())
}
pub async fn local_capability_snapshot(
    instance: &Arc<crate::api::client::DartImClient>,
) -> Result<String, DartImError> {
    Ok(serde_json::json!(
        instance
            .clone_inner()?
            .backend_capabilities_snapshot_async()
            .await
    )
    .to_string())
}
pub fn subscribe_public_capabilities(
    instance: &Arc<crate::api::core::DartImCore>,
) -> Result<Arc<DartCapabilitySubscription>, DartImError> {
    Ok(DartCapabilitySubscription::new(
        instance.clone_inner()?.subscribe_backend_capabilities(),
    ))
}
pub fn subscribe_local_capabilities(
    instance: &Arc<crate::api::client::DartImClient>,
) -> Result<Arc<DartCapabilitySubscription>, DartImError> {
    Ok(DartCapabilitySubscription::new(
        instance.clone_inner()?.subscribe_backend_capabilities(),
    ))
}
pub async fn next_capability_state(
    subscription: &Arc<DartCapabilitySubscription>,
) -> Result<Option<String>, DartImError> {
    let mut closed = subscription.closed.subscribe();
    if *closed.borrow() {
        return Ok(None);
    }
    let mut updates = subscription.updates.lock().await;
    tokio::select! {
        _ = closed.changed() => Ok(None),
        result = updates.changed() => {
            if result.is_err() { return Ok(None); }
            Ok(Some(serde_json::json!(*updates.borrow_and_update()).to_string()))
        }
    }
}
pub fn close_capability_subscription(subscription: &Arc<DartCapabilitySubscription>) {
    subscription.closed.send_replace(true);
}
