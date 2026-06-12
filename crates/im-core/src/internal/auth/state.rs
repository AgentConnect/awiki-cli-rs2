use base64::{
    engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD},
    Engine as _,
};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use time::{format_description::well_known::Rfc3339, Duration, OffsetDateTime};

const EXPIRY_LEEWAY_SECONDS: i64 = 30;
const REFRESH_WINDOW_SECONDS: i64 = 300;

#[derive(Debug, Clone, Default)]
pub(crate) struct AuthStateSnapshot {
    pub(crate) has_token: bool,
    pub(crate) has_valid_token: bool,
    pub(crate) token_expired: bool,
    pub(crate) needs_refresh: bool,
    pub(crate) bearer_token: Option<String>,
    pub(crate) subject: Option<String>,
    pub(crate) issued_at: Option<String>,
    pub(crate) expires_at: Option<String>,
}

#[derive(Deserialize)]
struct AuthStateFile {
    #[serde(default)]
    jwt_token: Option<String>,
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    issued_at: Option<String>,
    #[serde(default)]
    expires_at: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct JwtMetadata {
    subject: Option<String>,
    issued_at: Option<OffsetDateTime>,
    expires_at: Option<OffsetDateTime>,
}

pub(crate) fn read_auth_state(path: &Path) -> crate::ImResult<AuthStateSnapshot> {
    let raw = match std::fs::read(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(AuthStateSnapshot::default());
        }
        Err(err) => {
            return Err(crate::ImError::CredentialFileUnreadable {
                path_kind: "auth_state".to_string(),
                detail: err.to_string(),
            });
        }
    };
    parse_auth_state(&raw)
}

pub(crate) async fn read_auth_state_async(path: PathBuf) -> crate::ImResult<AuthStateSnapshot> {
    let raw = match tokio::fs::read(path).await {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(AuthStateSnapshot::default());
        }
        Err(err) => {
            return Err(crate::ImError::CredentialFileUnreadable {
                path_kind: "auth_state".to_string(),
                detail: err.to_string(),
            });
        }
    };
    parse_auth_state(&raw)
}

pub(crate) fn read_jwt_token(path: &Path) -> crate::ImResult<Option<String>> {
    let snapshot = read_auth_state(path)?;
    if snapshot.has_valid_token {
        Ok(snapshot.bearer_token)
    } else {
        Ok(None)
    }
}

pub(crate) fn persist_jwt_token(path: &Path, token: &str) -> crate::ImResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| crate::ImError::PathUnavailable {
            path_kind: "auth_state".to_string(),
            detail: "auth state path has no parent".to_string(),
        })?;
    std::fs::create_dir_all(parent)?;

    let trimmed = token.trim();
    let metadata = jwt_metadata(trimmed);
    let mut body = Map::from_iter([
        ("jwt_token".to_string(), Value::String(trimmed.to_string())),
        (
            "token_type".to_string(),
            Value::String("Bearer".to_string()),
        ),
    ]);
    if let Some(subject) = metadata.subject {
        body.insert("subject".to_string(), Value::String(subject));
    }
    if let Some(issued_at) = metadata.issued_at.and_then(format_rfc3339) {
        body.insert("issued_at".to_string(), Value::String(issued_at));
    }
    if let Some(expires_at) = metadata.expires_at.and_then(format_rfc3339) {
        body.insert("expires_at".to_string(), Value::String(expires_at));
    }

    let bytes = serde_json::to_vec_pretty(&Value::Object(body)).map_err(|err| {
        crate::ImError::Serialization {
            detail: err.to_string(),
        }
    })?;
    std::fs::write(path, bytes)?;
    Ok(())
}

fn parse_auth_state(raw: &[u8]) -> crate::ImResult<AuthStateSnapshot> {
    let parsed: AuthStateFile =
        serde_json::from_slice(raw).map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })?;
    let bearer_token = first_non_empty_token([
        parsed.jwt_token.as_deref(),
        parsed.token.as_deref(),
        parsed.access_token.as_deref(),
    ]);
    let Some(token) = bearer_token else {
        return Ok(AuthStateSnapshot::default());
    };

    let metadata = jwt_metadata(&token);
    let expires_at = parsed
        .expires_at
        .as_deref()
        .and_then(parse_rfc3339)
        .or(metadata.expires_at);
    let issued_at = parsed
        .issued_at
        .as_deref()
        .and_then(parse_rfc3339)
        .or(metadata.issued_at);
    let now = OffsetDateTime::now_utc();
    let token_expired = expires_at
        .map(|value| value <= now + Duration::seconds(EXPIRY_LEEWAY_SECONDS))
        .unwrap_or(false);
    let expires_soon = expires_at
        .map(|value| value <= now + Duration::seconds(REFRESH_WINDOW_SECONDS))
        .unwrap_or(false);

    Ok(AuthStateSnapshot {
        has_token: true,
        has_valid_token: !token_expired,
        token_expired,
        needs_refresh: token_expired || expires_soon,
        bearer_token: Some(token),
        subject: parsed
            .subject
            .filter(|subject| !subject.trim().is_empty())
            .or(metadata.subject),
        issued_at: issued_at.and_then(format_rfc3339),
        expires_at: expires_at.and_then(format_rfc3339),
    })
}

fn first_non_empty_token<'a>(tokens: impl IntoIterator<Item = Option<&'a str>>) -> Option<String> {
    tokens
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|token| !token.is_empty())
        .map(ToOwned::to_owned)
}

fn jwt_metadata(token: &str) -> JwtMetadata {
    let Some(payload) = decode_jwt_payload(token) else {
        return JwtMetadata::default();
    };
    JwtMetadata {
        subject: payload
            .get("sub")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|subject| !subject.is_empty())
            .map(ToOwned::to_owned),
        issued_at: numeric_date(&payload, "iat").and_then(offset_from_unix_timestamp),
        expires_at: numeric_date(&payload, "exp").and_then(offset_from_unix_timestamp),
    }
}

fn decode_jwt_payload(token: &str) -> Option<Value> {
    let payload = token.split('.').nth(1)?;
    let decoded = URL_SAFE_NO_PAD
        .decode(payload)
        .or_else(|_| URL_SAFE.decode(payload))
        .ok()?;
    serde_json::from_slice(&decoded).ok()
}

fn numeric_date(payload: &Value, key: &str) -> Option<i64> {
    let value = payload.get(key)?;
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
        .or_else(|| value.as_str()?.parse::<i64>().ok())
}

fn offset_from_unix_timestamp(value: i64) -> Option<OffsetDateTime> {
    OffsetDateTime::from_unix_timestamp(value).ok()
}

fn parse_rfc3339(value: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(value.trim(), &Rfc3339).ok()
}

fn format_rfc3339(value: OffsetDateTime) -> Option<String> {
    value.format(&Rfc3339).ok()
}
