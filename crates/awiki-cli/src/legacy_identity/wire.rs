use super::service::CommandResult;
use super::types::{IdentityError, IdentitySummary};
use crate::authsdk::{build_json_rpc_payload, HttpError, RpcError};
use crate::transportcfg::Profile;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::fmt;

pub const DID_AUTH_RPC_ENDPOINT: &str = "/user-service/did-auth/rpc";
pub const HANDLE_RPC_ENDPOINT: &str = "/user-service/handle/rpc";
pub const DID_PROFILE_RPC_ENDPOINT: &str = "/user-service/did/profile/rpc";
pub const EMAIL_SEND_ENDPOINT: &str = "/user-service/auth/email-send";
pub const EMAIL_STATUS_ENDPOINT: &str = "/user-service/auth/email-status";
pub const PHONE_BIND_SEND_ENDPOINT: &str = "/user-service/auth/phone-bind-send";
pub const PHONE_BIND_VERIFY_ENDPOINT: &str = "/user-service/auth/phone-bind-verify";

#[derive(Debug, Clone, PartialEq)]
pub struct RpcCall {
    pub endpoint: &'static str,
    pub method: &'static str,
    pub profile: Profile,
    pub params: Value,
}

impl RpcCall {
    pub fn payload(&self) -> Value {
        build_json_rpc_payload(self.method, self.params.clone())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RestCall {
    pub endpoint: &'static str,
    pub method: &'static str,
    pub profile: Profile,
    pub query: BTreeMap<String, String>,
    pub body: Value,
    pub authenticated: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ServiceError {
    pub status_code: u16,
    pub rpc_code: i64,
    pub message: String,
    pub data: Option<Value>,
}

impl fmt::Display for ServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.rpc_code, self.status_code) {
            (code, _) if code != 0 => {
                write!(formatter, "service rpc error {code}: {}", self.message)
            }
            (_, status_code) if status_code != 0 => {
                write!(
                    formatter,
                    "service http error {status_code}: {}",
                    self.message
                )
            }
            _ => formatter.write_str(&self.message),
        }
    }
}

impl std::error::Error for ServiceError {}

impl From<RpcError> for ServiceError {
    fn from(value: RpcError) -> Self {
        Self {
            status_code: 0,
            rpc_code: value.code,
            message: value.message,
            data: value.data,
        }
    }
}

impl From<HttpError> for ServiceError {
    fn from(value: HttpError) -> Self {
        Self {
            status_code: value.status_code,
            rpc_code: 0,
            message: value.message,
            data: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HandleLookupResult {
    #[serde(default)]
    pub handle: String,
    #[serde(default)]
    pub did: String,
    #[serde(default)]
    pub domain: String,
    #[serde(default)]
    pub full_handle: String,
    #[serde(default)]
    pub status: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RegisterRpcParams {
    pub did_document: Value,
    pub handle: String,
    pub phone: Option<String>,
    pub otp_code: Option<String>,
    pub email: Option<String>,
    pub invite_code: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RecoverHandleRpcParams {
    pub did_document: Value,
    pub handle: String,
    pub phone: String,
    pub otp_code: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReplaceDidRpcParams {
    pub new_did_document: Value,
    pub is_public: Option<bool>,
    pub is_agent: Option<bool>,
    pub role: Option<String>,
    pub endpoint_url: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpdateProfileParams {
    pub display_name: String,
    pub bio: String,
    pub tags_csv: String,
    pub markdown: String,
    pub preserve_markdown: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProfileUpdateCall {
    pub call: RpcCall,
    pub changed_fields: Vec<String>,
}

pub fn build_handle_lookup_by_did_rpc_call(did: &str) -> Result<RpcCall, IdentityError> {
    im_core::compat::directory::build_handle_lookup_by_did_rpc_call(did)
        .map(legacy_rpc_call)
        .map_err(identity_error_from_im)
}

pub fn build_handle_lookup_by_handle_rpc_call(handle: &str) -> Result<RpcCall, IdentityError> {
    im_core::compat::directory::build_handle_lookup_by_handle_rpc_call(handle)
        .map(legacy_rpc_call)
        .map_err(identity_error_from_im)
}

pub fn build_profile_resolve_rpc_call(did: &str) -> Result<RpcCall, IdentityError> {
    im_core::compat::directory::build_profile_resolve_rpc_call(did)
        .map(legacy_rpc_call)
        .map_err(identity_error_from_im)
}

pub fn build_public_profile_rpc_call(did: &str) -> Result<RpcCall, IdentityError> {
    im_core::compat::directory::build_public_profile_rpc_call(did)
        .map(legacy_rpc_call)
        .map_err(identity_error_from_im)
}

pub fn build_get_me_profile_rpc_call() -> RpcCall {
    legacy_rpc_call(im_core::compat::identity::build_get_me_profile_rpc_call())
}

pub fn build_refresh_token_rpc_call() -> RpcCall {
    legacy_rpc_call(im_core::compat::identity::build_refresh_token_rpc_call())
}

pub fn build_update_me_profile_rpc_call(
    params: UpdateProfileParams,
) -> Result<ProfileUpdateCall, IdentityError> {
    im_core::compat::identity::build_update_me_profile_rpc_call(sdk_update_profile_params(params))
        .map(|call| ProfileUpdateCall {
            call: legacy_rpc_call(call.call),
            changed_fields: call.changed_fields,
        })
        .map_err(identity_error_from_im)
}

pub fn build_register_rpc_call(params: RegisterRpcParams) -> Result<RpcCall, IdentityError> {
    im_core::compat::identity::build_register_rpc_call(sdk_register_params(params))
        .map(legacy_rpc_call)
        .map_err(identity_error_from_im)
}

pub fn build_recover_handle_rpc_call(
    params: RecoverHandleRpcParams,
) -> Result<RpcCall, IdentityError> {
    im_core::compat::identity::build_recover_handle_rpc_call(sdk_recover_params(params))
        .map(legacy_rpc_call)
        .map_err(identity_error_from_im)
}

pub fn build_replace_did_rpc_call(params: ReplaceDidRpcParams) -> RpcCall {
    legacy_rpc_call(im_core::compat::identity::build_replace_did_rpc_call(
        sdk_replace_did_params(params),
    ))
}

pub fn build_send_otp_rpc_call(phone: &str) -> Result<RpcCall, IdentityError> {
    im_core::compat::directory::build_send_otp_rpc_call(phone)
        .map(legacy_rpc_call)
        .map_err(identity_error_from_im)
}

pub fn build_email_send_rest_call(
    email: &str,
    handle: Option<&str>,
    authenticated: bool,
) -> Result<RestCall, IdentityError> {
    im_core::compat::identity::build_email_send_rest_call(email, handle, authenticated)
        .map(legacy_rest_call)
        .map_err(identity_error_from_im)
}

pub fn build_email_status_rest_call(
    email: &str,
    handle: Option<&str>,
    authenticated: bool,
) -> Result<RestCall, IdentityError> {
    im_core::compat::identity::build_email_status_rest_call(email, handle, authenticated)
        .map(legacy_rest_call)
        .map_err(identity_error_from_im)
}

pub fn build_phone_bind_send_rest_call(phone: &str) -> Result<RestCall, IdentityError> {
    im_core::compat::identity::build_phone_bind_send_rest_call(phone)
        .map(legacy_rest_call)
        .map_err(identity_error_from_im)
}

pub fn build_phone_bind_verify_rest_call(
    phone: &str,
    code: &str,
) -> Result<RestCall, IdentityError> {
    im_core::compat::identity::build_phone_bind_verify_rest_call(phone, code)
        .map(legacy_rest_call)
        .map_err(identity_error_from_im)
}

pub fn build_update_profile_payload(
    params: UpdateProfileParams,
) -> Result<(Value, Vec<String>), IdentityError> {
    im_core::compat::identity::build_update_profile_payload(sdk_update_profile_params(params))
        .map_err(identity_error_from_im)
}

pub fn normalize_phone(phone: &str) -> Result<String, IdentityError> {
    im_core::compat::identity::normalize_phone(phone).map_err(identity_error_from_im)
}

pub fn sanitize_otp(code: &str) -> String {
    im_core::compat::identity::sanitize_otp(code)
}

pub fn split_csv(raw: &str) -> Vec<String> {
    im_core::compat::identity::split_csv(raw)
}

pub fn normalize_email(email: &str) -> String {
    im_core::compat::identity::normalize_email(email)
}

pub fn handle_lookup_error_is_not_found(error: &ServiceError) -> bool {
    error.status_code == 404 || error.rpc_code == -32002
}

pub fn normalize_handle_lookup_result(result: HandleLookupResult) -> Option<HandleLookupResult> {
    if result.handle.trim().is_empty() || result.did.trim().is_empty() {
        return None;
    }
    Some(result)
}

pub fn register_phone_otp_result(
    identity_name: &str,
    handle: &str,
    full_handle: &str,
    phone: &str,
    result: Value,
) -> Result<CommandResult, IdentityError> {
    let full_handle = full_handle.trim();
    Ok(CommandResult {
        data: json!({
            "action": "send_handle_otp",
            "identity_name": identity_name,
            "handle": handle.trim(),
            "full_handle": full_handle,
            "method": "phone",
            "phone": normalize_phone(phone)?,
            "verification_state": "otp_sent",
            "result": result,
        }),
        summary: format!("OTP sent for handle {full_handle}"),
        warnings: Vec::new(),
    })
}

pub fn registration_email_sent_result(
    identity_name: &str,
    handle: &str,
    full_handle: &str,
    email: &str,
    result: Value,
) -> CommandResult {
    let full_handle = full_handle.trim();
    CommandResult {
        data: json!({
            "action": "send_registration_email",
            "identity_name": identity_name,
            "handle": handle.trim(),
            "full_handle": full_handle,
            "method": "email",
            "email": normalize_email(email),
            "verification_state": "email_sent",
            "result": result,
        }),
        summary: format!("Activation email sent for handle {full_handle}"),
        warnings: Vec::new(),
    }
}

pub fn registration_email_pending_result(
    identity_name: &str,
    handle: &str,
    full_handle: &str,
    email: &str,
) -> CommandResult {
    CommandResult {
        data: json!({
            "action": "wait_for_registration_email",
            "identity_name": identity_name,
            "handle": handle.trim(),
            "full_handle": full_handle.trim(),
            "method": "email",
            "email": normalize_email(email),
            "verification_state": "pending",
        }),
        summary: "Email verification is still pending".to_string(),
        warnings: Vec::new(),
    }
}

pub fn register_completed_result(
    identity: &IdentitySummary,
    full_handle: &str,
    method: &str,
    result: Value,
) -> CommandResult {
    let full_handle = full_handle.trim();
    CommandResult {
        data: json!({
            "action": "register_handle",
            "identity": identity_value(identity),
            "full_handle": full_handle,
            "method": method,
            "verification_state": "completed",
            "result": result,
        }),
        summary: format!("Handle {full_handle} registered successfully"),
        warnings: Vec::new(),
    }
}

pub fn recover_otp_result(
    identity_name: &str,
    handle: &str,
    full_handle: &str,
    phone: &str,
    result: Value,
) -> Result<CommandResult, IdentityError> {
    let full_handle = full_handle.trim();
    Ok(CommandResult {
        data: json!({
            "action": "send_recover_otp",
            "identity_name": identity_name,
            "handle": handle.trim(),
            "full_handle": full_handle,
            "method": "phone",
            "phone": normalize_phone(phone)?,
            "verification_state": "otp_sent",
            "result": result,
        }),
        summary: format!("OTP sent for handle {full_handle} recovery"),
        warnings: Vec::new(),
    })
}

pub fn bind_phone_otp_result(
    identity: &IdentitySummary,
    phone: &str,
    result: Value,
) -> Result<CommandResult, IdentityError> {
    Ok(CommandResult {
        data: json!({
            "action": "send_bind_phone_otp",
            "identity": identity_value(identity),
            "phone": normalize_phone(phone)?,
            "verification_state": "otp_sent",
            "result": result,
        }),
        summary: "Phone binding OTP sent".to_string(),
        warnings: Vec::new(),
    })
}

pub fn bind_phone_completed_result(
    identity: &IdentitySummary,
    phone: &str,
    result: Value,
) -> Result<CommandResult, IdentityError> {
    Ok(CommandResult {
        data: json!({
            "action": "bind_phone",
            "identity": identity_value(identity),
            "phone": normalize_phone(phone)?,
            "verification_state": "completed",
            "result": result,
        }),
        summary: "Phone bound successfully".to_string(),
        warnings: Vec::new(),
    })
}

pub fn bind_email_sent_result(
    identity: &IdentitySummary,
    email: &str,
    result: Value,
) -> CommandResult {
    CommandResult {
        data: json!({
            "action": "send_bind_email",
            "identity": identity_value(identity),
            "email": normalize_email(email),
            "verification_state": "email_sent",
            "result": result,
        }),
        summary: "Binding email sent".to_string(),
        warnings: Vec::new(),
    }
}

pub fn bind_email_pending_result(identity: &IdentitySummary, email: &str) -> CommandResult {
    CommandResult {
        data: json!({
            "action": "wait_for_bind_email",
            "identity": identity_value(identity),
            "email": normalize_email(email),
            "verification_state": "pending",
        }),
        summary: "Email verification is still pending".to_string(),
        warnings: Vec::new(),
    }
}

pub fn bind_email_completed_result(identity: &IdentitySummary, email: &str) -> CommandResult {
    CommandResult {
        data: json!({
            "action": "bind_email",
            "identity": identity_value(identity),
            "email": normalize_email(email),
            "verification_state": "completed",
        }),
        summary: "Email binding verified successfully".to_string(),
        warnings: Vec::new(),
    }
}

pub fn refresh_token_result(
    identity: &IdentitySummary,
    previous_token_present: bool,
) -> CommandResult {
    CommandResult {
        data: json!({
            "action": "refresh_token",
            "identity": identity_value(identity),
            "previous_token_present": previous_token_present,
            "auth_flow": "did_auth_get_me_without_stored_bearer",
        }),
        summary: format!("JWT refreshed for identity {}", identity.identity_name),
        warnings: Vec::new(),
    }
}

pub fn resolve_result(
    resolve: Option<Value>,
    lookup: Option<Value>,
    public_profile: Option<Value>,
    warnings: Vec<String>,
) -> CommandResult {
    let mut data = Map::new();
    if let Some(resolve) = resolve {
        data.insert("resolve".to_string(), resolve);
    }
    if let Some(lookup) = lookup {
        data.insert("lookup".to_string(), lookup);
    }
    if let Some(public_profile) = public_profile {
        data.insert("public_profile".to_string(), public_profile);
    }
    CommandResult {
        data: Value::Object(data),
        summary: "Identity resolved successfully".to_string(),
        warnings,
    }
}

pub fn profile_self_result(profile: Value) -> CommandResult {
    CommandResult {
        data: json!({
            "subject": "self",
            "profile": profile,
        }),
        summary: "Fetched current identity profile".to_string(),
        warnings: Vec::new(),
    }
}

pub fn profile_public_result(subject: Value, profile: Value) -> CommandResult {
    CommandResult {
        data: json!({
            "subject": subject,
            "profile": profile,
        }),
        summary: "Fetched public profile".to_string(),
        warnings: Vec::new(),
    }
}

pub fn profile_update_result(
    identity: &IdentitySummary,
    changed_fields: Vec<String>,
    profile: Value,
) -> CommandResult {
    CommandResult {
        data: json!({
            "action": "update_profile",
            "identity": identity_value(identity),
            "changed_fields": changed_fields,
            "profile": profile,
        }),
        summary: "Profile updated successfully".to_string(),
        warnings: Vec::new(),
    }
}

pub fn replace_did_result(
    identity: &IdentitySummary,
    old_did: &str,
    did: &str,
    backup_path: &str,
    result: Value,
) -> CommandResult {
    CommandResult {
        data: json!({
            "action": "replace_did",
            "identity": identity_value(identity),
            "old_did": old_did,
            "did": did,
            "backup_path": backup_path,
            "result": result,
        }),
        summary: format!(
            "Identity {} DID replaced successfully",
            identity.identity_name
        ),
        warnings: Vec::new(),
    }
}

fn identity_value(identity: &IdentitySummary) -> Value {
    serde_json::to_value(identity).unwrap_or(Value::Null)
}

fn legacy_rpc_call(call: im_core::compat::identity::RpcCall) -> RpcCall {
    RpcCall {
        endpoint: call.endpoint,
        method: call.method,
        profile: legacy_transport_profile(call.profile),
        params: call.params,
    }
}

fn legacy_rest_call(call: im_core::compat::identity::RestCall) -> RestCall {
    RestCall {
        endpoint: call.endpoint,
        method: call.method,
        profile: legacy_transport_profile(call.profile),
        query: call.query,
        body: call.body,
        authenticated: call.authenticated,
    }
}

fn legacy_transport_profile(profile: im_core::compat::identity::TransportProfile) -> Profile {
    match profile {
        im_core::compat::identity::TransportProfile::BridgeFastPath => Profile::BridgeFastPath,
        im_core::compat::identity::TransportProfile::HealthProbe => Profile::HealthProbe,
        im_core::compat::identity::TransportProfile::AuthRefresh => Profile::AuthRefresh,
        im_core::compat::identity::TransportProfile::RpcDefault => Profile::RpcDefault,
        im_core::compat::identity::TransportProfile::RpcReadHeavy => Profile::RpcReadHeavy,
    }
}

fn sdk_register_params(params: RegisterRpcParams) -> im_core::compat::identity::RegisterRpcParams {
    im_core::compat::identity::RegisterRpcParams {
        did_document: params.did_document,
        handle: params.handle,
        phone: params.phone,
        otp_code: params.otp_code,
        email: params.email,
        invite_code: params.invite_code,
    }
}

fn sdk_recover_params(
    params: RecoverHandleRpcParams,
) -> im_core::compat::identity::RecoverHandleRpcParams {
    im_core::compat::identity::RecoverHandleRpcParams {
        did_document: params.did_document,
        handle: params.handle,
        phone: params.phone,
        otp_code: params.otp_code,
    }
}

fn sdk_replace_did_params(
    params: ReplaceDidRpcParams,
) -> im_core::compat::identity::ReplaceDidRpcParams {
    im_core::compat::identity::ReplaceDidRpcParams {
        new_did_document: params.new_did_document,
        is_public: params.is_public,
        is_agent: params.is_agent,
        role: params.role,
        endpoint_url: params.endpoint_url,
    }
}

fn sdk_update_profile_params(
    params: UpdateProfileParams,
) -> im_core::compat::identity::UpdateProfileParams {
    im_core::compat::identity::UpdateProfileParams {
        display_name: params.display_name,
        bio: params.bio,
        tags_csv: params.tags_csv,
        markdown: params.markdown,
        preserve_markdown: params.preserve_markdown,
    }
}

fn identity_error_from_im(err: im_core::ImError) -> IdentityError {
    match err {
        im_core::ImError::InvalidInput { message, .. } => {
            IdentityError::InvalidInput(format!("invalid input: {message}"))
        }
        err => IdentityError::Internal(err.to_string()),
    }
}
