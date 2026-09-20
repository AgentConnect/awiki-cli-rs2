use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde_json::{json, Value};

use super::*;

#[test]
fn web_management_proof_binds_audience_complete_document_and_fresh_nonce() {
    let did = "did:web:identity.example:awiki:web:123e4567e89b42d3a456426614174000";
    let kid = format!("{did}#device-admin-sign");
    let private = anp::PrivateKeyMaterial::Ed25519(ed25519_dalek::SigningKey::from_bytes(&[9; 32]));
    let document = json!({"id":did,"service":[{"id":"#custom","type":"Custom","serviceEndpoint":{"nested_proof":"business-value","nested_token":"business-value"}}]});
    let checkpoint = crate::internal::identity_device_state::IdentityInternalCheckpoint {
        document_version: 3,
        registry_version: 2,
        document_hash: format!("sha256:{}", "A".repeat(43)),
    };
    let now = time::OffsetDateTime::from_unix_timestamp(1_784_515_200).unwrap();
    let signer = |_: &str, input: &[u8]| {
        private
            .sign_message(input)
            .map_err(|_| crate::ImError::PermissionDenied)
    };
    let update = device_document_update::prepare_update(
        "operation".into(),
        checkpoint.clone(),
        document.clone(),
        "device-admin".into(),
        &kid,
        Some("configured-audience"),
        &signer,
        now,
    )
    .unwrap();
    let again = device_document_update::prepare_update(
        "operation".into(),
        checkpoint.clone(),
        document.clone(),
        "device-admin".into(),
        &kid,
        Some("configured-audience"),
        &signer,
        now,
    )
    .unwrap();
    assert_ne!(
        update.authorizing_device_proof.nonce,
        again.authorizing_device_proof.nonce
    );
    assert!(device_document_update::prepare_update(
        "operation".into(),
        checkpoint.clone(),
        document.clone(),
        "device-admin".into(),
        &kid,
        None,
        &signer,
        now
    )
    .is_err());
    let revoke = device_revoke::prepare_revoke(
        "operation".into(),
        "device-target".into(),
        checkpoint,
        document.clone(),
        "device-admin".into(),
        &kid,
        Some("configured-audience"),
        &signer,
        now,
    )
    .unwrap();
    for (method, purpose, mut params, proof) in [
        (
            device_document_update::DEVICE_DOCUMENT_UPDATE_METHOD,
            device_document_update::DEVICE_DOCUMENT_UPDATE_PURPOSE,
            device_document_update::build_update_call(&update)
                .unwrap()
                .params,
            update.authorizing_device_proof,
        ),
        (
            device_revoke::DEVICE_REVOKE_METHOD,
            device_revoke::DEVICE_REVOKE_PURPOSE,
            device_revoke::build_revoke_call(&revoke).unwrap().params,
            revoke.authorizing_device_proof,
        ),
    ] {
        params
            .as_object_mut()
            .unwrap()
            .remove("authorizing_device_proof");
        assert_eq!(params["new_document"], document);
        let mut signing = json!({"type": proof.proof_type, "purpose":purpose,"method":method,"audience":"configured-audience","key_id":proof.key_id,"created_at":proof.created_at,"expires_at":proof.expires_at,"nonce":proof.nonce,"params":params});
        let signature = URL_SAFE_NO_PAD.decode(&proof.signature).unwrap();
        let verify = |value: &Value| {
            private.public_key().verify_message(
                &serde_json_canonicalizer::to_vec(value).unwrap(),
                &signature,
            )
        };
        verify(&signing).unwrap();
        signing["audience"] = json!("another-deployment");
        assert!(verify(&signing).is_err());
        signing["audience"] = json!("configured-audience");
        signing["params"]["new_document"]["service"][0]["serviceEndpoint"]["nested_proof"] =
            json!("changed");
        assert!(verify(&signing).is_err());
    }
}
