mod did_auth;
mod file;
mod hosted;
pub(crate) mod vault;

pub(crate) use self::did_auth::ProviderBackedDidAuth;
pub(crate) use self::file::FileBackedIdentitySigner;
pub(crate) use self::hosted::{HostBackedDeviceIdentitySigner, HostedIdentitySigner};
pub(crate) use self::vault::LegacyVaultKeyMaterialRefs;

#[derive(Clone)]
pub(crate) struct DeviceRequestSigningMaterial {
    pub(crate) key_id: String,
    pub(crate) private_key_pem: String,
}

impl std::fmt::Debug for DeviceRequestSigningMaterial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceRequestSigningMaterial")
            .field("key_id", &self.key_id)
            .field("private_key_pem", &"<redacted-private-key>")
            .finish()
    }
}

/// Explicit compatibility adapter for identities where legacy `key-1` has both
/// request-signing and DID Document root-control semantics.
///
/// New multi-device identities must not use this adapter: their device signing
/// and root-control keys are separate vault records.
pub(crate) struct LegacyKey1RoleAdapter {
    key1_private_pem: String,
}

impl LegacyKey1RoleAdapter {
    pub(crate) fn new(key1_private_pem: String) -> Self {
        Self { key1_private_pem }
    }

    pub(crate) fn device_request_signing_private_pem(&self) -> String {
        self.key1_private_pem.clone()
    }

    pub(crate) fn did_document_root_private_pem(&self) -> String {
        self.key1_private_pem.clone()
    }
}

impl std::fmt::Debug for LegacyKey1RoleAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LegacyKey1RoleAdapter")
            .field("key1_private_pem", &"<redacted-private-key>")
            .finish()
    }
}

pub(crate) trait IdentitySigner: Send + Sync {
    fn did_document(&self) -> crate::ImResult<serde_json::Value>;

    fn optional_did_document(&self) -> crate::ImResult<Option<serde_json::Value>>;

    /// Returns the device key used for login, HTTP auth, and daily requests.
    fn device_request_signing_private_pem(&self) -> crate::ImResult<String>;

    /// Returns the exact verification method and matching private key used for
    /// DID-WBA requests. Callers must not infer `keyid` from relationship
    /// ordering independently of the selected private key.
    fn device_request_signing_material(&self) -> crate::ImResult<DeviceRequestSigningMaterial>;

    /// Returns the DID root key used only to create or update a DID Document.
    fn did_document_root_private_pem(&self) -> crate::ImResult<String>;

    fn e2ee_agreement_private_pem(&self) -> crate::ImResult<String>;

    fn request_signing_key_id(&self) -> crate::ImResult<String> {
        self.device_request_signing_material()
            .map(|material| material.key_id)
    }

    fn agreement_key_id(&self) -> crate::ImResult<String> {
        relationship_key_id(&self.did_document()?, "keyAgreement")
    }

    fn root_control_key_id(&self) -> crate::ImResult<String> {
        let document = self.did_document()?;
        document
            .get("proof")
            .and_then(serde_json::Value::as_object)
            .and_then(|proof| proof.get("verificationMethod"))
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .ok_or(crate::ImError::PermissionDenied)
    }

    fn public_key(&self, kid: &str) -> crate::ImResult<anp::PublicKeyMaterial> {
        let document = self.did_document()?;
        let method = anp::authentication::find_verification_method(&document, kid)
            .ok_or(crate::ImError::PermissionDenied)?;
        anp::authentication::extract_public_key(&method)
            .map_err(|_| crate::ImError::PermissionDenied)
    }

    fn sign(&self, kid: &str, message: &[u8]) -> crate::ImResult<Vec<u8>> {
        let material = self.device_request_signing_material()?;
        if material.key_id != kid {
            return Err(crate::ImError::PermissionDenied);
        }
        let private = private_key_from_pem(&material.private_key_pem, "request signing")?;
        private
            .sign_message(message)
            .map_err(|_| crate::ImError::PermissionDenied)
    }

    fn ecdh(&self, kid: &str, peer_public: &[u8]) -> crate::ImResult<zeroize::Zeroizing<[u8; 32]>> {
        if self.agreement_key_id()? != kid {
            return Err(crate::ImError::PermissionDenied);
        }
        let private = private_key_from_pem(&self.e2ee_agreement_private_pem()?, "E2EE agreement")?;
        let anp::PrivateKeyMaterial::X25519(private) = private else {
            return Err(crate::ImError::PermissionDenied);
        };
        let peer: [u8; 32] = peer_public
            .try_into()
            .map_err(|_| crate::ImError::PermissionDenied)?;
        let shared = zeroize::Zeroizing::new(
            private
                .diffie_hellman(&x25519_dalek::PublicKey::from(peer))
                .to_bytes(),
        );
        if shared.iter().all(|byte| *byte == 0) {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(shared)
    }

    fn ensure_request_signing_available(&self) -> crate::ImResult<()> {
        let kid = self.request_signing_key_id()?;
        self.sign(&kid, b"awiki:identity-signer:availability:v1")
            .map(|_| ())
    }

    fn ensure_agreement_available(&self) -> crate::ImResult<()> {
        let kid = self.agreement_key_id()?;
        self.ecdh(&kid, &x25519_dalek::X25519_BASEPOINT_BYTES)
            .map(|_| ())
    }

    fn ensure_root_control_available(&self) -> crate::ImResult<()> {
        let private = private_key_from_pem(
            &self.did_document_root_private_pem()?,
            "DID document root control",
        )?;
        if matches!(
            private,
            anp::PrivateKeyMaterial::Ed25519(_) | anp::PrivateKeyMaterial::Secp256k1(_)
        ) {
            Ok(())
        } else {
            Err(crate::ImError::PermissionDenied)
        }
    }

    fn sign_object_proof(
        &self,
        kid: &str,
        document: &serde_json::Value,
        issuer_did: &str,
        created: Option<String>,
    ) -> crate::ImResult<serde_json::Value> {
        let public_key = self.public_key(kid)?;
        let prepared =
            anp::proof::prepare_object_proof(document, &public_key, kid, issuer_did, created)
                .map_err(map_crypto_error)?;
        let signature = self.sign(kid, prepared.signing_input())?;
        anp::proof::complete_object_proof(prepared, &signature).map_err(map_crypto_error)
    }

    fn sign_document_proof(
        &self,
        document: &serde_json::Value,
        verification_method: &str,
        options: anp::proof::ProofGenerationOptions,
    ) -> crate::ImResult<serde_json::Value> {
        if self.root_control_key_id()? != verification_method {
            return Err(crate::ImError::PermissionDenied);
        }
        let private = private_key_from_pem(
            &self.did_document_root_private_pem()?,
            "DID document root control",
        )?;
        anp::proof::generate_w3c_proof(document, &private, verification_method, options)
            .map_err(map_crypto_error)
    }

    fn sign_origin_proof(
        &self,
        method: &str,
        meta: &serde_json::Value,
        body: &serde_json::Value,
        kid: &str,
        options: anp::proof::Rfc9421OriginProofGenerationOptions,
    ) -> crate::ImResult<anp::proof::Rfc9421OriginProof> {
        let public_key = self.public_key(kid)?;
        let prepared =
            anp::proof::prepare_rfc9421_origin_proof(method, meta, body, &public_key, kid, options)
                .map_err(map_crypto_error)?;
        let signature = self.sign(kid, prepared.signing_input())?;
        anp::proof::complete_rfc9421_origin_proof(prepared, &signature).map_err(map_crypto_error)
    }

    fn legacy_did_wba_header(
        &self,
        kid: &str,
        service_domain: &str,
        version: &str,
    ) -> crate::ImResult<String> {
        let document = self.did_document()?;
        let prepared = anp::authentication::prepare_legacy_did_wba_auth_header(
            &document,
            service_domain,
            version,
            kid,
        )
        .map_err(map_crypto_error)?;
        let signature = self.sign(kid, prepared.signing_input())?;
        anp::authentication::complete_legacy_did_wba_auth_header(prepared, &signature)
            .map_err(map_crypto_error)
    }

    fn http_signature_headers(
        &self,
        kid: &str,
        request_url: &str,
        request_method: &str,
        headers: Option<&std::collections::BTreeMap<String, String>>,
        body: Option<&[u8]>,
        mut options: anp::authentication::HttpSignatureOptions,
    ) -> crate::ImResult<std::collections::BTreeMap<String, String>> {
        options.keyid = Some(kid.to_owned());
        let document = self.did_document()?;
        let prepared = anp::authentication::prepare_http_signature_headers(
            &document,
            request_url,
            request_method,
            headers,
            body,
            options,
        )
        .map_err(map_crypto_error)?;
        let signature = self.sign(kid, prepared.signing_input())?;
        anp::authentication::complete_http_signature_headers(prepared, &signature)
            .map_err(map_crypto_error)
    }

    fn auth_state(&self) -> crate::ImResult<crate::internal::auth::state::AuthStateSnapshot>;

    fn valid_auth_token(&self) -> crate::ImResult<Option<String>>;

    fn persist_auth_token(&self, token: &str) -> crate::ImResult<()>;

    /// Advances a live Vault-backed provider to the auth SecretRef committed
    /// by the identity index. Non-Vault providers cannot participate in this
    /// local vNext convergence operation.
    fn advance_vault_auth_ref(
        &self,
        _committed: &crate::internal::secret_vault::record::SecretRef,
    ) -> crate::ImResult<()> {
        Err(crate::ImError::PermissionDenied)
    }

    /// Advances a live vNext Vault-backed provider to the DID root SecretRef
    /// committed by the identity index after root import.
    fn advance_vault_root_ref(
        &self,
        _committed: &crate::internal::secret_vault::record::SecretRef,
    ) -> crate::ImResult<()> {
        Err(crate::ImError::PermissionDenied)
    }
}

fn relationship_key_id(
    document: &serde_json::Value,
    relationship: &str,
) -> crate::ImResult<String> {
    document
        .get(relationship)
        .and_then(serde_json::Value::as_array)
        .and_then(|entries| entries.first())
        .and_then(|entry| {
            entry
                .as_str()
                .or_else(|| entry.get("id").and_then(serde_json::Value::as_str))
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or(crate::ImError::PermissionDenied)
}

fn private_key_from_pem(
    private_key_pem: &str,
    operation: &str,
) -> crate::ImResult<anp::PrivateKeyMaterial> {
    anp::PrivateKeyMaterial::from_pem(private_key_pem).map_err(|_| {
        crate::ImError::LocalStateUnavailable {
            detail: format!("{operation} key material is invalid"),
        }
    })
}

fn map_crypto_error(error: impl std::fmt::Display) -> crate::ImError {
    crate::ImError::LocalStateUnavailable {
        detail: format!("identity signing operation failed: {error}"),
    }
}
