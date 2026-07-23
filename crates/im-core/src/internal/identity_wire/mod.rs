pub(crate) mod bind;
pub(crate) mod device_join;
pub(crate) mod device_revoke;
pub(crate) mod directory;
pub(crate) mod document;
pub(crate) mod profile;
pub(crate) mod registration;
pub(crate) mod relationships;
pub(crate) mod replace_did;
pub(crate) mod update_document;

use serde_json::{json, Value};
use std::collections::BTreeMap;

pub(crate) const JSON_RPC_VERSION: &str = "2.0";
pub(crate) const JSON_RPC_ID: &str = "req-1";

pub(crate) const DID_AUTH_RPC_ENDPOINT: &str = "/user-service/did-auth/rpc";
pub(crate) const HANDLE_RPC_ENDPOINT: &str = "/user-service/handle/rpc";
pub(crate) const DID_PROFILE_RPC_ENDPOINT: &str = "/user-service/did/profile/rpc";
pub(crate) const DID_RELATIONSHIPS_RPC_ENDPOINT: &str = "/user-service/did/relationships/rpc";
pub(crate) const EMAIL_SEND_ENDPOINT: &str = "/user-service/auth/email-send";
pub(crate) const EMAIL_STATUS_ENDPOINT: &str = "/user-service/auth/email-status";
pub(crate) const SMS_CODES_ENDPOINT: &str = "/user-service/auth/sms-codes";
pub(crate) const ACCOUNT_VERIFICATION_EXCHANGE_ENDPOINT: &str =
    "/user-service/auth/account-verification/exchange";
pub(crate) const PHONE_BIND_SEND_ENDPOINT: &str = "/user-service/auth/phone-bind-send";
pub(crate) const PHONE_BIND_VERIFY_ENDPOINT: &str = "/user-service/auth/phone-bind-verify";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TransportProfile {
    BridgeFastPath,
    HealthProbe,
    AuthRefresh,
    RpcDefault,
    RpcReadHeavy,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RpcCall {
    pub endpoint: &'static str,
    pub method: &'static str,
    pub profile: TransportProfile,
    pub params: Value,
}

impl RpcCall {
    pub fn payload(&self) -> Value {
        json!({
            "jsonrpc": JSON_RPC_VERSION,
            "id": JSON_RPC_ID,
            "method": self.method,
            "params": self.params.clone(),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RestCall {
    pub endpoint: &'static str,
    pub method: &'static str,
    pub profile: TransportProfile,
    pub query: BTreeMap<String, String>,
    pub body: Value,
    pub authenticated: bool,
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
pub struct ReplaceDidRpcParams {
    pub new_did_document: Value,
    pub is_public: Option<bool>,
    pub is_agent: Option<bool>,
    pub role: Option<String>,
    pub endpoint_url: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct UpdateDocumentRpcParams {
    pub did_document: Value,
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
    pub avatar_uri: String,
    pub avatar_url: String,
    pub preserve_markdown: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProfileUpdateCall {
    pub call: RpcCall,
    pub changed_fields: Vec<String>,
}

pub(crate) fn rpc_call(
    endpoint: &'static str,
    method: &'static str,
    profile: TransportProfile,
    params: Value,
) -> RpcCall {
    RpcCall {
        endpoint,
        method,
        profile,
        params,
    }
}

pub(crate) fn rest_call(
    endpoint: &'static str,
    method: &'static str,
    body: Value,
    query: BTreeMap<String, String>,
    authenticated: bool,
) -> RestCall {
    RestCall {
        endpoint,
        method,
        profile: TransportProfile::RpcDefault,
        query,
        body,
        authenticated,
    }
}

pub(crate) fn required_trimmed(value: &str, field: &str) -> crate::ImResult<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some(field.to_string()),
            format!("{field} is required"),
        ));
    }
    Ok(value.to_string())
}

pub(crate) fn nullable_trimmed(value: String) -> Value {
    let value = value.trim();
    if value.is_empty() {
        Value::Null
    } else {
        Value::String(value.to_string())
    }
}

pub(crate) fn normalize_phone(phone: &str) -> crate::ImResult<String> {
    let phone = phone.trim();
    if is_international_phone(phone) {
        return Ok(phone.to_string());
    }
    if is_china_local_phone(phone) {
        return Ok(format!("+86{phone}"));
    }
    Err(crate::ImError::invalid_input(
        Some("phone".to_string()),
        format!("invalid phone number {phone:?}"),
    ))
}

pub(crate) fn sanitize_otp(code: &str) -> String {
    code.split_whitespace().collect()
}

pub(crate) fn split_csv(raw: &str) -> Vec<String> {
    raw.split(',')
        .filter_map(|item| {
            let item = item.trim();
            (!item.is_empty()).then(|| item.to_string())
        })
        .collect()
}

pub(crate) fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

pub(crate) fn required_normalized_email(email: &str) -> crate::ImResult<String> {
    let email = normalize_email(email);
    if email.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("email".to_string()),
            "email is required",
        ));
    }
    Ok(email)
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
