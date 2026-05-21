use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub(crate) fn build_email_send_rest_call(
    email: &str,
    handle: Option<&str>,
    authenticated: bool,
) -> crate::ImResult<super::RestCall> {
    let email = super::required_normalized_email(email)?;
    let mut body = Map::new();
    body.insert("email".to_string(), Value::String(email));
    if let Some(handle) = handle.map(str::trim).filter(|handle| !handle.is_empty()) {
        body.insert("handle".to_string(), Value::String(handle.to_string()));
    }
    Ok(super::rest_call(
        super::EMAIL_SEND_ENDPOINT,
        "POST",
        Value::Object(body),
        BTreeMap::new(),
        authenticated,
    ))
}

pub(crate) fn build_email_status_rest_call(
    email: &str,
    handle: Option<&str>,
    authenticated: bool,
) -> crate::ImResult<super::RestCall> {
    let mut query = BTreeMap::new();
    query.insert(
        "email".to_string(),
        super::required_normalized_email(email)?,
    );
    if let Some(handle) = handle.map(str::trim).filter(|handle| !handle.is_empty()) {
        query.insert("handle".to_string(), handle.to_string());
    }
    Ok(super::rest_call(
        super::EMAIL_STATUS_ENDPOINT,
        "GET",
        Value::Null,
        query,
        authenticated,
    ))
}

pub(crate) fn build_phone_bind_send_rest_call(phone: &str) -> crate::ImResult<super::RestCall> {
    Ok(super::rest_call(
        super::PHONE_BIND_SEND_ENDPOINT,
        "POST",
        json!({ "phone": super::normalize_phone(phone)? }),
        BTreeMap::new(),
        true,
    ))
}

pub(crate) fn build_phone_bind_verify_rest_call(
    phone: &str,
    code: &str,
) -> crate::ImResult<super::RestCall> {
    Ok(super::rest_call(
        super::PHONE_BIND_VERIFY_ENDPOINT,
        "POST",
        json!({ "phone": super::normalize_phone(phone)?, "code": super::sanitize_otp(code) }),
        BTreeMap::new(),
        true,
    ))
}
