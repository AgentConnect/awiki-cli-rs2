use crate::dto::error::DartImError;
use std::sync::Arc;

pub(crate) type CapabilityUpdates =
    tokio::sync::watch::Receiver<im_core::backend_capabilities::CapabilityState>;

struct SubscriptionState {
    updates: Option<CapabilityUpdates>,
    source: Option<tokio::sync::watch::Receiver<Option<CapabilityUpdates>>>,
}

pub struct DartCapabilitySubscription {
    state: tokio::sync::Mutex<SubscriptionState>,
    closed: tokio::sync::watch::Sender<bool>,
}
impl DartCapabilitySubscription {
    fn new(
        mut updates: tokio::sync::watch::Receiver<im_core::backend_capabilities::CapabilityState>,
    ) -> Arc<Self> {
        updates.mark_changed();
        let (closed, _) = tokio::sync::watch::channel(false);
        Arc::new(Self {
            state: tokio::sync::Mutex::new(SubscriptionState {
                updates: Some(updates),
                source: None,
            }),
            closed,
        })
    }

    fn new_local(mut source: tokio::sync::watch::Receiver<Option<CapabilityUpdates>>) -> Arc<Self> {
        let mut updates = source.borrow_and_update().clone();
        if let Some(updates) = &mut updates {
            updates.mark_changed();
        }
        let (closed, _) = tokio::sync::watch::channel(false);
        Arc::new(Self {
            state: tokio::sync::Mutex::new(SubscriptionState {
                updates,
                source: Some(source),
            }),
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
    Ok(DartCapabilitySubscription::new_local(
        instance.subscribe_capability_source()?,
    ))
}
pub async fn next_capability_state(
    subscription: &Arc<DartCapabilitySubscription>,
) -> Result<Option<String>, DartImError> {
    let mut closed = subscription.closed.subscribe();
    if *closed.borrow() {
        return Ok(None);
    }
    let mut state = subscription.state.lock().await;
    loop {
        let SubscriptionState { updates, source } = &mut *state;
        let Some(current) = updates.as_mut() else {
            return Ok(None);
        };
        tokio::select! {
            biased;
            _ = closed.changed() => return Ok(None),
            result = async {
                match source {
                    Some(source) => source.changed().await,
                    None => std::future::pending().await,
                }
            } => {
                if result.is_err() { return Ok(None); }
                // Rebind before consuming any queued result from the previous
                // authorization context, and emit the new context immediately.
                *updates = source.as_mut().unwrap().borrow_and_update().clone();
                if let Some(updates) = updates { updates.mark_changed(); }
            },
            result = current.changed() => {
                if source.as_ref().is_some_and(|source| source.has_changed().unwrap_or(true)) {
                    continue;
                }
                if result.is_err() { return Ok(None); }
                return Ok(Some(serde_json::json!(*current.borrow_and_update()).to_string()));
            }
        }
    }
}
pub fn close_capability_subscription(subscription: &Arc<DartCapabilitySubscription>) {
    subscription.closed.send_replace(true);
}

#[cfg(test)]
#[path = "capabilities_tests.rs"]
mod tests;
