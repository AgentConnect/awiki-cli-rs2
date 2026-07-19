//! Product-owned P5 v2 PreKey lifecycle and strict wire adapters.

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

const DEFAULT_OPK_BATCH_SIZE: usize = 16;
const SIGNED_PREKEY_LIFETIME_DAYS: i64 = 30;

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
        let available = store.load_available_opk_publics()?;
        let expires_at = DateTime::parse_from_rfc3339(&local.bundle.signed_prekey.expires_at)
            .map_err(|_| crate::ImError::PermissionDenied)?
            .with_timezone(&Utc);
        if expires_at > now && !available.is_empty() {
            return Ok(V2LocalPrekeyPublication {
                bundle: local.bundle,
                one_time_prekeys: available,
            });
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
        let store =
            SqliteV2DirectStateStore::new_with_secret_vault(&connection, vault, scope).unwrap();
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
            store.load_available_opk_publics().unwrap(),
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

        let publish =
            publish_request(&publication, "did:example:message-service", "publish-1").unwrap();
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
    }
}
