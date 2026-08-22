mod anp_identity;
mod did_auth;
mod file;
mod hosted;
#[cfg(feature = "provider-traits")]
mod provider;
pub(crate) mod vault;

pub(crate) use self::anp_identity::{AnpIdentitySigner, PendingAnpEnrollmentSigner};
pub(crate) use self::did_auth::ProviderBackedDidAuth;
pub(crate) use self::file::FileBackedIdentitySigner;
pub(crate) use self::hosted::{HostBackedDeviceIdentitySigner, HostedIdentitySigner};
#[cfg(feature = "provider-traits")]
pub(crate) use self::provider::ProviderIdentitySigner;
pub(crate) use self::vault::LegacyVaultKeyMaterialRefs;

pub(crate) trait IdentitySigner: Send + Sync {
    fn async_session(
        &self,
    ) -> Option<std::sync::Arc<dyn crate::internal::identity_provider::IdentitySession>> {
        None
    }

    fn did_document(&self) -> crate::ImResult<serde_json::Value>;

    fn optional_did_document(&self) -> crate::ImResult<Option<serde_json::Value>>;

    fn request_signing_key_id(&self) -> crate::ImResult<String>;

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

    fn sign(&self, kid: &str, message: &[u8]) -> crate::ImResult<Vec<u8>>;

    fn sign_device_assertion(&self, kid: &str, message: &[u8]) -> crate::ImResult<Vec<u8>> {
        self.sign(kid, message)
    }

    fn sign_root(&self, kid: &str, message: &[u8]) -> crate::ImResult<Vec<u8>>;

    fn ecdh(&self, kid: &str, peer_public: &[u8]) -> crate::ImResult<zeroize::Zeroizing<[u8; 32]>>;

    fn legacy_root_private_pem(&self) -> crate::ImResult<zeroize::Zeroizing<String>> {
        Err(crate::ImError::PermissionDenied)
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
        let kid = self.root_control_key_id()?;
        self.sign_root(&kid, b"awiki:root-control:availability:v1")
            .map(|_| ())
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
        let public_key = self.public_key(verification_method)?;
        let prepared =
            anp::proof::prepare_w3c_proof(document, &public_key, verification_method, options)
                .map_err(map_crypto_error)?;
        let signature = self.sign_root(verification_method, prepared.signing_input())?;
        anp::proof::complete_w3c_proof(prepared, &signature).map_err(map_crypto_error)
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

    fn reload_custody(&self) -> crate::ImResult<()> {
        Err(crate::ImError::PermissionDenied)
    }

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

pub(crate) fn sign_private_pem(
    private_key_pem: &str,
    message: &[u8],
    operation: &str,
) -> crate::ImResult<Vec<u8>> {
    private_key_from_pem(private_key_pem, operation)?
        .sign_message(message)
        .map_err(|_| crate::ImError::PermissionDenied)
}

pub(crate) fn ecdh_private_pem(
    private_key_pem: &str,
    peer_public: &[u8],
    operation: &str,
) -> crate::ImResult<zeroize::Zeroizing<[u8; 32]>> {
    let private = private_key_from_pem(private_key_pem, operation)?;
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

fn map_crypto_error(error: impl std::fmt::Display) -> crate::ImError {
    crate::ImError::LocalStateUnavailable {
        detail: format!("identity signing operation failed: {error}"),
    }
}

#[cfg(test)]
impl IdentitySigner for anp::PrivateKeyMaterial {
    fn did_document(&self) -> crate::ImResult<serde_json::Value> {
        Err(crate::ImError::PermissionDenied)
    }

    fn optional_did_document(&self) -> crate::ImResult<Option<serde_json::Value>> {
        Ok(None)
    }

    fn request_signing_key_id(&self) -> crate::ImResult<String> {
        Ok("did:example:test#signing".to_owned())
    }

    fn public_key(&self, _kid: &str) -> crate::ImResult<anp::PublicKeyMaterial> {
        Ok(anp::PrivateKeyMaterial::public_key(self))
    }

    fn sign(&self, _kid: &str, message: &[u8]) -> crate::ImResult<Vec<u8>> {
        self.sign_message(message)
            .map_err(|_| crate::ImError::PermissionDenied)
    }

    fn sign_root(&self, _kid: &str, message: &[u8]) -> crate::ImResult<Vec<u8>> {
        self.sign_message(message)
            .map_err(|_| crate::ImError::PermissionDenied)
    }

    fn ecdh(
        &self,
        _kid: &str,
        peer_public: &[u8],
    ) -> crate::ImResult<zeroize::Zeroizing<[u8; 32]>> {
        let anp::PrivateKeyMaterial::X25519(private) = self else {
            return Err(crate::ImError::PermissionDenied);
        };
        let peer: [u8; 32] = peer_public
            .try_into()
            .map_err(|_| crate::ImError::PermissionDenied)?;
        Ok(zeroize::Zeroizing::new(
            private
                .diffie_hellman(&x25519_dalek::PublicKey::from(peer))
                .to_bytes(),
        ))
    }

    fn auth_state(&self) -> crate::ImResult<crate::internal::auth::state::AuthStateSnapshot> {
        Ok(crate::internal::auth::state::AuthStateSnapshot::default())
    }

    fn valid_auth_token(&self) -> crate::ImResult<Option<String>> {
        Ok(None)
    }

    fn persist_auth_token(&self, _token: &str) -> crate::ImResult<()> {
        Err(crate::ImError::PermissionDenied)
    }
}
