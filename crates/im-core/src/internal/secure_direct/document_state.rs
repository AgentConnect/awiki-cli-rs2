//! Reuse method-validated documents only against synchronized local User hashes.
//! Cross-Home and independently hosted DID methods keep explicit resolution.
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::internal::message_runtime::send_lifecycle::{self, OwnerScope};
use crate::internal::wire::sync_v2::direct_e2ee_state::IdentityObservations;

struct Observation {
    connection: Option<String>,
    healthy: bool,
    hashes: BTreeMap<String, String>,
}
struct Document {
    hash: String,
    value: Value,
    retry_at: Instant,
}

#[derive(Default)]
pub(crate) struct DirectDocumentState {
    observations: BTreeMap<OwnerScope, Observation>,
    documents: BTreeMap<(OwnerScope, String), Document>,
}

fn local_user_document(client: &crate::ImClient, did: &str) -> bool {
    let domain = &client.core_inner().sdk_config().did_domain;
    let wba = format!("did:wba:{domain}:");
    let web = format!("did:web:{domain}:");
    let Some(path) = did.strip_prefix(&wba).or_else(|| did.strip_prefix(&web)) else {
        return false;
    };
    let parts: Vec<_> = path.split(':').collect();
    let owned_namespace = matches!(parts.as_slice(), ["awiki", "web", _])
        || (did.starts_with(&wba)
            && (matches!(parts.as_slice(), ["user", _] | ["user", _, _])
                || matches!(
                    parts.as_slice(),
                    ["agent", "runtime" | "skill" | "daemon", _, _]
                        | ["agent", "runtime" | "skill" | "daemon", _]
                )));
    owned_namespace
        && parts.iter().all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        })
}

fn digest(document: &Value) -> crate::ImResult<(String, usize)> {
    let bytes =
        serde_json_canonicalizer::to_vec(document).map_err(|_| crate::ImError::PermissionDenied)?;
    Ok((
        format!("sha256:{}", URL_SAFE_NO_PAD.encode(Sha256::digest(&bytes))),
        bytes.len(),
    ))
}

pub(crate) fn invalidate(client: &crate::ImClient) {
    let Ok(scope) = send_lifecycle::owner_scope(client) else {
        return;
    };
    if let Some(observation) = client
        .core_inner()
        .direct_document_state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .observations
        .get_mut(&scope)
    {
        observation.healthy = false;
    }
}

pub(crate) fn observe_hint(client: &crate::ImClient, notification: &Value) {
    if notification.get("method").and_then(Value::as_str) == Some("sync.changed")
        && (notification
            .pointer("/params/reason")
            .and_then(Value::as_str)
            == Some("e2ee_identity_changed")
            || notification
                .pointer("/params/domains")
                .and_then(Value::as_array)
                .is_some_and(|domains| {
                    domains
                        .iter()
                        .any(|domain| domain.as_str() == Some("device_registry"))
                }))
    {
        invalidate(client);
        // Fence any delta already in flight. Its older response must not
        // restore a document made dirty by a later identity notification.
        send_lifecycle::invalidate_sync(client);
    }
}

pub(crate) fn get(client: &crate::ImClient, did: &str) -> crate::ImResult<Option<Value>> {
    if !local_user_document(client, did) {
        return Ok(None);
    }
    let scope = send_lifecycle::owner_scope(client)?;
    let Some(connection) = send_lifecycle::connection_token(client) else {
        return Ok(None);
    };
    let (value, hash) = {
        let state = client
            .core_inner()
            .direct_document_state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(observation) = state.observations.get(&scope).filter(|observed| {
            observed.healthy && observed.connection.as_ref() == Some(&connection)
        }) else {
            return Ok(None);
        };
        let Some(document) = state.documents.get(&(scope.clone(), did.to_owned())) else {
            return Ok(None);
        };
        if observation.hashes.get(did) != Some(&document.hash) {
            return Ok(None);
        }
        (document.value.clone(), document.hash.clone())
    };
    // Time-sensitive method checks and per-device P5 checks still run on every
    // use. A matching byte digest is not a replacement for those checks.
    let value = match crate::internal::discovery::did_document::validate_resolved_did_document(
        did, value,
    ) {
        Ok(value) => value,
        Err(error) => {
            invalidate(client);
            return Err(error);
        }
    };
    if send_lifecycle::connection_token(client).as_ref() != Some(&connection) {
        return Ok(None);
    }
    let state = client
        .core_inner()
        .direct_document_state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !state.observations.get(&scope).is_some_and(|observed| {
        observed.healthy
            && observed.connection.as_ref() == Some(&connection)
            && observed.hashes.get(did) == Some(&hash)
    }) {
        return Ok(None);
    }
    Ok(Some(value))
}

pub(crate) fn remember(
    client: &crate::ImClient,
    did: &str,
    document: &Value,
) -> crate::ImResult<()> {
    if !local_user_document(client, did) {
        return Ok(());
    }
    let scope = send_lifecycle::owner_scope(client)?;
    let (hash, bytes) = digest(document)?;
    let mut state = client
        .core_inner()
        .direct_document_state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(expected) = state
        .observations
        .get(&scope)
        .filter(|observation| {
            observation.healthy
                && observation.connection.is_some()
                && observation.connection == send_lifecycle::connection_token(client)
        })
        .and_then(|observation| observation.hashes.get(did))
    {
        if expected != &hash {
            if let Some(observation) = state.observations.get_mut(&scope) {
                observation.healthy = false;
            }
            return Err(crate::ImError::CursorStale);
        }
    }
    if bytes > 256 * 1024 {
        return Ok(());
    }
    if state.documents.len() >= 128
        && !state
            .documents
            .contains_key(&(scope.clone(), did.to_owned()))
    {
        state.documents.clear();
    }
    state.documents.insert(
        (scope, did.to_owned()),
        Document {
            hash,
            value: document.clone(),
            retry_at: Instant::now(),
        },
    );
    Ok(())
}

pub(crate) async fn observe_async(
    client: &crate::ImClient,
    observations: Option<&IdentityObservations>,
    connection: Option<String>,
    healthy: bool,
) -> crate::ImResult<()> {
    let scope = send_lifecycle::owner_scope(client)?;
    let Some(observations) = observations.filter(|observations| observations.complete) else {
        client
            .core_inner()
            .direct_document_state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .observations
            .remove(&scope);
        return Ok(());
    };
    let hashes: BTreeMap<_, _> = observations
        .identities
        .iter()
        .filter(|entry| local_user_document(client, &entry.did))
        .map(|entry| (entry.did.clone(), entry.document_hash.clone()))
        .collect();
    let refresh = {
        let mut state = client
            .core_inner()
            .direct_document_state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state
            .observations
            .retain(|owner, _| owner.owner != scope.owner || *owner == scope);
        state
            .documents
            .retain(|(owner, _), _| owner.owner != scope.owner || *owner == scope);
        let mut refresh = Vec::new();
        for ((owner, did), cached) in &mut state.documents {
            if *owner == scope
                && healthy
                && connection.is_some()
                && cached.retry_at <= Instant::now()
                && hashes.get(did).is_some_and(|hash| hash != &cached.hash)
            {
                refresh.push(did.clone());
                cached.retry_at = Instant::now() + Duration::from_secs(5);
            }
        }
        state.observations.insert(
            scope,
            Observation {
                connection: connection.clone(),
                healthy,
                hashes,
            },
        );
        refresh
    };
    if connection != send_lifecycle::connection_token(client) {
        invalidate(client);
        return Ok(());
    }
    // Only already used, changed documents are refreshed. No startup crawl over
    // old contacts, and no extra periodic request source is introduced.
    use futures_util::{stream, StreamExt};
    stream::iter(refresh)
        .map(|did| async move {
            let document = crate::internal::discovery::did_document::resolve_did_document_async(
                &mut crate::internal::transport::CoreHttpTransport::new(client),
                &did,
            )
            .await;
            if let Ok(document) = document {
                let _ = remember(client, &did, &document);
            }
        })
        .buffer_unordered(4)
        .collect::<Vec<_>>()
        .await;
    Ok(())
}
