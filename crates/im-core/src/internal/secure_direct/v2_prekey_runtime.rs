//! Product-owned P5 v2 PreKey lifecycle and strict wire adapters.
//!
//! Secure attachment grant refs use the AWiki sender-home private adapter and
//! are never merged into the standard cross-domain `direct.send` request.

use anp::direct_e2ee::{
    build_prekey_bundle_v2, get_prekey_bundle_request_v2, key_service_metadata_v2,
    parse_get_prekey_bundle_result_v2, parse_publish_prekey_bundle_result_v2,
    publish_prekey_bundle_request_v2, verify_prekey_bundle_v2, V2GetPrekeyBundleBody,
    V2GetPrekeyBundleResult, V2OneTimePrekey, V2PrekeyBundle, V2PublishPrekeyBundleBody,
    V2PublishPrekeyBundleResult, V2SignedPrekey, MTI_DIRECT_E2EE_SUITE_V2,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use rand::rngs::OsRng;
use sha2::{Digest as _, Sha256};
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret as X25519StaticSecret};

use super::v2_store::SqliteV2DirectStateStore;
use crate::internal::transport::{AsyncAuthenticatedRestTransport, AsyncAuthenticatedRpcTransport};

const DEFAULT_OPK_BATCH_SIZE: usize = 16;
const SIGNED_PREKEY_LIFETIME_DAYS: i64 = 30;
const DIRECT_E2EE_ATTACHMENT_ENDPOINT: &str = "/im/private/direct-e2ee-attachment";

pub(crate) struct V2LocalPrekeyIdentity<'a> {
    pub(crate) did: &'a str,
    pub(crate) device_id: &'a str,
    pub(crate) signing_key_id: &'a str,
    pub(crate) e2ee_key_id: &'a str,
    pub(crate) signing_private: &'a anp::PrivateKeyMaterial,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct V2LocalPrekeyPublication {
    pub(crate) bundle: V2PrekeyBundle,
    pub(crate) one_time_prekeys: Vec<V2OneTimePrekey>,
}

/// Returns stable local material across retries, generating and Vault-sealing
/// a new batch before any publish request can leave the process.
pub(crate) fn ensure_local_prekey_publication(
    store: &SqliteV2DirectStateStore<'_>,
    identity: V2LocalPrekeyIdentity<'_>,
    now: DateTime<Utc>,
) -> crate::ImResult<V2LocalPrekeyPublication> {
    require_exact("did", identity.did)?;
    require_exact("device_id", identity.device_id)?;
    require_exact("signing_key_id", identity.signing_key_id)?;
    require_exact("e2ee_key_id", identity.e2ee_key_id)?;

    if let Some(local) = store.load_active_bundle()? {
        let available = store.load_available_opk_publics(&local.bundle.bundle_id)?;
        let expires_at = DateTime::parse_from_rfc3339(&local.bundle.signed_prekey.expires_at)
            .map_err(|_| crate::ImError::PermissionDenied)?
            .with_timezone(&Utc);
        let proof_signing_key_id = local
            .bundle
            .proof
            .get("verificationMethod")
            .and_then(serde_json::Value::as_str);
        if expires_at > now
            && !available.is_empty()
            && proof_signing_key_id == Some(identity.signing_key_id)
            && local.bundle.static_key_agreement_id == identity.e2ee_key_id
        {
            if let Some(published) = local.published_one_time_prekeys {
                if !available
                    .iter()
                    .all(|remaining| published.iter().any(|original| original == remaining))
                {
                    return Err(crate::ImError::PermissionDenied);
                }
                return Ok(V2LocalPrekeyPublication {
                    bundle: local.bundle,
                    one_time_prekeys: published,
                });
            }
            // Legacy local material did not retain the immutable public
            // publish batch. Reusing its bundle id with the now-smaller OPK
            // set would violate server digest idempotency, so rotate instead.
        }
    }

    let signed_prekey_private = X25519StaticSecret::random_from_rng(OsRng);
    let signed_prekey_public = X25519PublicKey::from(&signed_prekey_private).to_bytes();
    let signed_prekey_id = opaque_key_id("spk", &signed_prekey_public);
    let expires_at = (now + Duration::days(SIGNED_PREKEY_LIFETIME_DAYS))
        .to_rfc3339_opts(SecondsFormat::Secs, true);
    let signed_prekey = V2SignedPrekey {
        key_id: signed_prekey_id,
        public_key_b64u: URL_SAFE_NO_PAD.encode(signed_prekey_public),
        expires_at,
    };
    let bundle_id = opaque_key_id("bundle", &signed_prekey_public);
    let created = now.to_rfc3339_opts(SecondsFormat::Secs, true);
    let bundle = build_prekey_bundle_v2(
        &bundle_id,
        identity.did,
        identity.device_id,
        identity.e2ee_key_id,
        signed_prekey,
        identity.signing_private,
        identity.signing_key_id,
        Some(&created),
    )
    .map_err(v2_error)?;

    let mut local_opks = Vec::with_capacity(DEFAULT_OPK_BATCH_SIZE);
    for _ in 0..DEFAULT_OPK_BATCH_SIZE {
        let private = X25519StaticSecret::random_from_rng(OsRng);
        let public_bytes = X25519PublicKey::from(&private).to_bytes();
        local_opks.push((
            V2OneTimePrekey {
                key_id: opaque_key_id("opk", &public_bytes),
                public_key_b64u: URL_SAFE_NO_PAD.encode(public_bytes),
            },
            private,
        ));
    }
    local_opks.sort_by(|left, right| left.0.key_id.cmp(&right.0.key_id));
    store.publish_local_bundle(&bundle, &signed_prekey_private, &local_opks, &created)?;
    Ok(V2LocalPrekeyPublication {
        bundle,
        one_time_prekeys: local_opks.into_iter().map(|(public, _)| public).collect(),
    })
}

pub(crate) fn publish_request(
    publication: &V2LocalPrekeyPublication,
    service_did: &str,
    operation_id: &str,
) -> crate::ImResult<serde_json::Value> {
    let meta = key_service_metadata_v2(
        &publication.bundle.owner_did,
        &publication.bundle.owner_device_id,
        require_exact("service_did", service_did)?,
        require_exact("operation_id", operation_id)?,
    );
    publish_prekey_bundle_request_v2(
        meta,
        V2PublishPrekeyBundleBody {
            prekey_bundle: publication.bundle.clone(),
            one_time_prekeys: publication.one_time_prekeys.clone(),
        },
    )
    .map_err(v2_error)
}

pub(crate) fn validate_publish_result(
    value: &serde_json::Value,
    publication: &V2LocalPrekeyPublication,
) -> crate::ImResult<V2PublishPrekeyBundleResult> {
    let result = parse_publish_prekey_bundle_result_v2(value).map_err(v2_error)?;
    if result.owner_did != publication.bundle.owner_did
        || result.owner_device_id != publication.bundle.owner_device_id
        || result.bundle_id != publication.bundle.bundle_id
        || result
            .published_opk_count
            .is_some_and(|count| count != publication.one_time_prekeys.len() as u64)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(result)
}

pub(crate) fn get_request(
    local_did: &str,
    local_device_id: &str,
    service_did: &str,
    operation_id: &str,
    target_did: &str,
    target_device_id: &str,
    require_opk: bool,
) -> crate::ImResult<serde_json::Value> {
    let meta = key_service_metadata_v2(
        require_exact("local_did", local_did)?,
        require_exact("local_device_id", local_device_id)?,
        require_exact("service_did", service_did)?,
        require_exact("operation_id", operation_id)?,
    );
    get_prekey_bundle_request_v2(
        meta,
        V2GetPrekeyBundleBody {
            target_did: require_exact("target_did", target_did)?.to_owned(),
            target_device_id: require_exact("target_device_id", target_device_id)?.to_owned(),
            preferred_suite: Some(MTI_DIRECT_E2EE_SUITE_V2.to_owned()),
            require_opk: Some(require_opk),
        },
    )
    .map_err(v2_error)
}

pub(crate) fn verify_get_result(
    value: &serde_json::Value,
    target_did: &str,
    target_device_id: &str,
    target_did_document: &serde_json::Value,
    now: DateTime<Utc>,
    require_opk: bool,
) -> crate::ImResult<V2GetPrekeyBundleResult> {
    let result = parse_get_prekey_bundle_result_v2(value).map_err(v2_error)?;
    if result.target_did != target_did
        || result.target_device_id != target_device_id
        || (require_opk && result.one_time_prekey.is_none())
    {
        return Err(crate::ImError::PermissionDenied);
    }
    verify_prekey_bundle_v2(&result.prekey_bundle, target_did_document, now).map_err(v2_error)?;
    Ok(result)
}

pub(crate) async fn ensure_local_prekey_published(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
) -> crate::ImResult<V2LocalPrekeyPublication> {
    let mut resolver = crate::internal::transport::CoreHttpTransport::new(client);
    let did_document = crate::internal::discovery::did_document::resolve_did_document_async(
        &mut resolver,
        client.did().as_str(),
    )
    .await?;
    let mut publisher = crate::internal::transport::CoreHttpTransport::new(client);
    ensure_local_prekey_published_with_document(core, client, &did_document, &mut publisher).await
}

/// Publishes from a locally persisted, server-authorized vNext DID Document.
pub(crate) async fn ensure_local_prekey_published_from_authorized_document(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
) -> crate::ImResult<V2LocalPrekeyPublication> {
    let mut publisher = crate::internal::transport::CoreHttpTransport::new(client);
    ensure_local_prekey_published_from_authorized_document_with_transport(
        core,
        client,
        &mut publisher,
    )
    .await
}

pub(crate) async fn ensure_local_prekey_published_from_authorized_document_with_transport<T>(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    publisher: &mut T,
) -> crate::ImResult<V2LocalPrekeyPublication>
where
    T: AsyncAuthenticatedRpcTransport,
{
    let did_document = client.runtime().key_provider.did_document()?;
    if did_document.get("id").and_then(serde_json::Value::as_str) != Some(client.did().as_str())
        || (client.did().as_str().starts_with("did:wba:")
            && !anp::authentication::validate_did_document_binding(&did_document, true))
    {
        return Err(crate::ImError::PermissionDenied);
    }
    ensure_local_prekey_published_with_document(core, client, &did_document, publisher).await
}

async fn ensure_local_prekey_published_with_document<T>(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    did_document: &serde_json::Value,
    publisher: &mut T,
) -> crate::ImResult<V2LocalPrekeyPublication>
where
    T: AsyncAuthenticatedRpcTransport,
{
    let (scope, authorization) = local_scope_and_authorization(core, client)?;
    let eligible = anp::authentication::find_eligible_device(
        did_document,
        authorization.protocol_device_id.as_str(),
        anp::authentication::PROFILE_DIRECT_E2EE_V2,
    )
    .map_err(|_| crate::ImError::PermissionDenied)?
    .ok_or(crate::ImError::PermissionDenied)?;
    if eligible.signing_key_id != authorization.signing_key_id
        || eligible.e2ee_key_id != authorization.e2ee_key_id
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let signing_private = anp::PrivateKeyMaterial::from_pem(
        &client
            .runtime()
            .key_provider
            .device_request_signing_private_pem()?,
    )
    .map_err(|_| crate::ImError::PermissionDenied)?;
    let publication = {
        let connection = crate::internal::local_state::open_writable(
            &core.inner().sdk_paths().local_state.sqlite_path,
        )?;
        let vault = core
            .inner()
            .identity_vault()
            .ok_or(crate::ImError::IdentityVault {
                failure: crate::IdentityVaultFailure::Unavailable,
            })?
            .vault();
        let store = SqliteV2DirectStateStore::new_with_secret_vault(&connection, vault, scope)?;
        ensure_local_prekey_publication(
            &store,
            V2LocalPrekeyIdentity {
                did: client.did().as_str(),
                device_id: authorization.protocol_device_id.as_str(),
                signing_key_id: &authorization.signing_key_id,
                e2ee_key_id: &authorization.e2ee_key_id,
                signing_private: &signing_private,
            },
            Utc::now(),
        )?
    };

    let service_did = anp::direct_e2ee::message_service_did_from_document(did_document)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let operation_id = format!(
        "p5-v2-prekey-publish:{}:{}",
        authorization.protocol_device_id.as_str(),
        publication.bundle.bundle_id
    );
    let request = publish_request(&publication, &service_did, &operation_id)?;
    let (method, params) = split_rpc_request(request)?;
    let response = publisher
        .authenticated_rpc("/im/rpc", &method, params)
        .await?;
    validate_publish_result(&response, &publication)?;
    Ok(publication)
}

pub(crate) async fn fetch_verified_prekey(
    client: &crate::core::ImClient,
    target_did: &str,
    target_device_id: &str,
    target_did_document: &serde_json::Value,
    operation_seed: &str,
) -> crate::ImResult<V2GetPrekeyBundleResult> {
    require_exact("operation_seed", operation_seed)?;
    let service_did = target_prekey_service_did(target_did, target_did_document)?;
    let local_state = local_authorization(client)?;
    // One logical query: OPK is an optional response sidecar. A transport
    // retry reuses the exact operation ID and body; it is never a second
    // business query asking for a different OPK policy.
    let operation_id = format!("p5-v2-prekey-get:{operation_seed}");
    let request = get_request(
        client.did().as_str(),
        local_state.protocol_device_id.as_str(),
        &service_did,
        &operation_id,
        target_did,
        target_device_id,
        false,
    )?;
    let (method, params) = split_rpc_request(request)?;
    for attempt in 0..=1 {
        let response = crate::internal::transport::CoreHttpTransport::new(client)
            .authenticated_rpc("/im/rpc", &method, params.clone())
            .await;
        match response {
            Ok(response) => {
                return verify_get_result(
                    &response,
                    target_did,
                    target_device_id,
                    target_did_document,
                    Utc::now(),
                    false,
                );
            }
            Err(error) if attempt == 0 && retryable_prekey_transport_error(&error) => continue,
            Err(error) => return Err(error),
        }
    }
    unreachable!("the bounded PreKey attempt loop always returns")
}

fn retryable_prekey_transport_error(error: &crate::ImError) -> bool {
    matches!(
        error,
        crate::ImError::TransportUnavailable { .. }
            | crate::ImError::Io { .. }
            | crate::ImError::Service {
                status_code: Some(500..=599),
                ..
            }
    )
}

fn target_prekey_service_did(
    target_did: &str,
    target_did_document: &serde_json::Value,
) -> crate::ImResult<String> {
    let target_did = require_exact("target_did", target_did)?;
    if target_did_document
        .get("id")
        .and_then(serde_json::Value::as_str)
        != Some(target_did)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    anp::direct_e2ee::message_service_did_from_document(target_did_document)
        .map_err(|_| crate::ImError::PermissionDenied)
}

pub(crate) async fn post_standard_direct(
    client: &crate::core::ImClient,
    prepared: &super::v2_runtime::PreparedV2Outbound,
) -> crate::ImResult<anp::direct_e2ee::V2DirectSendResult> {
    let (method, params) = split_rpc_request(prepared.direct_request()?)?;
    let response = crate::internal::transport::CoreHttpTransport::new(client)
        .authenticated_rpc("/im/rpc", &method, params)
        .await?;
    super::v2_runtime::parse_send_result(&response, prepared)
}

pub(crate) async fn post_standard_direct_attachment(
    client: &crate::core::ImClient,
    prepared: &super::v2_runtime::PreparedV2Outbound,
    attachment_grant_ref: &serde_json::Value,
) -> crate::ImResult<anp::direct_e2ee::V2DirectSendResult> {
    let body = direct_attachment_http_body(
        prepared.direct_request()?,
        &prepared.metadata.operation_id,
        attachment_grant_ref,
    )?;
    let response = crate::internal::transport::CoreHttpTransport::new(client)
        .authenticated_rest_post(DIRECT_E2EE_ATTACHMENT_ENDPOINT, "POST", body)
        .await?;
    super::v2_runtime::parse_send_result(&response, prepared)
}

fn direct_attachment_http_body(
    mut direct_request: serde_json::Value,
    request_id: &str,
    attachment_grant_ref: &serde_json::Value,
) -> crate::ImResult<serde_json::Value> {
    let request = direct_request
        .as_object_mut()
        .ok_or(crate::ImError::PermissionDenied)?;
    if request_id.is_empty()
        || request_id.trim() != request_id
        || !attachment_grant_ref.is_object()
        || request.contains_key("jsonrpc")
        || request.contains_key("id")
    {
        return Err(crate::ImError::PermissionDenied);
    }
    request.insert(
        "jsonrpc".to_owned(),
        serde_json::Value::String("2.0".to_owned()),
    );
    request.insert(
        "id".to_owned(),
        serde_json::Value::String(request_id.to_owned()),
    );
    Ok(serde_json::json!({
        "direct_request": direct_request,
        "attachment_grant_refs": [attachment_grant_ref],
    }))
}

pub(crate) fn local_static_private(
    client: &crate::core::ImClient,
) -> crate::ImResult<X25519StaticSecret> {
    match anp::PrivateKeyMaterial::from_pem(
        &client.runtime().key_provider.e2ee_agreement_private_pem()?,
    )
    .map_err(|_| crate::ImError::PermissionDenied)?
    {
        anp::PrivateKeyMaterial::X25519(private) => Ok(private),
        _ => Err(crate::ImError::PermissionDenied),
    }
}

pub(crate) fn static_public_from_document(
    did_document: &serde_json::Value,
    key_id: &str,
) -> crate::ImResult<[u8; 32]> {
    let method = did_document
        .get("verificationMethod")
        .and_then(serde_json::Value::as_array)
        .and_then(|methods| {
            methods
                .iter()
                .find(|method| method.get("id").and_then(serde_json::Value::as_str) == Some(key_id))
        })
        .ok_or(crate::ImError::PermissionDenied)?;
    match anp::authentication::extract_public_key(method)
        .map_err(|_| crate::ImError::PermissionDenied)?
    {
        anp::PublicKeyMaterial::X25519(public) => Ok(public),
        _ => Err(crate::ImError::PermissionDenied),
    }
}

fn local_scope_and_authorization(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
) -> crate::ImResult<(
    super::v2_store::V2OwnerScope,
    crate::internal::identity_device_state::DeviceAuthorizationProjection,
)> {
    let authorization = local_authorization(client)?;
    let alias = client
        .current_identity()
        .local_alias
        .as_deref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let index =
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities)
            .load_index()?;
    let state = index
        .credentials
        .get(alias)
        .and_then(|entry| entry.device_state.as_ref())
        .ok_or(crate::ImError::PermissionDenied)?;
    let scope = super::v2_store::V2OwnerScope::from_identity_state(
        &client.current_identity().id,
        client.did(),
        state,
    )?;
    Ok((scope, authorization))
}

fn local_authorization(
    client: &crate::core::ImClient,
) -> crate::ImResult<crate::internal::identity_device_state::DeviceAuthorizationProjection> {
    let alias = client
        .current_identity()
        .local_alias
        .as_deref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let index = crate::internal::identity_store::IdentityStore::new(
        &client.core_inner().sdk_paths().identities,
    )
    .load_index()?;
    index
        .credentials
        .get(alias)
        .and_then(|entry| entry.device_state.as_ref())
        .and_then(|state| state.authorization.clone())
        .filter(|authorization| {
            authorization.status
                == crate::internal::identity_device_state::DeviceAuthorizationStatus::Active
        })
        .ok_or(crate::ImError::PermissionDenied)
}

fn split_rpc_request(value: serde_json::Value) -> crate::ImResult<(String, serde_json::Value)> {
    let mut object = value
        .as_object()
        .filter(|object| object.len() == 2)
        .cloned()
        .ok_or(crate::ImError::PermissionDenied)?;
    let method = object
        .remove("method")
        .and_then(|value| value.as_str().map(str::to_owned))
        .filter(|value| !value.is_empty())
        .ok_or(crate::ImError::PermissionDenied)?;
    let params = object
        .remove("params")
        .filter(serde_json::Value::is_object)
        .ok_or(crate::ImError::PermissionDenied)?;
    if !object.is_empty() {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok((method, params))
}

fn opaque_key_id(prefix: &str, public: &[u8; 32]) -> String {
    let digest = Sha256::digest(public);
    format!("{prefix}-{}", URL_SAFE_NO_PAD.encode(&digest[..16]))
}

fn require_exact<'a>(field: &str, value: &'a str) -> crate::ImResult<&'a str> {
    if value.is_empty() || value.trim() != value {
        Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            format!("{field} must be a non-empty exact value"),
        ))
    } else {
        Ok(value)
    }
}

fn v2_error(_: anp::direct_e2ee::DirectE2eeV2Error) -> crate::ImError {
    crate::ImError::PermissionDenied
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal::identity_device_state::{
        DeviceAuthorizationProjection, DeviceAuthorizationRole, DeviceAuthorizationStatus,
        IdentityDeviceMode, IdentityDeviceState, IdentityInternalCheckpoint,
        IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
    };
    use crate::internal::secure_direct::secret_store::DirectSecretVault;
    use crate::internal::secure_direct::v2_store::V2OwnerScope;
    use crate::vault::{DeviceVaultRootKey, FileSecretVault, FileSecretVaultStore};
    use rusqlite::Connection;
    use std::sync::Arc;

    #[test]
    fn direct_attachment_wrapper_keeps_grant_ref_outside_standard_request() {
        let direct_request = serde_json::json!({
            "method": "direct.send",
            "params": {
                "meta": {"message_id": "wire-message-1"},
                "body": {"ciphertext_b64u": "opaque"}
            }
        });
        let grant_ref = serde_json::json!({
            "attachment_id": "attachment-1",
            "object_uri": "https://objects.example/attachment-1",
            "size": "16",
            "digest": {"alg": "sha-256", "value_b64u": "digest"},
            "mime_type": "text/plain",
            "object_encryption_mode": "object-e2ee",
            "plaintext_size": "0"
        });

        let body =
            direct_attachment_http_body(direct_request.clone(), "wire-message-1", &grant_ref)
                .unwrap();

        assert_eq!(body["direct_request"]["jsonrpc"], "2.0");
        assert_eq!(body["direct_request"]["id"], "wire-message-1");
        assert_eq!(body["direct_request"]["method"], direct_request["method"]);
        assert_eq!(body["direct_request"]["params"], direct_request["params"]);
        assert_eq!(
            body["attachment_grant_refs"],
            serde_json::json!([grant_ref])
        );
        assert!(body["direct_request"].get("client").is_none());
    }

    #[test]
    fn prekey_get_targets_the_target_agents_message_service() {
        let target_did = "did:wba:remote.example:agents:bob:e1";
        let target_document = serde_json::json!({
            "id": target_did,
            "service": [{
                "id": format!("{target_did}#messages"),
                "type": "ANPMessageService",
                "serviceEndpoint": "https://remote.example/im/rpc",
                "serviceDid": "did:wba:remote.example",
                "profiles": ["anp.direct.e2ee.v2"],
                "securityProfiles": ["direct-e2ee"]
            }]
        });

        let service_did = target_prekey_service_did(target_did, &target_document).unwrap();
        assert_eq!(service_did, "did:wba:remote.example");
        let request = get_request(
            "did:wba:local.example:agents:alice:e1",
            "dev-a",
            &service_did,
            "get-remote-bob",
            target_did,
            "dev-b",
            true,
        )
        .unwrap();
        let (meta, body) = anp::direct_e2ee::parse_get_prekey_bundle_request_v2(&request).unwrap();
        assert_eq!(meta.target.did, "did:wba:remote.example");
        assert_eq!(body.target_did, target_did);
        assert_eq!(body.target_device_id, "dev-b");
        assert!(
            target_prekey_service_did("did:wba:other.example:agents:bob:e1", &target_document)
                .is_err()
        );
    }

    #[test]
    fn local_material_precedes_strict_publish_and_get_requests() {
        let temp = tempfile::tempdir().unwrap();
        let connection = Connection::open(temp.path().join("prekeys.sqlite")).unwrap();
        let did = crate::ids::Did::parse("did:example:alice").unwrap();
        let state = IdentityDeviceState {
            schema_version: IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
            mode: IdentityDeviceMode::VNext,
            authorization: Some(DeviceAuthorizationProjection {
                protocol_device_id: crate::ids::ProtocolDeviceId::parse("alice-phone").unwrap(),
                signing_key_id: "did:example:alice#phone-sign".to_owned(),
                e2ee_key_id: "did:example:alice#phone-e2ee".to_owned(),
                status: DeviceAuthorizationStatus::Active,
                role: DeviceAuthorizationRole::Admin,
                management_ready: true,
                auth_generation: 1,
            }),
            checkpoint: Some(IdentityInternalCheckpoint {
                document_version: 1,
                document_hash: "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned(),
                registry_version: 1,
            }),
        };
        let scope = V2OwnerScope::from_identity_state(
            &crate::ids::IdentityId::parse("identity-alice-phone").unwrap(),
            &did,
            &state,
        )
        .unwrap();
        let vault: DirectSecretVault = Arc::new(FileSecretVault::new(
            DeviceVaultRootKey::from_bytes([71; 32]),
            FileSecretVaultStore::new(temp.path().join("vault")),
        ));
        let store = SqliteV2DirectStateStore::new_with_secret_vault(
            &connection,
            vault.clone(),
            scope.clone(),
        )
        .unwrap();
        let signing =
            anp::PrivateKeyMaterial::Ed25519(ed25519_dalek::SigningKey::from_bytes(&[72; 32]));
        let now = DateTime::parse_from_rfc3339("2026-07-20T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let publication = ensure_local_prekey_publication(
            &store,
            V2LocalPrekeyIdentity {
                did: did.as_str(),
                device_id: "alice-phone",
                signing_key_id: "did:example:alice#phone-sign",
                e2ee_key_id: "did:example:alice#phone-e2ee",
                signing_private: &signing,
            },
            now,
        )
        .unwrap();
        assert_eq!(publication.one_time_prekeys.len(), DEFAULT_OPK_BATCH_SIZE);
        assert!(store.load_active_bundle().unwrap().is_some());
        assert_eq!(
            store
                .load_available_opk_publics(&publication.bundle.bundle_id)
                .unwrap(),
            publication.one_time_prekeys
        );
        let resumed = ensure_local_prekey_publication(
            &store,
            V2LocalPrekeyIdentity {
                did: did.as_str(),
                device_id: "alice-phone",
                signing_key_id: "did:example:alice#phone-sign",
                e2ee_key_id: "did:example:alice#phone-e2ee",
                signing_private: &signing,
            },
            now,
        )
        .unwrap();
        assert_eq!(resumed, publication);

        let original_publish =
            publish_request(&publication, "did:example:message-service", "publish-1").unwrap();
        let consumed_key_id = publication.one_time_prekeys[0].key_id.clone();
        connection
            .execute(
                r#"DELETE FROM direct_e2ee_v2_one_time_prekeys
WHERE owner_identity_id = ?1 AND local_device_id = ?2 AND key_id = ?3"#,
                rusqlite::params!["identity-alice-phone", "alice-phone", consumed_key_id],
            )
            .unwrap();
        drop(store);
        drop(connection);

        let connection = Connection::open(temp.path().join("prekeys.sqlite")).unwrap();
        let store =
            SqliteV2DirectStateStore::new_with_secret_vault(&connection, vault, scope).unwrap();
        let resumed_after_consumption = ensure_local_prekey_publication(
            &store,
            V2LocalPrekeyIdentity {
                did: did.as_str(),
                device_id: "alice-phone",
                signing_key_id: "did:example:alice#phone-sign",
                e2ee_key_id: "did:example:alice#phone-e2ee",
                signing_private: &signing,
            },
            now,
        )
        .unwrap();
        assert_eq!(resumed_after_consumption, publication);
        assert_eq!(
            store
                .load_available_opk_publics(&publication.bundle.bundle_id)
                .unwrap()
                .len(),
            DEFAULT_OPK_BATCH_SIZE - 1
        );
        assert_eq!(
            publish_request(
                &resumed_after_consumption,
                "did:example:message-service",
                "publish-1"
            )
            .unwrap(),
            original_publish
        );

        let publish = original_publish;
        let (meta, body) =
            anp::direct_e2ee::parse_publish_prekey_bundle_request_v2(&publish).unwrap();
        assert_eq!(meta.sender_device_id, "alice-phone");
        assert_eq!(body.one_time_prekeys.len(), DEFAULT_OPK_BATCH_SIZE);
        let publish_result = serde_json::json!({
            "published": true,
            "owner_did": did.as_str(),
            "owner_device_id": "alice-phone",
            "bundle_id": publication.bundle.bundle_id,
            "published_at": "2026-07-20T00:00:01Z",
            "published_opk_count": DEFAULT_OPK_BATCH_SIZE
        });
        validate_publish_result(&publish_result, &publication).unwrap();

        let get = get_request(
            did.as_str(),
            "alice-phone",
            "did:example:message-service",
            "get-1",
            "did:example:bob",
            "bob-laptop",
            true,
        )
        .unwrap();
        let (_, get_body) = anp::direct_e2ee::parse_get_prekey_bundle_request_v2(&get).unwrap();
        assert_eq!(get_body.target_device_id, "bob-laptop");
        assert_eq!(get_body.require_opk, Some(true));

        let rotated = ensure_local_prekey_publication(
            &store,
            V2LocalPrekeyIdentity {
                did: did.as_str(),
                device_id: "alice-phone",
                signing_key_id: "did:example:alice#phone-sign-rotated",
                e2ee_key_id: "did:example:alice#phone-e2ee-rotated",
                signing_private: &signing,
            },
            now,
        )
        .unwrap();
        assert_ne!(rotated.bundle.bundle_id, publication.bundle.bundle_id);
        assert_eq!(
            rotated.bundle.static_key_agreement_id,
            "did:example:alice#phone-e2ee-rotated"
        );
        assert_eq!(
            rotated.bundle.proof["verificationMethod"],
            "did:example:alice#phone-sign-rotated"
        );
    }
}
