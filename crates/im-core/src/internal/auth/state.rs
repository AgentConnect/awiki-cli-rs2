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

#[derive(Clone, Default)]
pub(crate) struct AuthStateSnapshot {
    pub(crate) has_token: bool,
    pub(crate) has_valid_token: bool,
    pub(crate) token_expired: bool,
    pub(crate) needs_refresh: bool,
    pub(crate) bearer_token: Option<String>,
    // Temporary test-only projection for pre-V1 flows removed in S6.
    // Production auth state neither reads nor persists a refresh token.
    #[cfg(test)]
    pub(crate) refresh_token: Option<String>,
    pub(crate) subject: Option<String>,
    pub(crate) issued_at: Option<String>,
    pub(crate) expires_at: Option<String>,
}

impl std::fmt::Debug for AuthStateSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthStateSnapshot")
            .field("has_token", &self.has_token)
            .field("has_valid_token", &self.has_valid_token)
            .field("token_expired", &self.token_expired)
            .field("needs_refresh", &self.needs_refresh)
            .field(
                "bearer_token",
                &self.bearer_token.as_ref().map(|_| "<redacted>"),
            )
            .field("subject", &self.subject)
            .field("issued_at", &self.issued_at)
            .field("expires_at", &self.expires_at)
            .finish()
    }
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

    let bytes = auth_state_json_for_token(token)?;
    let temporary = parent.join(format!(
        ".auth-state-{}.tmp",
        crate::internal::wire::common::generate_operation_id()
    ));
    let write_result = (|| -> crate::ImResult<()> {
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        crate::internal::atomic_file::replace(&temporary, path)?;
        #[cfg(unix)]
        std::fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    write_result?;
    Ok(())
}

pub(crate) fn auth_state_json_for_token(token: &str) -> crate::ImResult<Vec<u8>> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("access_token".to_owned()),
            "access_token is required",
        ));
    }
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
    let expires_at = metadata.expires_at;
    if let Some(expires_at) = expires_at.and_then(format_rfc3339) {
        body.insert("expires_at".to_string(), Value::String(expires_at));
    }

    serde_json::to_vec_pretty(&Value::Object(body)).map_err(|err| crate::ImError::Serialization {
        detail: err.to_string(),
    })
}

pub(crate) fn parse_auth_state(raw: &[u8]) -> crate::ImResult<AuthStateSnapshot> {
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
        #[cfg(test)]
        refresh_token: None,
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

pub(crate) fn decode_jwt_payload(token: &str) -> Option<Value> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_token_rotation_has_no_refresh_token_state_or_debug_leak() {
        let rotated = auth_state_json_for_token("access-secret-two").unwrap();
        let rotated = parse_auth_state(&rotated).unwrap();
        assert_eq!(rotated.bearer_token.as_deref(), Some("access-secret-two"));
        let debug = format!("{rotated:?}");
        assert!(!debug.contains("access-secret-two"));
        assert!(debug.contains("<redacted>"));
    }

    #[test]
    fn legacy_access_only_auth_state_remains_compatible() {
        let state = parse_auth_state(br#"{"jwt_token":"legacy-token"}"#).unwrap();
        assert_eq!(state.bearer_token.as_deref(), Some("legacy-token"));
    }
}
