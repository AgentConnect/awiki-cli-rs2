//! Closed wire contract for same-deployment Manifest Handle Recovery v1.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};

use crate::internal::platform_secret::SecretBytes;

#[cfg(test)]
mod tests;

pub(crate) const HANDLE_RECOVERY_PURPOSE: &str = "awiki.identity.handle-recovery.v1";
pub(crate) const HANDLE_RECOVERY_COMMIT_PURPOSE: &str = "awiki.identity.handle-recovery.commit.v1";
pub(crate) const HANDLE_RECOVERY_COMMIT_METHOD: &str = "handle_recovery_commit";
const DEVICE_SIGNATURE_TYPE: &str = "awiki-device-signature-v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CanonicalHandle {
    pub(crate) local_part: String,
    pub(crate) domain: String,
    pub(crate) full: String,
}

pub(crate) struct RecoveryGrantExchangeResult {
    pub(crate) recovery_grant: SecretBytes,
    pub(crate) expires_at: String,
}

impl std::fmt::Debug for RecoveryGrantExchangeResult {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RecoveryGrantExchangeResult")
            .field("recovery_grant", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IdentityTransition {
    pub(crate) previous_did: String,
    pub(crate) current_did: String,
    pub(crate) binding_generation: String,
}

pub(crate) struct AccountVerificationResult {
    pub(crate) account_verification_token: SecretBytes,
    pub(crate) expires_at: String,
    pub(crate) account_user_id: Option<String>,
    pub(crate) handle: Option<String>,
    pub(crate) did: Option<String>,
    pub(crate) identity_transition: Option<IdentityTransition>,
}

impl std::fmt::Debug for AccountVerificationResult {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AccountVerificationResult")
            .field("account_verification_token", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .field("account_user_id", &self.account_user_id)
            .field("handle", &self.handle)
            .field("did", &self.did)
            .field("identity_transition", &self.identity_transition)
            .finish()
    }
}

pub(crate) struct CommitProofInput<'a> {
    pub(crate) operation_id: &'a str,
    pub(crate) handle: &'a str,
    pub(crate) recovery_grant: SecretBytes,
    pub(crate) expected_binding_generation: &'a str,
    pub(crate) new_did_document: Value,
    pub(crate) bootstrap_device_id: &'a str,
    pub(crate) bootstrap_signing_key_id: &'a str,
    pub(crate) bootstrap_signing_private_key: &'a anp::PrivateKeyMaterial,
    pub(crate) created_at: &'a str,
    pub(crate) expires_at: &'a str,
    pub(crate) nonce: &'a [u8],
}

impl std::fmt::Debug for CommitProofInput<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CommitProofInput")
            .field("operation_id", &self.operation_id)
            .field("handle", &self.handle)
            .field("recovery_grant", &"<redacted>")
            .field(
                "expected_binding_generation",
                &self.expected_binding_generation,
            )
            .field("new_did_document", &self.new_did_document)
            .field("bootstrap_device_id", &self.bootstrap_device_id)
            .field("bootstrap_signing_key_id", &self.bootstrap_signing_key_id)
            .field("bootstrap_signing_private_key", &"<redacted>")
            .field("created_at", &self.created_at)
            .field("expires_at", &self.expires_at)
            .field("nonce", &"<redacted>")
            .finish()
    }
}

pub(crate) struct PreparedCommit {
    pub(crate) call: super::RpcCall,
    pub(crate) signed_params: Value,
    pub(crate) request_hash: String,
}

impl std::fmt::Debug for PreparedCommit {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedCommit")
            .field("endpoint", &self.call.endpoint)
            .field("method", &self.call.method)
            .field("request_hash", &self.request_hash)
            .finish()
    }
}

pub(crate) fn canonical_handle(value: &str) -> crate::ImResult<CanonicalHandle> {
    if value.is_empty() || value != value.trim() || value.ends_with('.') {
        return Err(invalid("handle", "full Handle must already be canonical"));
    }
    let Some((local_part, domain)) = value.split_once('.') else {
        return Err(invalid(
            "handle",
            "full Handle must include a provider domain",
        ));
    };
    if local_part.is_empty()
        || domain.is_empty()
        || value.bytes().any(|byte| byte.is_ascii_uppercase())
        || !anp::wns::validate_local_part(local_part)
        || domain.split('.').any(|label| {
            label.is_empty()
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
    {
        return Err(invalid("handle", "full Handle is not canonical"));
    }
    Ok(CanonicalHandle {
        local_part: local_part.to_owned(),
        domain: domain.to_owned(),
        full: value.to_owned(),
    })
}

pub(crate) fn validate_operation_id(value: &str) -> crate::ImResult<String> {
    let scalar_count = value.chars().count();
    if scalar_count == 0
        || scalar_count > 128
        || value.chars().any(|character| {
            character.is_whitespace() || character == '\u{7f}' || (character as u32) < 0x20
        })
    {
        return Err(invalid(
            "operation_id",
            "operation id is outside the closed Handle Recovery profile",
        ));
    }
    Ok(value.to_owned())
}

pub(crate) fn recovery_otp_target(handle: &str, operation_id: &str) -> crate::ImResult<String> {
    let handle = canonical_handle(handle)?;
    let operation_id = validate_operation_id(operation_id)?;
    let target = json!({
        "domain": handle.domain,
        "full_handle": handle.full,
        "handle": handle.local_part,
        "operation_id": operation_id,
        "purpose": HANDLE_RECOVERY_PURPOSE,
    });
    let canonical = serde_json_canonicalizer::to_vec(&target).map_err(serialization)?;
    Ok(format!("sha256:{:x}", Sha256::digest(canonical)))
}

pub(crate) fn build_send_otp_call(
    phone: &str,
    handle: &str,
    operation_id: &str,
) -> crate::ImResult<super::RpcCall> {
    let phone = super::normalize_phone(phone)?;
    let handle = canonical_handle(handle)?;
    let operation_id = validate_operation_id(operation_id)?;
    let _ = recovery_otp_target(&handle.full, &operation_id)?;
    Ok(super::rpc_call(
        super::HANDLE_RPC_ENDPOINT,
        "send_otp",
        super::TransportProfile::RpcDefault,
        json!({
            "phone": phone,
            "purpose": HANDLE_RECOVERY_PURPOSE,
            "handle": handle.local_part,
            "domain": handle.domain,
            "full_handle": handle.full,
            "operation_id": operation_id,
        }),
    ))
}

pub(crate) fn build_grant_exchange_call(
    phone: &str,
    code: &str,
    handle: &str,
    operation_id: &str,
) -> crate::ImResult<super::RestCall> {
    let phone = super::normalize_phone(phone)?;
    let code = super::sanitize_otp(code);
    if code.is_empty() {
        return Err(invalid("code", "OTP code is required"));
    }
    let handle = canonical_handle(handle)?;
    let operation_id = validate_operation_id(operation_id)?;
    let _ = recovery_otp_target(&handle.full, &operation_id)?;
    Ok(super::rest_call(
        super::HANDLE_RECOVERY_EXCHANGE_ENDPOINT,
        "POST",
        json!({
            "phone": phone,
            "code": code,
            "handle": handle.full,
            "operation_id": operation_id,
        }),
        Default::default(),
        false,
    ))
}

pub(crate) fn parse_grant_exchange_result(
    value: Value,
) -> crate::ImResult<RecoveryGrantExchangeResult> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Raw {
        recovery_grant: String,
        purpose: String,
        expires_at: String,
    }
    let raw: Raw = closed(value, "Handle Recovery grant exchange")?;
    if raw.purpose != HANDLE_RECOVERY_PURPOSE {
        return Err(invalid("purpose", "unexpected Recovery grant purpose"));
    }
    validate_timestamp("expires_at", &raw.expires_at)?;
    Ok(RecoveryGrantExchangeResult {
        recovery_grant: required_secret(raw.recovery_grant, "recovery_grant")?,
        expires_at: raw.expires_at,
    })
}

pub(crate) fn parse_account_verification_result(
    value: Value,
) -> crate::ImResult<AccountVerificationResult> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Raw {
        account_verification_token: String,
        purpose: String,
        expires_at: String,
        account_user_id: Option<String>,
        handle: Option<String>,
        did: Option<String>,
        identity_transition: Option<RawTransition>,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct RawTransition {
        kind: String,
        previous_did: String,
        current_did: String,
        binding_generation: String,
    }
    let raw: Raw = closed(value, "account verification exchange")?;
    if raw.purpose != "awiki.device.join.v1" {
        return Err(invalid(
            "purpose",
            "unexpected account verification purpose",
        ));
    }
    validate_timestamp("expires_at", &raw.expires_at)?;
    let projection_count = [
        raw.account_user_id.is_some(),
        raw.handle.is_some(),
        raw.did.is_some(),
        raw.identity_transition.is_some(),
    ]
    .into_iter()
    .filter(|present| *present)
    .count();
    if projection_count != 0 && projection_count != 4 {
        return Err(invalid(
            "identity_transition",
            "identity transition projection must be all-or-none",
        ));
    }
    let transition = raw
        .identity_transition
        .map(|transition| {
            if transition.kind != "handle_recovery"
                || !canonical_generation(&transition.binding_generation)
            {
                return Err(invalid(
                    "identity_transition",
                    "unsupported identity transition",
                ));
            }
            let previous = crate::ids::Did::parse(transition.previous_did.clone())?;
            let current = crate::ids::Did::parse(transition.current_did.clone())?;
            if current.as_str() != raw.did.as_deref().unwrap_or_default() {
                return Err(invalid(
                    "did",
                    "projected DID must match identity transition current DID",
                ));
            }
            if canonical_handle(raw.handle.as_deref().unwrap_or_default()).is_err() {
                return Err(invalid("handle", "projected Handle is not canonical"));
            }
            Ok(IdentityTransition {
                previous_did: previous.as_str().to_owned(),
                current_did: current.as_str().to_owned(),
                binding_generation: transition.binding_generation,
            })
        })
        .transpose()?;
    Ok(AccountVerificationResult {
        account_verification_token: required_secret(
            raw.account_verification_token,
            "account_verification_token",
        )?,
        expires_at: raw.expires_at,
        account_user_id: raw.account_user_id,
        handle: raw.handle,
        did: raw.did,
        identity_transition: transition,
    })
}

pub(crate) fn prepare_commit(input: CommitProofInput<'_>) -> crate::ImResult<PreparedCommit> {
    let operation_id = validate_operation_id(input.operation_id)?;
    let handle = canonical_handle(input.handle)?;
    if !canonical_generation(input.expected_binding_generation) {
        return Err(invalid(
            "expected_binding_generation",
            "expected binding generation must be canonical positive decimal",
        ));
    }
    validate_timestamp("created_at", input.created_at)?;
    validate_timestamp("expires_at", input.expires_at)?;
    if input.nonce.is_empty() {
        return Err(invalid("nonce", "proof nonce is required"));
    }
    let mut projected_document = input.new_did_document.clone();
    projected_document
        .as_object_mut()
        .ok_or_else(|| invalid("new_did_document", "DID document must be an object"))?
        .remove("proof");
    let signed_params = json!({
        "operation_id": operation_id,
        "handle": handle.full,
        "expected_binding_generation": input.expected_binding_generation,
        "new_did_document": projected_document,
        "bootstrap_device_id": required(input.bootstrap_device_id, "bootstrap_device_id")?,
    });
    let canonical_params =
        serde_json_canonicalizer::to_vec(&signed_params).map_err(serialization)?;
    let request_hash = format!(
        "sha256:{}",
        URL_SAFE_NO_PAD.encode(Sha256::digest(&canonical_params))
    );
    let nonce = URL_SAFE_NO_PAD.encode(input.nonce);
    let signing_object = json!({
        "type": DEVICE_SIGNATURE_TYPE,
        "purpose": HANDLE_RECOVERY_COMMIT_PURPOSE,
        "method": HANDLE_RECOVERY_COMMIT_METHOD,
        "key_id": required(input.bootstrap_signing_key_id, "bootstrap_signing_key_id")?,
        "created_at": input.created_at,
        "expires_at": input.expires_at,
        "nonce": nonce,
        "params": signed_params,
    });
    let signing_bytes = serde_json_canonicalizer::to_vec(&signing_object).map_err(serialization)?;
    let signature = input
        .bootstrap_signing_private_key
        .sign_message(&signing_bytes)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    if signature.len() != 64 {
        return Err(crate::ImError::PermissionDenied);
    }
    let proof = json!({
        "type": DEVICE_SIGNATURE_TYPE,
        "key_id": input.bootstrap_signing_key_id,
        "created_at": input.created_at,
        "expires_at": input.expires_at,
        "nonce": nonce,
        "signature": URL_SAFE_NO_PAD.encode(signature),
    });
    let recovery_grant = String::from_utf8(input.recovery_grant.expose_secret().to_vec())
        .map_err(|_| invalid("recovery_grant", "Recovery grant must be UTF-8"))?;
    let params = json!({
        "operation_id": operation_id,
        "handle": handle.full,
        "recovery_grant": recovery_grant,
        "expected_binding_generation": input.expected_binding_generation,
        "new_did_document": input.new_did_document,
        "bootstrap_device_id": input.bootstrap_device_id,
        "bootstrap_device_proof": proof,
    });
    Ok(PreparedCommit {
        call: super::rpc_call(
            super::DID_AUTH_RPC_ENDPOINT,
            HANDLE_RECOVERY_COMMIT_METHOD,
            super::TransportProfile::RpcDefault,
            params,
        ),
        signed_params,
        request_hash,
    })
}

fn canonical_generation(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.as_bytes()[0] != b'0'
}

fn validate_timestamp(field: &str, value: &str) -> crate::ImResult<()> {
    if !value.ends_with('Z')
        || time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
            .is_err()
    {
        return Err(invalid(field, "timestamp must be canonical RFC 3339 UTC"));
    }
    Ok(())
}

fn closed<T: for<'de> Deserialize<'de>>(value: Value, context: &str) -> crate::ImResult<T> {
    serde_json::from_value(value).map_err(|_| crate::ImError::Serialization {
        detail: format!("invalid closed {context} response"),
    })
}

fn required(value: &str, field: &str) -> crate::ImResult<String> {
    if value.is_empty() || value != value.trim() {
        return Err(invalid(field, format!("{field} is required")));
    }
    Ok(value.to_owned())
}

fn required_secret(value: String, field: &str) -> crate::ImResult<SecretBytes> {
    required(&value, field).map(|value| SecretBytes::from_vec(value.into_bytes()))
}

fn serialization(error: serde_json::Error) -> crate::ImError {
    crate::ImError::Serialization {
        detail: error.to_string(),
    }
}

fn invalid(field: &str, message: impl Into<String>) -> crate::ImError {
    crate::ImError::invalid_input(Some(field.to_owned()), message.into())
}
