use anp::direct_e2ee::{OneTimePrekey, PrekeyBundle, SignedPrekey};
use anp::{PrivateKeyMaterial, PublicKeyMaterial};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};
use time::{format_description::well_known::Rfc3339, Duration, OffsetDateTime};

const SIGNED_PREKEY_LIFETIME: Duration = Duration::days(7);
const SIGNED_PREKEY_ROTATION_LEAD: Duration = Duration::days(1);

pub(crate) fn create_signed_prekey(
    private_key: &PrivateKeyMaterial,
) -> crate::ImResult<SignedPrekey> {
    let created = OffsetDateTime::now_utc();
    let expires = created + SIGNED_PREKEY_LIFETIME;
    let public_key_b64u = x25519_public_key_b64u(private_key)?;
    let key_digest = short_digest(public_key_b64u.as_bytes());
    Ok(SignedPrekey {
        key_id: format!("spk-{}-{key_digest}", created.unix_timestamp()),
        public_key_b64u,
        expires_at: format_timestamp(expires)?,
    })
}

pub(crate) fn signed_prekey_needs_rotation(prekey: &SignedPrekey) -> bool {
    let Ok(expires) = OffsetDateTime::parse(prekey.expires_at.trim(), &Rfc3339) else {
        return true;
    };
    let remaining = expires - OffsetDateTime::now_utc();
    remaining <= SIGNED_PREKEY_ROTATION_LEAD || remaining > SIGNED_PREKEY_LIFETIME
}

pub(crate) fn bundle_id(prekey: &SignedPrekey) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prekey.key_id.as_bytes());
    hasher.update([0]);
    hasher.update(prekey.public_key_b64u.as_bytes());
    format!("bundle-{}", short_hex(hasher.finalize().as_slice()))
}

pub(crate) fn bundle_proof_created(prekey: &SignedPrekey) -> crate::ImResult<String> {
    let expires = OffsetDateTime::parse(prekey.expires_at.trim(), &Rfc3339).map_err(|error| {
        crate::ImError::Serialization {
            detail: format!("parse direct E2EE signed prekey expiry: {error}"),
        }
    })?;
    format_timestamp(expires - SIGNED_PREKEY_LIFETIME)
}

pub(crate) fn publish_operation_id(
    bundle: &PrekeyBundle,
    one_time_prekeys: &[OneTimePrekey],
) -> crate::ImResult<String> {
    let payload = anp::direct_e2ee::prekey_bundle_publish_body(bundle, one_time_prekeys);
    let bytes = serde_json_canonicalizer::to_vec(&payload).map_err(|error| {
        crate::ImError::Serialization {
            detail: format!("canonicalize direct E2EE prekey publish payload: {error}"),
        }
    })?;
    Ok(format!("op-publish-{}", short_digest(&bytes)))
}

pub(crate) fn prekey_bundle_publish_request(
    local_did: &str,
    local_service_did: &str,
    bundle: &PrekeyBundle,
    one_time_prekeys: &[OneTimePrekey],
) -> crate::ImResult<Value> {
    let operation_id = publish_operation_id(bundle, one_time_prekeys)?;
    Ok(json!({
        "method": "direct.e2ee.publish_prekey_bundle",
        "params": {
            "meta": {
                "anp_version": "1.0",
                "profile": "anp.direct.e2ee.v1",
                "security_profile": "transport-protected",
                "sender_did": local_did,
                "target": {
                    "kind": "service",
                    "did": local_service_did,
                },
                "operation_id": operation_id,
            },
            "body": anp::direct_e2ee::prekey_bundle_publish_body(bundle, one_time_prekeys),
        },
    }))
}

fn x25519_public_key_b64u(private_key: &PrivateKeyMaterial) -> crate::ImResult<String> {
    match private_key.public_key() {
        PublicKeyMaterial::X25519(bytes) => Ok(URL_SAFE_NO_PAD.encode(bytes)),
        _ => Err(crate::ImError::Serialization {
            detail: "expected X25519 private key".to_owned(),
        }),
    }
}

fn format_timestamp(value: OffsetDateTime) -> crate::ImResult<String> {
    value
        .format(&Rfc3339)
        .map_err(|error| crate::ImError::Serialization {
            detail: format!("format direct E2EE timestamp: {error}"),
        })
}

fn short_digest(bytes: &[u8]) -> String {
    short_hex(Sha256::digest(bytes).as_slice())
}

fn short_hex(bytes: &[u8]) -> String {
    bytes[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generated_x25519_private_key() -> PrivateKeyMaterial {
        anp::authentication::create_did_wba_document(
            "example.test",
            anp::authentication::DidDocumentOptions::default(),
        )
        .unwrap()
        .load_private_key("key-3")
        .unwrap()
    }

    #[test]
    fn generated_signed_prekey_is_current_and_has_stable_bundle_identity() {
        let prekey = create_signed_prekey(&generated_x25519_private_key()).unwrap();
        assert!(!signed_prekey_needs_rotation(&prekey));
        assert_eq!(bundle_id(&prekey), bundle_id(&prekey));
        assert!(bundle_proof_created(&prekey).is_ok());
    }

    #[test]
    fn publish_operation_id_tracks_the_complete_payload() {
        let prekey = create_signed_prekey(&generated_x25519_private_key()).unwrap();
        let bundle = PrekeyBundle {
            bundle_id: bundle_id(&prekey),
            owner_did: "did:wba:example.test:alice".to_owned(),
            suite: "ANP-DIRECT-E2EE-X3DH-25519-CHACHA20POLY1305-SHA256-V1".to_owned(),
            static_key_agreement_id: "did:wba:example.test:alice#key-3".to_owned(),
            signed_prekey: prekey,
            proof: serde_json::json!({"z": "last", "a": "first"}),
        };
        let first = vec![OneTimePrekey {
            key_id: "opk-1".to_owned(),
            public_key_b64u: "key-1".to_owned(),
        }];
        let second = vec![OneTimePrekey {
            key_id: "opk-2".to_owned(),
            public_key_b64u: "key-2".to_owned(),
        }];

        assert_eq!(
            publish_operation_id(&bundle, &first).unwrap(),
            publish_operation_id(&bundle, &first).unwrap()
        );
        assert_ne!(
            publish_operation_id(&bundle, &first).unwrap(),
            publish_operation_id(&bundle, &second).unwrap()
        );
        let mut semantically_equivalent = bundle.clone();
        semantically_equivalent.proof = serde_json::json!({"a": "first", "z": "last"});
        assert_eq!(
            publish_operation_id(&bundle, &first).unwrap(),
            publish_operation_id(&semantically_equivalent, &first).unwrap()
        );

        let request = prekey_bundle_publish_request(
            "did:wba:example.test:alice",
            "did:wba:example.test:service",
            &bundle,
            &first,
        )
        .unwrap();
        assert_eq!(request["method"], "direct.e2ee.publish_prekey_bundle");
        assert_eq!(
            request["params"]["meta"]["operation_id"],
            publish_operation_id(&bundle, &first).unwrap()
        );
        assert_eq!(
            request["params"]["meta"]["security_profile"],
            "transport-protected"
        );
        assert_eq!(
            request["params"]["body"],
            anp::direct_e2ee::prekey_bundle_publish_body(&bundle, &first)
        );
    }
}
