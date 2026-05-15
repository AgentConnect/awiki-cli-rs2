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
    let did = required_trimmed(did, "did is required")?;
    Ok(rpc_call(
        HANDLE_RPC_ENDPOINT,
        "lookup",
        Profile::RpcDefault,
        json!({ "did": did }),
    ))
}

pub fn build_handle_lookup_by_handle_rpc_call(handle: &str) -> Result<RpcCall, IdentityError> {
    let handle = required_trimmed(handle, "handle is required")?;
    Ok(rpc_call(
        HANDLE_RPC_ENDPOINT,
        "lookup",
        Profile::RpcDefault,
        json!({ "handle": handle }),
    ))
}

pub fn build_profile_resolve_rpc_call(did: &str) -> Result<RpcCall, IdentityError> {
    let did = required_trimmed(did, "did is required")?;
    Ok(rpc_call(
        DID_PROFILE_RPC_ENDPOINT,
        "resolve",
        Profile::RpcDefault,
        json!({ "did": did }),
    ))
}

pub fn build_public_profile_rpc_call(did: &str) -> Result<RpcCall, IdentityError> {
    let did = required_trimmed(did, "did is required")?;
    Ok(rpc_call(
        DID_PROFILE_RPC_ENDPOINT,
        "get_public_profile",
        Profile::RpcReadHeavy,
        json!({ "did": did }),
    ))
}

pub fn build_get_me_profile_rpc_call() -> RpcCall {
    rpc_call(
        DID_PROFILE_RPC_ENDPOINT,
        "get_me",
        Profile::RpcReadHeavy,
        json!({}),
    )
}

pub fn build_refresh_token_rpc_call() -> RpcCall {
    rpc_call(
        DID_AUTH_RPC_ENDPOINT,
        "get_me",
        Profile::AuthRefresh,
        json!({}),
    )
}

pub fn build_update_me_profile_rpc_call(
    params: UpdateProfileParams,
) -> Result<ProfileUpdateCall, IdentityError> {
    let (payload, changed_fields) = build_update_profile_payload(params)?;
    Ok(ProfileUpdateCall {
        call: rpc_call(
            DID_PROFILE_RPC_ENDPOINT,
            "update_me",
            Profile::RpcDefault,
            payload,
        ),
        changed_fields,
    })
}

pub fn build_register_rpc_call(params: RegisterRpcParams) -> Result<RpcCall, IdentityError> {
    let handle = required_trimmed(&params.handle, "handle is required")?;
    let mut payload = Map::new();
    payload.insert("did_document".to_string(), params.did_document);
    payload.insert("handle".to_string(), Value::String(handle));
    if let Some(phone) = params.phone {
        if !phone.trim().is_empty() {
            payload.insert("phone".to_string(), Value::String(normalize_phone(&phone)?));
            payload.insert(
                "otp_code".to_string(),
                Value::String(sanitize_otp(params.otp_code.as_deref().unwrap_or_default())),
            );
        }
    }
    if let Some(email) = params.email {
        let email = normalize_email(&email);
        if !email.is_empty() {
            payload.insert("email".to_string(), Value::String(email));
        }
    }
    if !params.invite_code.is_empty() {
        payload.insert("invite_code".to_string(), Value::String(params.invite_code));
    }
    Ok(rpc_call(
        DID_AUTH_RPC_ENDPOINT,
        "register",
        Profile::RpcDefault,
        Value::Object(payload),
    ))
}

pub fn build_recover_handle_rpc_call(
    params: RecoverHandleRpcParams,
) -> Result<RpcCall, IdentityError> {
    let handle = required_trimmed(&params.handle, "handle is required")?;
    Ok(rpc_call(
        DID_AUTH_RPC_ENDPOINT,
        "recover_handle",
        Profile::RpcDefault,
        json!({
            "did_document": params.did_document,
            "handle": handle,
            "phone": normalize_phone(&params.phone)?,
            "otp_code": sanitize_otp(&params.otp_code),
        }),
    ))
}

pub fn build_replace_did_rpc_call(params: ReplaceDidRpcParams) -> RpcCall {
    let mut payload = Map::new();
    payload.insert("new_did_document".to_string(), params.new_did_document);
    if let Some(value) = params.is_public {
        payload.insert("is_public".to_string(), Value::Bool(value));
    }
    if let Some(value) = params.is_agent {
        payload.insert("is_agent".to_string(), Value::Bool(value));
    }
    if let Some(value) = params.role {
        payload.insert("role".to_string(), nullable_trimmed(value));
    }
    if let Some(value) = params.endpoint_url {
        payload.insert("endpoint_url".to_string(), nullable_trimmed(value));
    }
    rpc_call(
        DID_AUTH_RPC_ENDPOINT,
        "replace_did",
        Profile::RpcDefault,
        Value::Object(payload),
    )
}

pub fn build_send_otp_rpc_call(phone: &str) -> Result<RpcCall, IdentityError> {
    Ok(rpc_call(
        HANDLE_RPC_ENDPOINT,
        "send_otp",
        Profile::RpcDefault,
        json!({ "phone": normalize_phone(phone)? }),
    ))
}

pub fn build_email_send_rest_call(
    email: &str,
    handle: Option<&str>,
    authenticated: bool,
) -> Result<RestCall, IdentityError> {
    let email = required_normalized_email(email)?;
    let mut body = Map::new();
    body.insert("email".to_string(), Value::String(email));
    if let Some(handle) = handle.map(str::trim).filter(|handle| !handle.is_empty()) {
        body.insert("handle".to_string(), Value::String(handle.to_string()));
    }
    Ok(rest_call(
        EMAIL_SEND_ENDPOINT,
        "POST",
        Value::Object(body),
        BTreeMap::new(),
        authenticated,
    ))
}

pub fn build_email_status_rest_call(
    email: &str,
    handle: Option<&str>,
    authenticated: bool,
) -> Result<RestCall, IdentityError> {
    let mut query = BTreeMap::new();
    query.insert("email".to_string(), required_normalized_email(email)?);
    if let Some(handle) = handle.map(str::trim).filter(|handle| !handle.is_empty()) {
        query.insert("handle".to_string(), handle.to_string());
    }
    Ok(rest_call(
        EMAIL_STATUS_ENDPOINT,
        "GET",
        Value::Null,
        query,
        authenticated,
    ))
}

pub fn build_phone_bind_send_rest_call(phone: &str) -> Result<RestCall, IdentityError> {
    Ok(rest_call(
        PHONE_BIND_SEND_ENDPOINT,
        "POST",
        json!({ "phone": normalize_phone(phone)? }),
        BTreeMap::new(),
        true,
    ))
}

pub fn build_phone_bind_verify_rest_call(
    phone: &str,
    code: &str,
) -> Result<RestCall, IdentityError> {
    Ok(rest_call(
        PHONE_BIND_VERIFY_ENDPOINT,
        "POST",
        json!({ "phone": normalize_phone(phone)?, "code": sanitize_otp(code) }),
        BTreeMap::new(),
        true,
    ))
}

pub fn build_update_profile_payload(
    params: UpdateProfileParams,
) -> Result<(Value, Vec<String>), IdentityError> {
    let mut payload = Map::new();
    let mut changed_fields = Vec::new();
    if !params.display_name.trim().is_empty() {
        payload.insert(
            "nick_name".to_string(),
            Value::String(params.display_name.trim().to_string()),
        );
        changed_fields.push("display_name".to_string());
    }
    if !params.bio.trim().is_empty() {
        payload.insert(
            "bio".to_string(),
            Value::String(params.bio.trim().to_string()),
        );
        changed_fields.push("bio".to_string());
    }
    if !params.tags_csv.trim().is_empty() {
        payload.insert("tags".to_string(), json!(split_csv(&params.tags_csv)));
        changed_fields.push("tags".to_string());
    }
    let markdown = if params.preserve_markdown {
        params.markdown.clone()
    } else {
        params.markdown.trim().to_string()
    };
    if !markdown.trim().is_empty() {
        payload.insert("profile_md".to_string(), Value::String(markdown));
        changed_fields.push("profile_md".to_string());
    }
    if payload.is_empty() {
        return Err(invalid_input("no profile fields were provided"));
    }
    Ok((Value::Object(payload), changed_fields))
}

pub fn normalize_phone(phone: &str) -> Result<String, IdentityError> {
    let phone = phone.trim();
    if is_international_phone(phone) {
        return Ok(phone.to_string());
    }
    if is_china_local_phone(phone) {
        return Ok(format!("+86{phone}"));
    }
    Err(invalid_input(format!("invalid phone number {phone:?}")))
}

pub fn sanitize_otp(code: &str) -> String {
    code.split_whitespace().collect()
}

pub fn split_csv(raw: &str) -> Vec<String> {
    raw.split(',')
        .filter_map(|item| {
            let item = item.trim();
            (!item.is_empty()).then(|| item.to_string())
        })
        .collect()
}

pub fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
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

fn rpc_call(
    endpoint: &'static str,
    method: &'static str,
    profile: Profile,
    params: Value,
) -> RpcCall {
    RpcCall {
        endpoint,
        method,
        profile,
        params,
    }
}

fn rest_call(
    endpoint: &'static str,
    method: &'static str,
    body: Value,
    query: BTreeMap<String, String>,
    authenticated: bool,
) -> RestCall {
    RestCall {
        endpoint,
        method,
        profile: Profile::RpcDefault,
        query,
        body,
        authenticated,
    }
}

fn required_trimmed(value: &str, message: &str) -> Result<String, IdentityError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(invalid_input(message));
    }
    Ok(value.to_string())
}

fn required_normalized_email(email: &str) -> Result<String, IdentityError> {
    let email = normalize_email(email);
    if email.is_empty() {
        return Err(invalid_input("email is required"));
    }
    Ok(email)
}

fn invalid_input(message: impl Into<String>) -> IdentityError {
    IdentityError::InvalidInput(format!("invalid input: {}", message.into()))
}

fn nullable_trimmed(value: String) -> Value {
    let value = value.trim();
    if value.is_empty() {
        Value::Null
    } else {
        Value::String(value.to_string())
    }
}

fn is_international_phone(phone: &str) -> bool {
    let Some(rest) = phone.strip_prefix('+') else {
        return false;
    };
    let len = rest.len();
    (7..=17).contains(&len) && rest.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_china_local_phone(phone: &str) -> bool {
    let bytes = phone.as_bytes();
    bytes.len() == 11
        && bytes[0] == b'1'
        && (b'3'..=b'9').contains(&bytes[1])
        && bytes.iter().all(u8::is_ascii_digit)
}

fn identity_value(identity: &IdentitySummary) -> Value {
    serde_json::to_value(identity).unwrap_or(Value::Null)
}
