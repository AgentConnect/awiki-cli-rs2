use serde_json::Value;
use std::collections::BTreeSet;
use time::OffsetDateTime;

const EXPECTED_ISSUER: &str = "user-service";
const EXPECTED_PURPOSE: &str = "awiki.device.access.v1";
const EXPECTED_USER_SERVICE_AUDIENCE: &str = "awiki-user-service";
const EXPECTED_MESSAGE_SERVICE_AUDIENCE: &str = "awiki-message-service";
const CLOCK_SKEW_SECONDS: i64 = 30;

#[derive(Debug, Clone)]
pub(crate) struct ExpectedDeviceAccess<'a> {
    pub(crate) did: &'a str,
    pub(crate) user_id: &'a str,
    pub(crate) device_id: &'a str,
    pub(crate) key_id: &'a str,
    pub(crate) auth_generation: u64,
    pub(crate) role: crate::internal::identity_device_state::DeviceAuthorizationRole,
    pub(crate) management_ready: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeviceAccessTokenFreshness {
    Fresh,
    Expired,
}

pub(crate) fn validate_device_access_token(
    token: &str,
    expected: &ExpectedDeviceAccess<'_>,
) -> crate::ImResult<()> {
    match validate_device_access_token_binding(token, expected)? {
        DeviceAccessTokenFreshness::Fresh => Ok(()),
        DeviceAccessTokenFreshness::Expired => Err(crate::ImError::SessionExpired),
    }
}

pub(crate) fn validate_device_access_token_binding(
    token: &str,
    expected: &ExpectedDeviceAccess<'_>,
) -> crate::ImResult<DeviceAccessTokenFreshness> {
    let claims = required_payload(token)?;
    require_string(&claims, "iss", EXPECTED_ISSUER)?;
    require_string(&claims, "type", "access")?;
    require_string(&claims, "purpose", EXPECTED_PURPOSE)?;
    require_string(&claims, "sub", expected.did)?;
    require_string(&claims, "did", expected.did)?;
    require_string(&claims, "user_id", expected.user_id)?;
    require_string(&claims, "device_id", expected.device_id)?;
    require_string(&claims, "key_id", expected.key_id)?;
    if expected.key_id == format!("{}#key-1", expected.did) || claims.get("profile").is_some() {
        return Err(crate::ImError::PermissionDenied);
    }
    let generation = claims
        .get("auth_generation")
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
        .ok_or(crate::ImError::PermissionDenied)?;
    if generation != expected.auth_generation {
        return Err(crate::ImError::PermissionDenied);
    }
    validate_audience(&claims)?;
    let freshness = validate_times(&claims)?;
    validate_scopes(&claims, expected)?;
    Ok(freshness)
}

pub(crate) fn validate_legacy_access_token(token: &str, did: &str) -> crate::ImResult<()> {
    let claims = required_payload(token)?;
    require_string(&claims, "iss", EXPECTED_ISSUER)?;
    require_string(&claims, "type", "access")?;
    require_string(&claims, "sub", did)?;
    if has_any_device_claim(&claims) {
        return Err(crate::ImError::PermissionDenied);
    }
    let exp = claims
        .get("exp")
        .and_then(numeric_date)
        .ok_or(crate::ImError::PermissionDenied)?;
    if exp <= OffsetDateTime::now_utc().unix_timestamp() - CLOCK_SKEW_SECONDS {
        return Err(crate::ImError::SessionExpired);
    }
    Ok(())
}

fn required_payload(token: &str) -> crate::ImResult<Value> {
    let token = token.trim();
    if token.is_empty() {
        return Err(crate::ImError::AuthRequired);
    }
    crate::internal::auth::state::decode_jwt_payload(token)
        .filter(Value::is_object)
        .ok_or(crate::ImError::PermissionDenied)
}

fn require_string(claims: &Value, field: &str, expected: &str) -> crate::ImResult<()> {
    let actual = claims
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(crate::ImError::PermissionDenied)?;
    if actual != expected {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn validate_audience(claims: &Value) -> crate::ImResult<()> {
    let audiences = match claims.get("aud") {
        Some(Value::String(value)) => BTreeSet::from([value.as_str()]),
        Some(Value::Array(values)) => values.iter().filter_map(Value::as_str).collect(),
        _ => BTreeSet::new(),
    };
    if !audiences.contains(EXPECTED_USER_SERVICE_AUDIENCE)
        || !audiences.contains(EXPECTED_MESSAGE_SERVICE_AUDIENCE)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn validate_times(claims: &Value) -> crate::ImResult<DeviceAccessTokenFreshness> {
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let iat = claims
        .get("iat")
        .and_then(numeric_date)
        .ok_or(crate::ImError::PermissionDenied)?;
    let nbf = claims
        .get("nbf")
        .and_then(numeric_date)
        .ok_or(crate::ImError::PermissionDenied)?;
    let exp = claims
        .get("exp")
        .and_then(numeric_date)
        .ok_or(crate::ImError::PermissionDenied)?;
    if claims
        .get("jti")
        .and_then(Value::as_str)
        .map(str::trim)
        .is_none_or(str::is_empty)
        || nbf != iat
        || iat > now + CLOCK_SKEW_SECONDS
        || nbf > now + CLOCK_SKEW_SECONDS
        || exp <= iat
    {
        return Err(crate::ImError::PermissionDenied);
    }
    if exp <= now - CLOCK_SKEW_SECONDS {
        return Ok(DeviceAccessTokenFreshness::Expired);
    }
    Ok(DeviceAccessTokenFreshness::Fresh)
}

fn validate_scopes(claims: &Value, expected: &ExpectedDeviceAccess<'_>) -> crate::ImResult<()> {
    let values = claims
        .get("scopes")
        .and_then(Value::as_array)
        .ok_or(crate::ImError::PermissionDenied)?;
    let mut actual = BTreeSet::new();
    for value in values {
        let scope = value
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or(crate::ImError::PermissionDenied)?;
        if !matches!(
            scope,
            "device:read" | "message:connect" | "device:manage" | "device:root-import-complete"
        ) || !actual.insert(scope)
        {
            return Err(crate::ImError::PermissionDenied);
        }
    }
    let expected_scopes = match (expected.role, expected.management_ready) {
        (crate::internal::identity_device_state::DeviceAuthorizationRole::Admin, true) => {
            BTreeSet::from(["device:manage", "device:read", "message:connect"])
        }
        (
            crate::internal::identity_device_state::DeviceAuthorizationRole::Member
            | crate::internal::identity_device_state::DeviceAuthorizationRole::Admin,
            false,
        ) => BTreeSet::from([
            "device:read",
            "device:root-import-complete",
            "message:connect",
        ]),
        _ => return Err(crate::ImError::PermissionDenied),
    };
    if actual != expected_scopes {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn has_any_device_claim(claims: &Value) -> bool {
    [
        "purpose",
        "did",
        "user_id",
        "device_id",
        "key_id",
        "auth_generation",
        "scopes",
    ]
    .iter()
    .any(|field| claims.get(*field).is_some())
}

fn numeric_date(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
}

#[cfg(test)]
mod tests {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use serde_json::json;

    use super::*;

    const DID: &str = "did:wba:example.test:alice";
    const USER_ID: &str = "user-1";
    const DEVICE_ID: &str = "device-1";
    const KEY_ID: &str = "did:wba:example.test:alice#device-1-sign";

    fn jwt(mut claims: Value) -> String {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let object = claims.as_object_mut().unwrap();
        object.insert("iat".to_owned(), json!(now));
        object.insert("nbf".to_owned(), json!(now));
        object.insert("exp".to_owned(), json!(now + 300));
        object.insert("jti".to_owned(), json!("token-1"));
        format!(
            "e30.{}.signature",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
        )
    }

    fn admin_claims() -> Value {
        json!({
            "iss": EXPECTED_ISSUER,
            "aud": ["awiki-user-service", "awiki-message-service"],
            "sub": DID,
            "type": "access",
            "purpose": EXPECTED_PURPOSE,
            "did": DID,
            "user_id": USER_ID,
            "device_id": DEVICE_ID,
            "key_id": KEY_ID,
            "auth_generation": 1,
            "scopes": ["device:manage", "device:read", "message:connect"]
        })
    }

    fn expected_admin<'a>() -> ExpectedDeviceAccess<'a> {
        ExpectedDeviceAccess {
            did: DID,
            user_id: USER_ID,
            device_id: DEVICE_ID,
            key_id: KEY_ID,
            auth_generation: 1,
            role: crate::internal::identity_device_state::DeviceAuthorizationRole::Admin,
            management_ready: true,
        }
    }

    #[test]
    fn accepts_exact_v1_device_access_principal() {
        validate_device_access_token(&jwt(admin_claims()), &expected_admin()).unwrap();
    }

    #[test]
    fn rejects_custom_profile_and_root_key_principals() {
        let mut profile = admin_claims();
        profile["profile"] = json!("awiki-device-token-v1");
        assert_eq!(
            validate_device_access_token(&jwt(profile), &expected_admin()),
            Err(crate::ImError::PermissionDenied)
        );

        let mut root = admin_claims();
        let root_key = format!("{DID}#key-1");
        root["key_id"] = json!(root_key.clone());
        let mut expected = expected_admin();
        expected.key_id = &root_key;
        assert_eq!(
            validate_device_access_token(&jwt(root), &expected),
            Err(crate::ImError::PermissionDenied)
        );
    }

    #[test]
    fn device_access_requires_both_service_audiences() {
        let mut missing_message_service = admin_claims();
        missing_message_service["aud"] = json!([EXPECTED_USER_SERVICE_AUDIENCE]);

        assert_eq!(
            validate_device_access_token(&jwt(missing_message_service), &expected_admin(),),
            Err(crate::ImError::PermissionDenied)
        );
    }

    #[test]
    fn expired_device_access_keeps_exact_binding_but_requires_refresh() {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let mut claims = admin_claims();
        let object = claims.as_object_mut().unwrap();
        object.insert("iat".to_owned(), json!(now - 600));
        object.insert("nbf".to_owned(), json!(now - 600));
        object.insert("exp".to_owned(), json!(now - 60));
        object.insert("jti".to_owned(), json!("expired-token"));
        let token = format!(
            "e30.{}.signature",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
        );

        assert_eq!(
            validate_device_access_token_binding(&token, &expected_admin()).unwrap(),
            DeviceAccessTokenFreshness::Expired
        );
        assert_eq!(
            validate_device_access_token(&token, &expected_admin()),
            Err(crate::ImError::SessionExpired)
        );
    }

    #[test]
    fn legacy_access_token_rejects_device_candidate_claims() {
        let legacy = jwt(json!({
            "iss": EXPECTED_ISSUER,
            "sub": DID,
            "type": "access"
        }));
        validate_legacy_access_token(&legacy, DID).unwrap();

        let candidate = jwt(json!({
            "iss": EXPECTED_ISSUER,
            "sub": DID,
            "type": "access",
            "device_id": DEVICE_ID
        }));
        assert_eq!(
            validate_legacy_access_token(&candidate, DID),
            Err(crate::ImError::PermissionDenied)
        );
    }
}
