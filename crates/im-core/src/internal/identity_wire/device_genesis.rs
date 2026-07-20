//! Strict first-party wire contract for vNext identity genesis and device tokens.
//!
//! Registry checkpoints, roles and device-token generations are AWiki-internal
//! state. This module deliberately keeps them out of public DTOs and ANP data.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use time::{format_description::well_known::Rfc3339, Duration, OffsetDateTime};
use zeroize::Zeroize;

use crate::identity::DeviceProof;
use crate::internal::identity_device_state::{
    DeviceAuthorizationProjection, DeviceAuthorizationRole, DeviceAuthorizationStatus,
    IdentityInternalCheckpoint,
};

pub(crate) const DEVICE_GENESIS_METHOD: &str = "device_genesis";
pub(crate) const DEVICE_GENESIS_PURPOSE: &str = "awiki.device.genesis.v1";
pub(crate) const DEVICE_TOKEN_ISSUE_METHOD: &str = "device_token_issue";
const DEVICE_PROOF_TYPE: &str = "awiki-device-signature-v1";
const DEVICE_TOKEN_PROFILE: &str = "awiki-device-token-v1";
const DEVICE_ACCESS_PURPOSE: &str = "awiki.device.access.v1";
const DEVICE_REFRESH_PURPOSE: &str = "awiki.device.refresh.v1";
const PROOF_TTL_SECONDS: i64 = 300;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PreparedDeviceTokenIssue {
    pub(crate) operation_id: String,
    pub(crate) did: String,
    pub(crate) device_id: String,
    pub(crate) signing_key_id: String,
    pub(crate) expected_scopes: Vec<String>,
    pub(crate) authorization: String,
}

impl std::fmt::Debug for PreparedDeviceTokenIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedDeviceTokenIssue")
            .field("operation_id", &self.operation_id)
            .field("did", &self.did)
            .field("device_id", &self.device_id)
            .field("signing_key_id", &self.signing_key_id)
            .field("expected_scopes", &self.expected_scopes)
            .field("authorization", &"<redacted-didwba-authorization>")
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeviceTokenIssueResult {
    pub(crate) access_token: String,
    pub(crate) refresh_token: String,
    pub(crate) expires_at: String,
    pub(crate) user_id: String,
    pub(crate) device_id: String,
    pub(crate) auth_generation: u64,
    pub(crate) scopes: Vec<String>,
}

impl std::fmt::Debug for DeviceTokenIssueResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceTokenIssueResult")
            .field("access_token", &"<redacted-access-token>")
            .field("refresh_token", &"<redacted-refresh-token>")
            .field("expires_at", &self.expires_at)
            .field("user_id", &self.user_id)
            .field("device_id", &self.device_id)
            .field("auth_generation", &self.auth_generation)
            .field("scopes", &self.scopes)
            .finish()
    }
}

impl Drop for DeviceTokenIssueResult {
    fn drop(&mut self) {
        self.access_token.zeroize();
        self.refresh_token.zeroize();
    }
}

pub(crate) struct DeviceTokenIssueWireCall {
    pub(crate) endpoint: &'static str,
    pub(crate) method: &'static str,
    pub(crate) params: Value,
}

impl std::fmt::Debug for DeviceTokenIssueWireCall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceTokenIssueWireCall")
            .field("endpoint", &self.endpoint)
            .field("method", &self.method)
            .field("params", &"<redacted-authorization-bearing-params>")
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AccountVerificationGrant {
    pub(crate) token: String,
    pub(crate) expires_at: String,
}

impl std::fmt::Debug for AccountVerificationGrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountVerificationGrant")
            .field("token", &"<redacted-account-verification-token>")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PreparedDeviceGenesis {
    pub(crate) operation_id: String,
    pub(crate) did_document: Value,
    pub(crate) bootstrap_device_id: String,
    pub(crate) bootstrap_device_proof: DeviceProof,
}

impl std::fmt::Debug for PreparedDeviceGenesis {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedDeviceGenesis")
            .field("operation_id", &self.operation_id)
            .field("did", &self.did_document.get("id"))
            .field("bootstrap_device_id", &self.bootstrap_device_id)
            .field("bootstrap_device_proof", &"<redacted-device-proof>")
            .finish()
    }
}

pub(crate) struct DeviceGenesisWireCall {
    pub(crate) endpoint: &'static str,
    pub(crate) method: &'static str,
    pub(crate) params: Value,
}

impl std::fmt::Debug for DeviceGenesisWireCall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceGenesisWireCall")
            .field("endpoint", &self.endpoint)
            .field("method", &self.method)
            .field("params", &"<redacted-token-bearing-params>")
            .finish()
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeviceGenesisResult {
    pub(crate) did: String,
    pub(crate) user_id: String,
    pub(crate) checkpoint: IdentityInternalCheckpoint,
    pub(crate) device: GenesisDeviceResult,
    pub(crate) access_token: String,
    pub(crate) refresh_token: String,
    pub(crate) token_expires_at: String,
}

impl std::fmt::Debug for DeviceGenesisResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceGenesisResult")
            .field("did", &self.did)
            .field("user_id", &self.user_id)
            .field("checkpoint", &self.checkpoint)
            .field("device", &self.device)
            .field("access_token", &"<redacted-access-token>")
            .field("refresh_token", &"<redacted-refresh-token>")
            .field("token_expires_at", &self.token_expires_at)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GenesisDeviceResult {
    pub(crate) device_id: String,
    pub(crate) signing_key_id: String,
    pub(crate) e2ee_key_id: String,
    pub(crate) status: String,
    pub(crate) role: String,
    pub(crate) management_ready: bool,
    pub(crate) auth_generation: u64,
}

impl DeviceGenesisResult {
    pub(crate) fn device_state(
        &self,
    ) -> crate::internal::identity_device_state::IdentityDeviceState {
        crate::internal::identity_device_state::IdentityDeviceState {
            schema_version:
                crate::internal::identity_device_state::IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
            mode: crate::internal::identity_device_state::IdentityDeviceMode::VNext,
            authorization: Some(DeviceAuthorizationProjection {
                protocol_device_id: crate::ids::ProtocolDeviceId::parse(&self.device.device_id)
                    .expect("validated genesis device id"),
                signing_key_id: self.device.signing_key_id.clone(),
                e2ee_key_id: self.device.e2ee_key_id.clone(),
                status: DeviceAuthorizationStatus::Active,
                role: DeviceAuthorizationRole::Admin,
                management_ready: true,
                auth_generation: self.device.auth_generation,
            }),
            checkpoint: Some(self.checkpoint.clone()),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AccountVerificationResponse {
    account_verification_token: String,
    purpose: String,
    expires_at: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDeviceGenesisResult {
    did: String,
    user_id: String,
    checkpoint: RawCheckpoint,
    device: GenesisDeviceResult,
    access_token: String,
    refresh_token: String,
    token_expires_at: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDeviceTokenIssueResult {
    access_token: String,
    refresh_token: String,
    token_type: String,
    expires_at: String,
    device_id: String,
    auth_generation: u64,
    scopes: Vec<String>,
}

impl Drop for RawDeviceTokenIssueResult {
    fn drop(&mut self) {
        self.access_token.zeroize();
        self.refresh_token.zeroize();
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCheckpoint {
    document_version: u64,
    document_hash: String,
    registry_version: u64,
}

pub(crate) fn build_sms_code_call(phone: &str) -> crate::ImResult<super::RestCall> {
    Ok(super::rest_call(
        super::SMS_CODES_ENDPOINT,
        "POST",
        json!({"phone": super::normalize_phone(phone)?}),
        BTreeMap::new(),
        false,
    ))
}

pub(crate) fn build_account_verification_exchange_call(
    phone: &str,
    code: &str,
    target_handle: &str,
    target_handle_domain: &str,
    idempotency_scope: &str,
) -> crate::ImResult<super::RestCall> {
    let code = super::sanitize_otp(code);
    if code.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("otp".to_owned()),
            "OTP code is required",
        ));
    }
    Ok(super::rest_call(
        super::ACCOUNT_VERIFICATION_EXCHANGE_ENDPOINT,
        "POST",
        json!({
            "provider": "sms",
            "purpose": DEVICE_GENESIS_PURPOSE,
            "phone": super::normalize_phone(phone)?,
            "code": code,
            "target_handle": required_lower_handle(target_handle)?,
            "target_handle_domain": required_lower_domain(target_handle_domain)?,
            "idempotency_scope": required(idempotency_scope, "idempotency_scope")?,
        }),
        BTreeMap::new(),
        false,
    ))
}

pub(crate) fn parse_account_verification_grant(
    raw: Value,
    now: OffsetDateTime,
) -> crate::ImResult<AccountVerificationGrant> {
    let raw: AccountVerificationResponse = strict_from_value(raw, "account verification")?;
    let token = required(
        &raw.account_verification_token,
        "account_verification_token",
    )?;
    if raw.purpose != DEVICE_GENESIS_PURPOSE {
        return Err(crate::ImError::PermissionDenied);
    }
    let expires_at = parse_future_time("expires_at", &raw.expires_at, now)?;
    Ok(AccountVerificationGrant {
        token,
        expires_at: format_time(expires_at)?,
    })
}

pub(crate) fn prepare_device_genesis(
    generated: &crate::internal::identity_generation::GeneratedVNextIdentityWithDaemonSubkey,
    operation_id: String,
    now: OffsetDateTime,
) -> crate::ImResult<PreparedDeviceGenesis> {
    let created_at = now.replace_nanosecond(0).unwrap_or(now);
    let expires_at = created_at + Duration::seconds(PROOF_TTL_SECONDS);
    let nonce = random_b64u(24)?;
    prepare_device_genesis_with_proof_fields(
        generated,
        operation_id,
        &format_time(created_at)?,
        &format_time(expires_at)?,
        &nonce,
    )
}

fn prepare_device_genesis_with_proof_fields(
    generated: &crate::internal::identity_generation::GeneratedVNextIdentityWithDaemonSubkey,
    operation_id: String,
    created_at: &str,
    expires_at: &str,
    nonce: &str,
) -> crate::ImResult<PreparedDeviceGenesis> {
    let operation_id = required(&operation_id, "operation_id")?;
    let unsigned_params = json!({
        "operation_id": operation_id,
        "did_document": generated.did_document,
        "bootstrap_device_id": generated.protocol_device_id.as_str(),
    });
    let mut proof = DeviceProof {
        proof_type: DEVICE_PROOF_TYPE.to_owned(),
        key_id: generated.device_signing_key_id.clone(),
        created_at: created_at.to_owned(),
        expires_at: expires_at.to_owned(),
        nonce: required(nonce, "nonce")?,
        signature: String::new(),
    };
    let private_key = anp::PrivateKeyMaterial::from_pem(&generated.device_signing_private_pem)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    if !matches!(private_key, anp::PrivateKeyMaterial::Ed25519(_)) {
        return Err(crate::ImError::PermissionDenied);
    }
    proof.signature = URL_SAFE_NO_PAD.encode(
        private_key
            .sign_message(&device_proof_bytes(
                &proof,
                DEVICE_GENESIS_PURPOSE,
                DEVICE_GENESIS_METHOD,
                &unsigned_params,
            )?)
            .map_err(|_| crate::ImError::PermissionDenied)?,
    );

    let prepared = PreparedDeviceGenesis {
        operation_id,
        did_document: generated.did_document.clone(),
        bootstrap_device_id: generated.protocol_device_id.as_str().to_owned(),
        bootstrap_device_proof: proof,
    };
    verify_prepared_proof(&prepared)?;
    Ok(prepared)
}

pub(crate) fn build_device_genesis_call(
    prepared: &PreparedDeviceGenesis,
    account_verification_token: &str,
) -> crate::ImResult<DeviceGenesisWireCall> {
    let account_verification_token =
        required(account_verification_token, "account_verification_token")?;
    verify_prepared_proof(prepared)?;
    Ok(DeviceGenesisWireCall {
        endpoint: super::DID_AUTH_RPC_ENDPOINT,
        method: DEVICE_GENESIS_METHOD,
        params: json!({
            "operation_id": prepared.operation_id,
            "account_verification_token": account_verification_token,
            "did_document": prepared.did_document,
            "bootstrap_device_id": prepared.bootstrap_device_id,
            "bootstrap_device_proof": prepared.bootstrap_device_proof,
        }),
    })
}

pub(crate) fn prepare_device_token_issue(
    operation_id: String,
    did_document: &Value,
    device_id: &str,
    signing_key_id: &str,
    expected_scopes: Vec<String>,
    signing_private: &anp::PrivateKeyMaterial,
    service_domain: &str,
) -> crate::ImResult<PreparedDeviceTokenIssue> {
    prepare_device_token_issue_for_readiness(
        operation_id,
        did_document,
        device_id,
        signing_key_id,
        expected_scopes,
        signing_private,
        service_domain,
        false,
    )
}

pub(crate) fn prepare_management_ready_device_token_issue(
    operation_id: String,
    did_document: &Value,
    device_id: &str,
    signing_key_id: &str,
    signing_private: &anp::PrivateKeyMaterial,
    service_domain: &str,
) -> crate::ImResult<PreparedDeviceTokenIssue> {
    prepare_device_token_issue_for_readiness(
        operation_id,
        did_document,
        device_id,
        signing_key_id,
        expected_token_scopes(true),
        signing_private,
        service_domain,
        true,
    )
}

#[allow(clippy::too_many_arguments)]
fn prepare_device_token_issue_for_readiness(
    operation_id: String,
    did_document: &Value,
    device_id: &str,
    signing_key_id: &str,
    expected_scopes: Vec<String>,
    signing_private: &anp::PrivateKeyMaterial,
    service_domain: &str,
    management_ready: bool,
) -> crate::ImResult<PreparedDeviceTokenIssue> {
    let operation_id = required(&operation_id, "operation_id")?;
    let did = did_document
        .get("id")
        .and_then(Value::as_str)
        .map(|value| required(value, "did"))
        .transpose()?
        .ok_or(crate::ImError::PermissionDenied)?;
    let device_id = required(device_id, "device_id")?;
    let signing_key_id = required(signing_key_id, "signing_key_id")?;
    if !matches!(signing_private, anp::PrivateKeyMaterial::Ed25519(_))
        || !anp::authentication::validate_did_document_binding(did_document, true)
        || expected_scopes != expected_token_scopes(management_ready)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let manifest = anp::authentication::validate_device_manifest(did_document)
        .map_err(|_| crate::ImError::PermissionDenied)?
        .ok_or(crate::ImError::PermissionDenied)?;
    let entry = manifest
        .devices
        .iter()
        .find(|entry| entry.device_id == device_id)
        .ok_or(crate::ImError::PermissionDenied)?;
    let signing_method = did_document
        .get("verificationMethod")
        .and_then(Value::as_array)
        .and_then(|methods| {
            methods.iter().find(|method| {
                method.get("id").and_then(Value::as_str) == Some(signing_key_id.as_str())
            })
        })
        .ok_or(crate::ImError::PermissionDenied)?;
    let expected_public = anp::authentication::extract_public_key(signing_method)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let actual_public = signing_private.public_key();
    if entry.signing_key_id != signing_key_id || actual_public.to_pem() != expected_public.to_pem()
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let service_domain = required(service_domain, "service_domain")?;
    let mut signing_document = did_document.clone();
    let authentication = signing_document
        .get_mut("authentication")
        .and_then(Value::as_array_mut)
        .ok_or(crate::ImError::PermissionDenied)?;
    let method_reference = Value::String(signing_key_id.clone());
    authentication.retain(|entry| entry != &method_reference);
    authentication.insert(0, method_reference);
    let authorization = anp::authentication::generate_auth_header(
        &signing_document,
        &service_domain,
        signing_private,
        "1.1",
    )
    .map_err(|_| crate::ImError::PermissionDenied)?;
    let prepared = PreparedDeviceTokenIssue {
        operation_id,
        did,
        device_id,
        signing_key_id,
        expected_scopes,
        authorization,
    };
    verify_prepared_device_token_issue(&prepared, did_document, &service_domain)?;
    Ok(prepared)
}

pub(crate) fn device_token_authorization_needs_refresh(
    prepared: &PreparedDeviceTokenIssue,
    now: OffsetDateTime,
) -> crate::ImResult<bool> {
    let parsed = anp::authentication::extract_auth_header_parts(&prepared.authorization)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let issued_at = OffsetDateTime::parse(&parsed.timestamp, &Rfc3339)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    if issued_at > now + Duration::seconds(60) {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(now - issued_at >= Duration::seconds(180))
}

pub(crate) fn verify_prepared_device_token_issue(
    prepared: &PreparedDeviceTokenIssue,
    did_document: &Value,
    service_domain: &str,
) -> crate::ImResult<()> {
    let parsed = anp::authentication::extract_auth_header_parts(&prepared.authorization)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let expected_fragment = prepared
        .signing_key_id
        .strip_prefix(&format!("{}#", prepared.did))
        .ok_or(crate::ImError::PermissionDenied)?;
    if prepared.operation_id.trim().is_empty()
        || prepared.device_id.trim().is_empty()
        || did_document.get("id").and_then(Value::as_str) != Some(prepared.did.as_str())
        || parsed.did != prepared.did
        || parsed.verification_method != expected_fragment
        || (prepared.expected_scopes != expected_token_scopes(false)
            && prepared.expected_scopes != expected_token_scopes(true))
    {
        return Err(crate::ImError::PermissionDenied);
    }
    anp::authentication::verify_auth_header_signature(
        &prepared.authorization,
        did_document,
        service_domain,
    )
    .map_err(|_| crate::ImError::PermissionDenied)
}

pub(crate) fn build_device_token_issue_call(
    prepared: &PreparedDeviceTokenIssue,
) -> crate::ImResult<DeviceTokenIssueWireCall> {
    if prepared.operation_id.trim().is_empty()
        || prepared.did.trim().is_empty()
        || prepared.device_id.trim().is_empty()
        || prepared.authorization.trim().is_empty()
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(DeviceTokenIssueWireCall {
        endpoint: super::DID_AUTH_RPC_ENDPOINT,
        method: DEVICE_TOKEN_ISSUE_METHOD,
        params: json!({
            "operation_id": prepared.operation_id,
            "did": prepared.did,
            "device_id": prepared.device_id,
            "authorization": prepared.authorization,
        }),
    })
}

pub(crate) fn parse_device_token_issue_result(
    raw: Value,
    prepared: &PreparedDeviceTokenIssue,
    expected_auth_generation: u64,
    now: OffsetDateTime,
) -> crate::ImResult<DeviceTokenIssueResult> {
    let mut raw: RawDeviceTokenIssueResult = strict_from_value(raw, "device token issue result")?;
    if raw.token_type != "bearer"
        || raw.device_id != prepared.device_id
        || raw.auth_generation != expected_auth_generation
        || raw.scopes != prepared.expected_scopes
        || expected_auth_generation == 0
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let access_token = required(&raw.access_token, "access_token")?;
    let refresh_token = required(&raw.refresh_token, "refresh_token")?;
    if access_token == refresh_token {
        return Err(crate::ImError::PermissionDenied);
    }
    let expires_at = parse_future_time("expires_at", &raw.expires_at, now)?;
    let access = validate_device_token(
        &access_token,
        "access",
        DEVICE_ACCESS_PURPOSE,
        &prepared.did,
        None,
        &prepared.device_id,
        &prepared.signing_key_id,
        expected_auth_generation,
        &prepared.expected_scopes,
        now,
        Some(expires_at),
    )?;
    let refresh = validate_device_token(
        &refresh_token,
        "refresh",
        DEVICE_REFRESH_PURPOSE,
        &prepared.did,
        Some(&access.user_id),
        &prepared.device_id,
        &prepared.signing_key_id,
        expected_auth_generation,
        &prepared.expected_scopes,
        now,
        None,
    )?;
    if refresh.user_id != access.user_id {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(DeviceTokenIssueResult {
        access_token,
        refresh_token,
        expires_at: format_time(expires_at)?,
        user_id: access.user_id,
        device_id: std::mem::take(&mut raw.device_id),
        auth_generation: raw.auth_generation,
        scopes: std::mem::take(&mut raw.scopes),
    })
}

pub(crate) fn parse_device_genesis_result(
    raw: Value,
    generated: &crate::internal::identity_generation::GeneratedVNextIdentityWithDaemonSubkey,
    now: OffsetDateTime,
) -> crate::ImResult<DeviceGenesisResult> {
    validate_generated_document(generated)?;
    let raw: RawDeviceGenesisResult = strict_from_value(raw, "device genesis result")?;
    if raw.did != generated.did.as_str()
        || raw.user_id.trim().is_empty()
        || raw.device.device_id != generated.protocol_device_id.as_str()
        || raw.device.signing_key_id != generated.device_signing_key_id
        || raw.device.e2ee_key_id != generated.device_e2ee_key_id
        || raw.device.status != "active"
        || raw.device.role != "admin"
        || !raw.device.management_ready
        || raw.device.auth_generation != 1
        || raw.checkpoint.document_version != 1
        || raw.checkpoint.registry_version != 1
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let expected_hash = document_hash(&generated.did_document)?;
    if raw.checkpoint.document_hash != expected_hash {
        return Err(crate::ImError::PermissionDenied);
    }
    let access_token = required(&raw.access_token, "access_token")?;
    let refresh_token = required(&raw.refresh_token, "refresh_token")?;
    if access_token == refresh_token {
        return Err(crate::ImError::PermissionDenied);
    }
    let token_expires = parse_future_time("token_expires_at", &raw.token_expires_at, now)?;
    validate_device_token(
        &access_token,
        "access",
        DEVICE_ACCESS_PURPOSE,
        generated.did.as_str(),
        Some(&raw.user_id),
        generated.protocol_device_id.as_str(),
        &generated.device_signing_key_id,
        1,
        &expected_token_scopes(true),
        now,
        Some(token_expires),
    )?;
    validate_device_token(
        &refresh_token,
        "refresh",
        DEVICE_REFRESH_PURPOSE,
        generated.did.as_str(),
        Some(&raw.user_id),
        generated.protocol_device_id.as_str(),
        &generated.device_signing_key_id,
        1,
        &expected_token_scopes(true),
        now,
        None,
    )?;
    let result = DeviceGenesisResult {
        did: raw.did,
        user_id: raw.user_id,
        checkpoint: IdentityInternalCheckpoint {
            document_version: raw.checkpoint.document_version,
            document_hash: raw.checkpoint.document_hash,
            registry_version: raw.checkpoint.registry_version,
        },
        device: raw.device,
        access_token,
        refresh_token,
        token_expires_at: format_time(token_expires)?,
    };
    result.device_state().validate_for_did(&generated.did)?;
    Ok(result)
}

pub(crate) fn document_hash(document: &Value) -> crate::ImResult<String> {
    let canonical = serde_json_canonicalizer::to_vec(document).map_err(|err| {
        crate::ImError::Serialization {
            detail: err.to_string(),
        }
    })?;
    Ok(format!(
        "sha256:{}",
        URL_SAFE_NO_PAD.encode(Sha256::digest(canonical))
    ))
}

pub(crate) fn strip_proof_and_token_fields(value: &Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .filter(|(key, _)| !key.ends_with("_proof") && !key.ends_with("_token"))
                .map(|(key, value)| (key.clone(), strip_proof_and_token_fields(value)))
                .collect(),
        ),
        Value::Array(values) => {
            Value::Array(values.iter().map(strip_proof_and_token_fields).collect())
        }
        _ => value.clone(),
    }
}

pub(crate) fn validate_generated_document(
    generated: &crate::internal::identity_generation::GeneratedVNextIdentityWithDaemonSubkey,
) -> crate::ImResult<()> {
    if generated.did_document.get("id").and_then(Value::as_str) != Some(generated.did.as_str())
        || !anp::authentication::validate_did_document_binding(&generated.did_document, true)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let manifest = anp::authentication::validate_device_manifest(&generated.did_document)
        .map_err(|_| crate::ImError::PermissionDenied)?
        .ok_or(crate::ImError::PermissionDenied)?;
    if manifest.devices.len() != 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    let device = &manifest.devices[0];
    if device.device_id != generated.protocol_device_id.as_str()
        || device.signing_key_id != generated.device_signing_key_id
        || device.e2ee_key_id != generated.device_e2ee_key_id
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

/// Validates the generation-bound management-ready token pair returned by a
/// first-party control-plane operation and returns its stable user id.
///
/// Recovery reuses the same device-token format as Genesis, but its result
/// intentionally does not repeat `user_id` outside the tokens.
pub(crate) fn validate_management_ready_token_pair(
    generated: &crate::internal::identity_generation::GeneratedVNextIdentityWithDaemonSubkey,
    access_token: &str,
    refresh_token: &str,
    token_expires_at: &str,
    now: OffsetDateTime,
) -> crate::ImResult<String> {
    validate_generated_document(generated)?;
    let access_token = access_token.trim();
    let refresh_token = refresh_token.trim();
    if access_token.is_empty() || refresh_token.is_empty() {
        return Err(crate::ImError::PermissionDenied);
    }
    if access_token == refresh_token {
        return Err(crate::ImError::PermissionDenied);
    }
    let expires_at = parse_future_time("token_expires_at", token_expires_at, now)?;
    let expected_scopes = expected_token_scopes(true);
    let access = validate_device_token(
        access_token,
        "access",
        DEVICE_ACCESS_PURPOSE,
        generated.did.as_str(),
        None,
        generated.protocol_device_id.as_str(),
        &generated.device_signing_key_id,
        1,
        &expected_scopes,
        now,
        Some(expires_at),
    )?;
    let refresh = validate_device_token(
        refresh_token,
        "refresh",
        DEVICE_REFRESH_PURPOSE,
        generated.did.as_str(),
        Some(&access.user_id),
        generated.protocol_device_id.as_str(),
        &generated.device_signing_key_id,
        1,
        &expected_scopes,
        now,
        None,
    )?;
    if access.user_id != refresh.user_id {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(access.user_id)
}

fn verify_prepared_proof(prepared: &PreparedDeviceGenesis) -> crate::ImResult<()> {
    let params = json!({
        "operation_id": prepared.operation_id,
        "did_document": prepared.did_document,
        "bootstrap_device_id": prepared.bootstrap_device_id,
    });
    let method = prepared
        .did_document
        .get("verificationMethod")
        .and_then(Value::as_array)
        .and_then(|methods| {
            methods.iter().find(|method| {
                method.get("id").and_then(Value::as_str)
                    == Some(prepared.bootstrap_device_proof.key_id.as_str())
            })
        })
        .ok_or(crate::ImError::PermissionDenied)?;
    let verification_method = anp::authentication::create_verification_method(method)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    if !matches!(
        verification_method.public_key,
        anp::PublicKeyMaterial::Ed25519(_)
    ) {
        return Err(crate::ImError::PermissionDenied);
    }
    verification_method
        .verify_signature(
            &device_proof_bytes(
                &prepared.bootstrap_device_proof,
                DEVICE_GENESIS_PURPOSE,
                DEVICE_GENESIS_METHOD,
                &params,
            )?,
            &prepared.bootstrap_device_proof.signature,
        )
        .map_err(|_| crate::ImError::PermissionDenied)
}

fn device_proof_bytes(
    proof: &DeviceProof,
    purpose: &str,
    method: &str,
    params: &Value,
) -> crate::ImResult<Vec<u8>> {
    let full_params = json!({
        "account_verification_token": "removed",
        "bootstrap_device_proof": proof,
        "operation_id": params.get("operation_id").cloned().unwrap_or(Value::Null),
        "did_document": params.get("did_document").cloned().unwrap_or(Value::Null),
        "bootstrap_device_id": params.get("bootstrap_device_id").cloned().unwrap_or(Value::Null),
    });
    let stripped_params = strip_proof_and_token_fields(&full_params);
    serde_json_canonicalizer::to_vec(&json!({
        "type": proof.proof_type,
        "purpose": purpose,
        "method": method,
        "key_id": proof.key_id,
        "created_at": proof.created_at,
        "expires_at": proof.expires_at,
        "nonce": proof.nonce,
        "params": stripped_params,
    }))
    .map_err(|err| crate::ImError::Serialization {
        detail: err.to_string(),
    })
}

struct ValidatedDeviceTokenClaims {
    user_id: String,
}

fn validate_device_token(
    token: &str,
    token_type: &str,
    purpose: &str,
    did: &str,
    expected_user_id: Option<&str>,
    device_id: &str,
    key_id: &str,
    auth_generation: u64,
    expected_scopes: &[String],
    now: OffsetDateTime,
    expected_expiry: Option<OffsetDateTime>,
) -> crate::ImResult<ValidatedDeviceTokenClaims> {
    let payload_segment = token
        .split('.')
        .nth(1)
        .ok_or(crate::ImError::PermissionDenied)?;
    let payload: Value = serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(payload_segment)
            .map_err(|_| crate::ImError::PermissionDenied)?,
    )
    .map_err(|_| crate::ImError::PermissionDenied)?;
    let object = payload
        .as_object()
        .ok_or(crate::ImError::PermissionDenied)?;
    if object.get("profile").and_then(Value::as_str) != Some(DEVICE_TOKEN_PROFILE)
        || object.get("purpose").and_then(Value::as_str) != Some(purpose)
        || object.get("type").and_then(Value::as_str) != Some(token_type)
        || object.get("sub").and_then(Value::as_str) != Some(did)
        || object.get("did").and_then(Value::as_str) != Some(did)
        || object.get("device_id").and_then(Value::as_str) != Some(device_id)
        || object.get("key_id").and_then(Value::as_str) != Some(key_id)
        || object.get("auth_generation").and_then(Value::as_u64) != Some(auth_generation)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let user_id = object
        .get("user_id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or(crate::ImError::PermissionDenied)?;
    if expected_user_id.is_some_and(|expected| expected != user_id) {
        return Err(crate::ImError::PermissionDenied);
    }
    let scopes = object
        .get("scopes")
        .and_then(Value::as_array)
        .ok_or(crate::ImError::PermissionDenied)?;
    let scopes = scopes
        .iter()
        .map(|scope| scope.as_str().ok_or(crate::ImError::PermissionDenied))
        .collect::<crate::ImResult<Vec<_>>>()?;
    let unique = scopes.iter().copied().collect::<BTreeSet<_>>();
    let expected_scope_set = expected_scopes
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if scopes != expected_scopes || unique.len() != scopes.len() || unique != expected_scope_set {
        return Err(crate::ImError::PermissionDenied);
    }
    if object
        .get("jti")
        .and_then(Value::as_str)
        .is_none_or(|value| value.trim().is_empty())
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let issued = object
        .get("iat")
        .and_then(Value::as_i64)
        .and_then(|value| OffsetDateTime::from_unix_timestamp(value).ok())
        .ok_or(crate::ImError::PermissionDenied)?;
    let not_before = object
        .get("nbf")
        .and_then(Value::as_i64)
        .and_then(|value| OffsetDateTime::from_unix_timestamp(value).ok())
        .ok_or(crate::ImError::PermissionDenied)?;
    let exp = object
        .get("exp")
        .and_then(Value::as_i64)
        .and_then(|value| OffsetDateTime::from_unix_timestamp(value).ok())
        .ok_or(crate::ImError::PermissionDenied)?;
    if not_before != issued || issued > now + Duration::seconds(60) || exp <= now {
        return Err(crate::ImError::PermissionDenied);
    }
    if expected_expiry.is_some_and(|expected| exp != expected) {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(ValidatedDeviceTokenClaims {
        user_id: user_id.to_owned(),
    })
}

fn expected_token_scopes(management_ready: bool) -> Vec<String> {
    if management_ready {
        ["device:manage", "device:read", "message:connect"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    } else {
        ["device:read", "message:connect"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    }
}

fn strict_from_value<T: for<'de> Deserialize<'de>>(
    value: Value,
    context: &str,
) -> crate::ImResult<T> {
    serde_json::from_value(value).map_err(|_| crate::ImError::Serialization {
        detail: format!("invalid {context}"),
    })
}

fn parse_future_time(
    field: &str,
    value: &str,
    now: OffsetDateTime,
) -> crate::ImResult<OffsetDateTime> {
    let parsed = OffsetDateTime::parse(value.trim(), &Rfc3339).map_err(|_| {
        crate::ImError::invalid_input(Some(field.to_owned()), format!("invalid {field}"))
    })?;
    if parsed <= now {
        return Err(crate::ImError::SessionExpired);
    }
    Ok(parsed)
}

fn format_time(value: OffsetDateTime) -> crate::ImResult<String> {
    value
        .format(&Rfc3339)
        .map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })
}

fn required(value: &str, field: &str) -> crate::ImResult<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            format!("{field} is required"),
        ));
    }
    Ok(value.to_owned())
}

fn required_lower_handle(value: &str) -> crate::ImResult<String> {
    let value = required(value, "target_handle")?;
    if value != value.to_ascii_lowercase() || value.contains('.') {
        return Err(crate::ImError::invalid_input(
            Some("target_handle".to_owned()),
            "target_handle must be a lowercase local part",
        ));
    }
    Ok(value)
}

fn required_lower_domain(value: &str) -> crate::ImResult<String> {
    let value = required(value, "target_handle_domain")?;
    let normalized = value.trim_end_matches('.').to_ascii_lowercase();
    if value != normalized || normalized.contains(['/', ':']) {
        return Err(crate::ImError::invalid_input(
            Some("target_handle_domain".to_owned()),
            "target_handle_domain must be a normalized domain",
        ));
    }
    Ok(normalized)
}

fn random_b64u(len: usize) -> crate::ImResult<String> {
    let mut bytes = vec![0_u8; len];
    rand::rngs::OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| crate::ImError::Internal {
            message: "secure genesis nonce generation failed".to_owned(),
        })?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recursive_signing_filter_removes_all_proof_and_token_fields() {
        let filtered = strip_proof_and_token_fields(&json!({
            "keep": 1,
            "access_token": "secret",
            "proof": "kept-because-name-is-not-suffixed",
            "nested": [{"join_session_token": "secret", "value": 2}],
            "device_proof": {"signature": "secret"},
        }));
        assert_eq!(
            filtered,
            json!({"keep": 1, "proof": "kept-because-name-is-not-suffixed", "nested": [{"value": 2}]})
        );
    }

    #[test]
    fn prepared_genesis_proof_is_bound_to_all_unsigned_params() {
        let generated = crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
            "awiki.info", "alice", None, None,
        ).unwrap();
        let prepared = prepare_device_genesis_with_proof_fields(
            &generated,
            "op-fixed".to_owned(),
            "2026-07-19T00:00:00Z",
            "2026-07-19T00:05:00Z",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        )
        .unwrap();
        verify_prepared_proof(&prepared).unwrap();

        let mut tampered = prepared.clone();
        tampered.bootstrap_device_id = "dev-tampered".to_owned();
        assert_eq!(
            verify_prepared_proof(&tampered),
            Err(crate::ImError::PermissionDenied)
        );
    }

    #[test]
    fn bootstrap_proof_has_stable_eight_field_jcs_vector() {
        let proof = DeviceProof {
            proof_type: DEVICE_PROOF_TYPE.to_owned(),
            key_id: "did:wba:awiki.info:user:alice:e1_fixed#dev-fixed-sign".to_owned(),
            created_at: "2026-07-19T00:00:00Z".to_owned(),
            expires_at: "2026-07-19T00:05:00Z".to_owned(),
            nonce: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned(),
            signature: String::new(),
        };
        let params = json!({
            "operation_id": "op-fixed",
            "did_document": {
                "id": "did:wba:awiki.info:user:alice:e1_fixed",
                "verificationMethod": [],
            },
            "bootstrap_device_id": "dev-fixed",
        });
        let bytes = device_proof_bytes(
            &proof,
            DEVICE_GENESIS_PURPOSE,
            DEVICE_GENESIS_METHOD,
            &params,
        )
        .unwrap();
        assert_eq!(
            std::str::from_utf8(&bytes).unwrap(),
            r#"{"created_at":"2026-07-19T00:00:00Z","expires_at":"2026-07-19T00:05:00Z","key_id":"did:wba:awiki.info:user:alice:e1_fixed#dev-fixed-sign","method":"device_genesis","nonce":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA","params":{"bootstrap_device_id":"dev-fixed","did_document":{"id":"did:wba:awiki.info:user:alice:e1_fixed","verificationMethod":[]},"operation_id":"op-fixed"},"purpose":"awiki.device.genesis.v1","type":"awiki-device-signature-v1"}"#
        );
        let key = anp::PrivateKeyMaterial::Ed25519(ed25519_dalek::SigningKey::from_bytes(&[7; 32]));
        let signature = URL_SAFE_NO_PAD.encode(key.sign_message(&bytes).unwrap());
        assert_eq!(
            signature,
            "4wWH43rIER5uxguE_qfUj3A88qBOTer-Z0B4QM4Ee32IrgISTwwPEMYiHE2EH69hZJrbvqQrvefjEsUrD0yYAA"
        );
    }

    #[test]
    fn genesis_result_parser_is_closed_and_checks_device_binding() {
        let generated = crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
            "awiki.info", "alice", None, None,
        ).unwrap();
        let now = OffsetDateTime::now_utc().replace_nanosecond(0).unwrap();
        let access_exp = now + Duration::hours(1);
        let refresh_exp = now + Duration::days(7);
        let user_id = "user-fixed";
        let access = fake_device_token(
            &generated,
            user_id,
            "access",
            DEVICE_ACCESS_PURPOSE,
            access_exp,
            &["device:manage", "device:read", "message:connect"],
        );
        let refresh = fake_device_token(
            &generated,
            user_id,
            "refresh",
            DEVICE_REFRESH_PURPOSE,
            refresh_exp,
            &["device:manage", "device:read", "message:connect"],
        );
        let valid = json!({
            "did": generated.did.as_str(),
            "user_id": user_id,
            "checkpoint": {
                "document_version": 1,
                "document_hash": document_hash(&generated.did_document).unwrap(),
                "registry_version": 1,
            },
            "device": {
                "device_id": generated.protocol_device_id.as_str(),
                "signing_key_id": generated.device_signing_key_id,
                "e2ee_key_id": generated.device_e2ee_key_id,
                "status": "active",
                "role": "admin",
                "management_ready": true,
                "auth_generation": 1,
            },
            "access_token": access,
            "refresh_token": refresh,
            "token_expires_at": format_time(access_exp).unwrap(),
        });
        parse_device_genesis_result(valid.clone(), &generated, now).unwrap();

        let mut unknown = valid.clone();
        unknown
            .as_object_mut()
            .unwrap()
            .insert("extra".to_owned(), json!(true));
        assert!(matches!(
            parse_device_genesis_result(unknown, &generated, now),
            Err(crate::ImError::Serialization { .. })
        ));
        let mut tampered = valid;
        tampered["device"]["device_id"] = json!("dev-tampered");
        assert_eq!(
            parse_device_genesis_result(tampered, &generated, now),
            Err(crate::ImError::PermissionDenied)
        );
    }

    #[test]
    fn token_bearing_debug_is_redacted() {
        let call = DeviceGenesisWireCall {
            endpoint: super::super::DID_AUTH_RPC_ENDPOINT,
            method: DEVICE_GENESIS_METHOD,
            params: json!({"account_verification_token": "grant-secret"}),
        };
        let debug = format!("{call:?}");
        assert!(!debug.contains("grant-secret"));
        assert!(debug.contains("redacted-token-bearing-params"));
    }

    #[test]
    fn token_issue_is_bound_to_device_key_and_rejects_token_claim_mismatch() {
        let generated = crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
            "awiki.info", "alice", None, None,
        ).unwrap();
        let private =
            anp::PrivateKeyMaterial::from_pem(&generated.device_signing_private_pem).unwrap();
        let prepared = prepare_device_token_issue(
            "op-token-fixed".to_owned(),
            &generated.did_document,
            generated.protocol_device_id.as_str(),
            &generated.device_signing_key_id,
            vec!["device:read".to_owned(), "message:connect".to_owned()],
            &private,
            "awiki.info",
        )
        .unwrap();
        verify_prepared_device_token_issue(&prepared, &generated.did_document, "awiki.info")
            .unwrap();
        let parsed =
            anp::authentication::extract_auth_header_parts(&prepared.authorization).unwrap();
        assert_eq!(
            parsed.verification_method,
            generated.device_signing_key_id.split('#').nth(1).unwrap()
        );

        let now = OffsetDateTime::now_utc().replace_nanosecond(0).unwrap();
        let access_exp = now + Duration::hours(1);
        let refresh_exp = now + Duration::days(7);
        let access = fake_device_token(
            &generated,
            "user-fixed",
            "access",
            DEVICE_ACCESS_PURPOSE,
            access_exp,
            &["device:read", "message:connect"],
        );
        let refresh = fake_device_token(
            &generated,
            "user-fixed",
            "refresh",
            DEVICE_REFRESH_PURPOSE,
            refresh_exp,
            &["device:read", "message:connect"],
        );
        let valid = json!({
            "access_token": access,
            "refresh_token": refresh,
            "token_type": "bearer",
            "expires_at": format_time(access_exp).unwrap(),
            "device_id": generated.protocol_device_id.as_str(),
            "auth_generation": 1,
            "scopes": ["device:read", "message:connect"],
        });
        let result = parse_device_token_issue_result(valid.clone(), &prepared, 1, now).unwrap();
        assert_eq!(result.user_id, "user-fixed");

        let mut wrong_device = valid;
        wrong_device["device_id"] = json!("dev-attacker");
        assert_eq!(
            parse_device_token_issue_result(wrong_device, &prepared, 1, now),
            Err(crate::ImError::PermissionDenied)
        );
    }

    fn fake_device_token(
        generated: &crate::internal::identity_generation::GeneratedVNextIdentityWithDaemonSubkey,
        user_id: &str,
        token_type: &str,
        purpose: &str,
        expires_at: OffsetDateTime,
        scopes: &[&str],
    ) -> String {
        let issued_at = OffsetDateTime::now_utc().unix_timestamp();
        let payload = json!({
            "profile": DEVICE_TOKEN_PROFILE,
            "purpose": purpose,
            "type": token_type,
            "sub": generated.did.as_str(),
            "did": generated.did.as_str(),
            "user_id": user_id,
            "device_id": generated.protocol_device_id.as_str(),
            "key_id": generated.device_signing_key_id,
            "auth_generation": 1,
            "jti": format!("jti-{token_type}"),
            "iat": issued_at,
            "nbf": issued_at,
            "scopes": scopes,
            "exp": expires_at.unix_timestamp(),
        });
        format!(
            "e30.{}.signature",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap())
        )
    }
}
