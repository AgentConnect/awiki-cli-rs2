//! Shared connection/receive lifetime for P5 and P6 prepared state. Being
//! connected is necessary for reuse, but never sufficient authorization.
use std::collections::BTreeMap;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct OwnerScope {
    pub owner: String,
    pub did: String,
    pub device: String,
    pub generation: u64,
    pub signing_key: String,
    pub e2ee_key: String,
}

pub(crate) fn owner_scope(client: &crate::ImClient) -> crate::ImResult<OwnerScope> {
    let auth = crate::internal::identity_device_state::active_authorization_for_client(client)?;
    Ok(OwnerScope {
        owner: client.current_identity().id.as_str().into(),
        did: client.did().as_str().into(),
        device: auth.protocol_device_id.as_str().into(),
        generation: auth.auth_generation,
        signing_key: auth.signing_key_id,
        e2ee_key: auth.e2ee_key_id,
    })
}

struct Session {
    connection: String,
    observation: String,
}

#[derive(Default)]
pub(crate) struct SendConnections {
    sessions: BTreeMap<OwnerScope, Session>,
}

pub(crate) fn connection_token(client: &crate::ImClient) -> Option<String> {
    let owner = owner_scope(client).ok()?;
    client
        .core_inner()
        .send_connections
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .sessions
        .get(&owner)
        .map(|session| session.observation.clone())
}

#[cfg(feature = "internal-test-helpers")]
pub(crate) fn diagnostics_for_test(client: &crate::ImClient) -> serde_json::Value {
    let Ok(owner) = owner_scope(client) else {
        return serde_json::json!({"scope_valid":false});
    };
    let state = client
        .core_inner()
        .send_connections
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let known: Vec<_> = state
        .sessions
        .keys()
        .filter(|key| key.owner == owner.owner)
        .collect();
    serde_json::json!({
        "scope_valid":true,
        "owner_connections":known.len(),
        "scope_matches":state.sessions.contains_key(&owner),
        "did_matches":known.iter().any(|key| key.did == owner.did),
        "device_matches":known.iter().any(|key| key.device == owner.device),
        "generation_matches":known.iter().any(|key| key.generation == owner.generation),
        "signing_key_matches":known.iter().any(|key| key.signing_key == owner.signing_key),
        "e2ee_key_matches":known.iter().any(|key| key.e2ee_key == owner.e2ee_key),
    })
}

pub(crate) fn invalidate_sync(client: &crate::ImClient) {
    let Ok(owner) = owner_scope(client) else {
        return;
    };
    if let Some(session) = client
        .core_inner()
        .send_connections
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .sessions
        .get_mut(&owner)
    {
        session.observation = crate::internal::wire::common::generate_operation_id();
    }
}

pub(crate) struct ConnectionGuard {
    client: crate::ImClient,
    owner: OwnerScope,
    token: String,
}

impl ConnectionGuard {
    pub(crate) fn connected(client: &crate::ImClient) -> crate::ImResult<Self> {
        let owner = owner_scope(client)?;
        let token = crate::internal::wire::common::generate_operation_id();
        {
            let mut state = client
                .core_inner()
                .send_connections
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state
                .sessions
                .retain(|key, _| key.owner != owner.owner || *key == owner);
            state.sessions.insert(
                owner.clone(),
                Session {
                    connection: token.clone(),
                    observation: token.clone(),
                },
            );
        }
        // Cached facts remain inert until a complete observation on this new
        // token confirms them. Reconnect must not manufacture a roster scan for
        // every unchanged group merely because the socket changed.
        Ok(Self {
            client: client.clone(),
            owner,
            token,
        })
    }
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        let mut state = self
            .client
            .core_inner()
            .send_connections
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state
            .sessions
            .get(&self.owner)
            .is_some_and(|session| session.connection == self.token)
        {
            state.sessions.remove(&self.owner);
        }
    }
}

pub(crate) struct ReceiveGuard {
    client: crate::ImClient,
    completed: bool,
}
impl ReceiveGuard {
    pub(crate) fn new(client: &crate::ImClient) -> Self {
        Self {
            client: client.clone(),
            completed: false,
        }
    }
    pub(crate) fn finish(mut self, completed: bool) {
        self.completed = completed;
    }
}
impl Drop for ReceiveGuard {
    fn drop(&mut self) {
        if !self.completed {
            invalidate_sync(&self.client);
        }
    }
}
