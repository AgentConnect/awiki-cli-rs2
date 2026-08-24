use std::collections::{BTreeMap, HashMap};
use std::net::IpAddr;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use super::dto::{
    AttemptCredential, ExternalHttpAuthAttempt, ExternalHttpAuthDecision, ExternalHttpHeader,
    ExternalHttpRequest, ExternalHttpResponse, PreparedRequest, TokenKey,
};

const FIXED_COMPONENTS: [&str; 3] = ["@method", "@target-uri", "@authority"];

#[derive(Default)]
pub(crate) struct ExternalHttpAuthState {
    tokens: Mutex<HashMap<TokenKey, StoredToken>>,
}

struct StoredToken {
    value: Zeroizing<String>,
    fingerprint: [u8; 32],
    expires_at: Option<SystemTime>,
}

pub struct ExternalHttpAuthService<'a> {
    client: &'a crate::core::ImClient,
}

impl<'a> ExternalHttpAuthService<'a> {
    pub(crate) fn new(client: &'a crate::core::ImClient) -> Self {
        Self { client }
    }

    pub fn prepare(
        &self,
        request: ExternalHttpRequest,
    ) -> crate::ImResult<ExternalHttpAuthAttempt> {
        let prepared = self.prepare_request(request)?;
        let signing_key_id = self
            .client
            .runtime()
            .key_provider
            .request_signing_key_id()?;
        let token_key = TokenKey {
            identity_id: self.client.current_identity().id.as_str().to_owned(),
            did: self.client.did().as_str().to_owned(),
            signing_key_id,
            origin: prepared.origin.clone(),
        };

        if let Some((token, fingerprint)) = self.cached_token(&token_key)? {
            return Ok(ExternalHttpAuthAttempt {
                header_patch: vec![ExternalHttpHeader::new(
                    "Authorization",
                    format!("Bearer {}", token.as_str()),
                )?],
                request: prepared,
                token_key,
                credential: AttemptCredential::Bearer { fingerprint },
                retry_count: 0,
            });
        }

        self.signature_attempt(prepared, token_key, None, 0)
    }

    pub async fn prepare_async(
        &self,
        request: ExternalHttpRequest,
    ) -> crate::ImResult<ExternalHttpAuthAttempt> {
        let prepared = self.prepare_request(request)?;
        let signing_key_id = self
            .client
            .runtime()
            .key_provider
            .request_signing_key_id()?;
        let token_key = TokenKey {
            identity_id: self.client.current_identity().id.as_str().to_owned(),
            did: self.client.did().as_str().to_owned(),
            signing_key_id,
            origin: prepared.origin.clone(),
        };

        if let Some((token, fingerprint)) = self.cached_token(&token_key)? {
            return Ok(ExternalHttpAuthAttempt {
                header_patch: vec![ExternalHttpHeader::new(
                    "Authorization",
                    format!("Bearer {}", token.as_str()),
                )?],
                request: prepared,
                token_key,
                credential: AttemptCredential::Bearer { fingerprint },
                retry_count: 0,
            });
        }

        self.signature_attempt_async(prepared, token_key, None, 0)
            .await
    }

    pub fn handle_response(
        &self,
        attempt: ExternalHttpAuthAttempt,
        response: ExternalHttpResponse,
    ) -> crate::ImResult<ExternalHttpAuthDecision> {
        match self.response_plan(attempt, response)? {
            ResponsePlan::Complete => Ok(ExternalHttpAuthDecision::Complete),
            ResponsePlan::Retry {
                request,
                token_key,
                nonce,
            } => self
                .signature_attempt(request, token_key, nonce.as_deref(), 1)
                .map(ExternalHttpAuthDecision::Retry),
        }
    }

    pub async fn handle_response_async(
        &self,
        attempt: ExternalHttpAuthAttempt,
        response: ExternalHttpResponse,
    ) -> crate::ImResult<ExternalHttpAuthDecision> {
        match self.response_plan(attempt, response)? {
            ResponsePlan::Complete => Ok(ExternalHttpAuthDecision::Complete),
            ResponsePlan::Retry {
                request,
                token_key,
                nonce,
            } => self
                .signature_attempt_async(request, token_key, nonce.as_deref(), 1)
                .await
                .map(ExternalHttpAuthDecision::Retry),
        }
    }

    /// Clears the process-local external origin Token Store.
    pub fn clear_cached_tokens(&self) -> crate::ImResult<()> {
        self.token_store()?.clear();
        Ok(())
    }

    fn prepare_request(&self, request: ExternalHttpRequest) -> crate::ImResult<PreparedRequest> {
        let url = reqwest::Url::parse(&request.url).map_err(|_| {
            crate::ImError::invalid_input(
                Some("external_http_request.url".to_owned()),
                "external HTTP URL must be absolute",
            )
        })?;
        if url.fragment().is_some() || !url.username().is_empty() || url.password().is_some() {
            return Err(crate::ImError::invalid_input(
                Some("external_http_request.url".to_owned()),
                "external HTTP URL must not contain credentials or a fragment",
            ));
        }
        if url.scheme() != "https"
            && !(url.scheme() == "http"
                && self
                    .client
                    .core_inner()
                    .external_http_allow_insecure_loopback_for_testing()
                && is_literal_loopback(&url))
        {
            return Err(crate::ImError::invalid_input(
                Some("external_http_request.url".to_owned()),
                "external HTTP authentication requires HTTPS",
            ));
        }
        let host = url.host_str().ok_or_else(|| {
            crate::ImError::invalid_input(
                Some("external_http_request.url".to_owned()),
                "external HTTP URL must contain a host",
            )
        })?;
        let canonical_host = host
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
            .unwrap_or(host);
        let authority = match url.port() {
            Some(port) if canonical_host.contains(':') => format!("[{canonical_host}]:{port}"),
            Some(port) => format!("{host}:{port}"),
            None if canonical_host.contains(':') => format!("[{canonical_host}]"),
            None => host.to_owned(),
        };
        Ok(PreparedRequest {
            url: url.as_str().to_owned(),
            method: request.method,
            origin: url.origin().ascii_serialization(),
            authority,
            host: canonical_host.to_ascii_lowercase(),
            headers: request.headers,
            body: request.body,
        })
    }

    fn signature_attempt(
        &self,
        request: PreparedRequest,
        token_key: TokenKey,
        nonce: Option<&str>,
        retry_count: u8,
    ) -> crate::ImResult<ExternalHttpAuthAttempt> {
        let patch = self
            .client
            .runtime()
            .key_provider
            .http_signature_headers(
                &token_key.signing_key_id,
                &request.url,
                &request.method,
                Some(&request.headers),
                request.body.as_deref(),
                anp::authentication::HttpSignatureOptions {
                    nonce: nonce.map(ToOwned::to_owned),
                    covered_components: Some(signing_components(request.body.is_some())),
                    ..Default::default()
                },
            )
            .map_err(|_| crate::ImError::PermissionDenied)?;
        let header_patch = patch
            .into_iter()
            .map(|(name, value)| ExternalHttpHeader::new(name, value))
            .collect::<crate::ImResult<Vec<_>>>()?;
        Ok(ExternalHttpAuthAttempt {
            header_patch,
            request,
            token_key,
            credential: AttemptCredential::Signature,
            retry_count,
        })
    }

    async fn signature_attempt_async(
        &self,
        request: PreparedRequest,
        token_key: TokenKey,
        nonce: Option<&str>,
        retry_count: u8,
    ) -> crate::ImResult<ExternalHttpAuthAttempt> {
        let Some(session) = self.client.runtime().identity_session.as_ref() else {
            return self.signature_attempt(request, token_key, nonce, retry_count);
        };
        use crate::internal::identity_provider::{
            map_provider_error, ProviderExactHttpRequest, ProviderHttpHeader,
            ProviderHttpSigningOptions, ProviderKeySelector,
        };
        let signed = session
            .prepare_http_signature(ProviderExactHttpRequest {
                key: ProviderKeySelector::Kid(token_key.signing_key_id.clone()),
                url: request.url.clone(),
                method: request.method.clone(),
                headers: request
                    .headers
                    .iter()
                    .map(|(name, value)| ProviderHttpHeader {
                        name: name.clone(),
                        value: value.clone(),
                    })
                    .collect(),
                body: request.body.clone(),
                options: ProviderHttpSigningOptions {
                    nonce: nonce.map(ToOwned::to_owned),
                    covered_components: Some(signing_components(request.body.is_some())),
                    ..Default::default()
                },
            })
            .await
            .map_err(map_provider_error)?;
        if signed.kid != token_key.signing_key_id {
            return Err(crate::ImError::PermissionDenied);
        }
        let header_patch = signed
            .header_patch
            .into_iter()
            .map(|header| ExternalHttpHeader::new(header.name, header.value))
            .collect::<crate::ImResult<Vec<_>>>()?;
        Ok(ExternalHttpAuthAttempt {
            header_patch,
            request,
            token_key,
            credential: AttemptCredential::Signature,
            retry_count,
        })
    }

    fn response_plan(
        &self,
        attempt: ExternalHttpAuthAttempt,
        response: ExternalHttpResponse,
    ) -> crate::ImResult<ResponsePlan> {
        if (200..=299).contains(&response.status_code) {
            if let ParsedResponseToken::Token(token) = response_token(&response.headers) {
                self.store_token(attempt.token_key, token)?;
            }
            return Ok(ResponsePlan::Complete);
        }
        if response.status_code != 401 || attempt.retry_count != 0 {
            return Ok(ResponsePlan::Complete);
        }
        let challenge = match response_challenge(&response.headers) {
            ChallengeResult::Absent => Challenge::default(),
            ChallengeResult::DidWba(challenge) => challenge,
            ChallengeResult::Unsupported => return Ok(ResponsePlan::Complete),
        };
        if !challenge_matches_request(&challenge, &attempt.request)
            || is_terminal_error(challenge.error.as_deref())
        {
            return Ok(ResponsePlan::Complete);
        }
        if let AttemptCredential::Bearer { fingerprint } = attempt.credential {
            self.clear_matching_token(&attempt.token_key, fingerprint)?;
        }
        if !accept_signature_compatible(&response.headers) {
            return Ok(ResponsePlan::Complete);
        }
        let signing_key_id = self
            .client
            .runtime()
            .key_provider
            .request_signing_key_id()?;
        if signing_key_id != attempt.token_key.signing_key_id
            || self.client.current_identity().id.as_str() != attempt.token_key.identity_id
            || self.client.did().as_str() != attempt.token_key.did
        {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(ResponsePlan::Retry {
            request: attempt.request,
            token_key: attempt.token_key,
            nonce: challenge.nonce,
        })
    }

    fn cached_token(
        &self,
        key: &TokenKey,
    ) -> crate::ImResult<Option<(Zeroizing<String>, [u8; 32])>> {
        let now = SystemTime::now();
        let mut tokens = self.token_store()?;
        if tokens
            .get(key)
            .and_then(|token| token.expires_at)
            .is_some_and(|expires_at| expires_at <= now)
        {
            tokens.remove(key);
            return Ok(None);
        }
        Ok(tokens
            .get(key)
            .map(|token| (Zeroizing::new(token.value.to_string()), token.fingerprint)))
    }

    fn store_token(&self, key: TokenKey, token: ResponseToken) -> crate::ImResult<()> {
        if token
            .expires_at
            .is_some_and(|expires_at| expires_at <= SystemTime::now())
        {
            return Ok(());
        }
        let fingerprint = token_fingerprint(&token.value);
        self.token_store()?.insert(
            key,
            StoredToken {
                value: Zeroizing::new(token.value),
                fingerprint,
                expires_at: token.expires_at,
            },
        );
        Ok(())
    }

    fn clear_matching_token(&self, key: &TokenKey, fingerprint: [u8; 32]) -> crate::ImResult<()> {
        let mut tokens = self.token_store()?;
        if tokens
            .get(key)
            .is_some_and(|current| current.fingerprint == fingerprint)
        {
            tokens.remove(key);
        }
        Ok(())
    }

    fn token_store(&self) -> crate::ImResult<MutexGuard<'_, HashMap<TokenKey, StoredToken>>> {
        self.client
            .external_http_auth_state()
            .tokens
            .lock()
            .map_err(|_| crate::ImError::Internal {
                message: "external HTTP authentication state is unavailable".to_owned(),
            })
    }
}

enum ResponsePlan {
    Complete,
    Retry {
        request: PreparedRequest,
        token_key: TokenKey,
        nonce: Option<String>,
    },
}

fn signing_components(body_present: bool) -> Vec<String> {
    let mut components = FIXED_COMPONENTS
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<Vec<_>>();
    if body_present {
        components.push("content-digest".to_owned());
    }
    components
}

fn is_literal_loopback(url: &reqwest::Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(host);
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .map(|address| address.is_loopback())
            .unwrap_or(false)
}

#[derive(Default)]
struct Challenge {
    realm: Option<String>,
    error: Option<String>,
    nonce: Option<String>,
}

enum ChallengeResult {
    Absent,
    DidWba(Challenge),
    Unsupported,
}

fn response_challenge(headers: &BTreeMap<String, String>) -> ChallengeResult {
    let Some(value) = headers.get("www-authenticate") else {
        return ChallengeResult::Absent;
    };
    let Some(challenges) = parse_authenticate_challenges(value) else {
        return ChallengeResult::Unsupported;
    };
    let mut did_wba = challenges
        .into_iter()
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("DIDWba"));
    let Some((_, parameters)) = did_wba.next() else {
        return ChallengeResult::Unsupported;
    };
    if did_wba.next().is_some() {
        return ChallengeResult::Unsupported;
    }
    let Some(parameters) = parse_header_parameters(parameters) else {
        return ChallengeResult::Unsupported;
    };
    ChallengeResult::DidWba(Challenge {
        realm: parameters.get("realm").cloned(),
        error: parameters
            .get("error")
            .map(|value| value.to_ascii_lowercase()),
        nonce: parameters
            .get("nonce")
            .filter(|value| !value.is_empty())
            .cloned(),
    })
}

fn challenge_matches_request(challenge: &Challenge, request: &PreparedRequest) -> bool {
    let Some(realm) = challenge.realm.as_deref() else {
        return true;
    };
    let realm = realm.trim();
    realm.eq_ignore_ascii_case(&request.origin)
        || realm.eq_ignore_ascii_case(&request.authority)
        || realm.eq_ignore_ascii_case(&request.host)
}

fn is_terminal_error(error: Option<&str>) -> bool {
    matches!(
        error,
        Some(
            "invalid_did"
                | "invalid_verification_method"
                | "forbidden_did"
                | "invalid_request"
                | "invalid_content_digest"
        )
    )
}

fn accept_signature_compatible(headers: &BTreeMap<String, String>) -> bool {
    let Some(value) = headers.get("accept-signature") else {
        return true;
    };
    let value = value.trim();
    if !value.starts_with("sig1=") {
        return false;
    }
    let Some(open) = value.find('(') else {
        return false;
    };
    let Some(close) = value[open + 1..].find(')').map(|index| open + 1 + index) else {
        return false;
    };
    let Some(components) = quoted_values(&value[open + 1..close]) else {
        return false;
    };
    if components.is_empty() {
        return false;
    }
    components.into_iter().all(|component| {
        FIXED_COMPONENTS
            .iter()
            .any(|allowed| component.eq_ignore_ascii_case(allowed))
            || component.eq_ignore_ascii_case("content-digest")
    })
}

enum ParsedResponseToken {
    Absent,
    Malformed,
    Token(ResponseToken),
}

struct ResponseToken {
    value: String,
    expires_at: Option<SystemTime>,
}

fn response_token(headers: &BTreeMap<String, String>) -> ParsedResponseToken {
    let Some(value) = headers.get("authentication-info") else {
        return ParsedResponseToken::Absent;
    };
    let Some(parameters) = parse_header_parameters(value) else {
        return ParsedResponseToken::Malformed;
    };
    let Some(token) = parameters
        .get("access_token")
        .filter(|token| !token.trim().is_empty())
    else {
        return ParsedResponseToken::Malformed;
    };
    if parameters
        .get("token_type")
        .is_some_and(|kind| !kind.eq_ignore_ascii_case("Bearer"))
    {
        return ParsedResponseToken::Malformed;
    }
    let now = SystemTime::now();
    let header_expiry = match parameters.get("expires_in") {
        Some(value) => {
            let Some(seconds) = canonical_u64(value) else {
                return ParsedResponseToken::Malformed;
            };
            let Some(expires_at) = now.checked_add(Duration::from_secs(seconds)) else {
                return ParsedResponseToken::Malformed;
            };
            Some(expires_at)
        }
        None => None,
    };
    let jwt_expiry = jwt_expiry(token);
    let expires_at = match (header_expiry, jwt_expiry) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    };
    ParsedResponseToken::Token(ResponseToken {
        value: token.to_owned(),
        expires_at,
    })
}

fn parse_header_parameters(value: &str) -> Option<BTreeMap<String, String>> {
    let bytes = value.as_bytes();
    let mut index = 0;
    let mut result = BTreeMap::new();
    while index < bytes.len() {
        skip_ows_and_commas(bytes, &mut index);
        if index == bytes.len() {
            break;
        }
        let key_start = index;
        while index < bytes.len() && is_parameter_name_byte(bytes[index]) {
            index += 1;
        }
        if key_start == index {
            return None;
        }
        let key = value[key_start..index].to_ascii_lowercase();
        skip_ows(bytes, &mut index);
        if bytes.get(index) != Some(&b'=') {
            return None;
        }
        index += 1;
        skip_ows(bytes, &mut index);
        let parsed = if bytes.get(index) == Some(&b'"') {
            parse_quoted(value, bytes, &mut index)?
        } else {
            let start = index;
            while index < bytes.len() && bytes[index] != b',' {
                index += 1;
            }
            let parsed = value[start..index].trim();
            if parsed.is_empty() {
                return None;
            }
            parsed.to_owned()
        };
        if result.insert(key, parsed).is_some() {
            return None;
        }
        skip_ows(bytes, &mut index);
        if index < bytes.len() {
            if bytes[index] != b',' {
                return None;
            }
            index += 1;
        }
    }
    Some(result)
}

fn parse_authenticate_challenges(value: &str) -> Option<Vec<(&str, &str)>> {
    let bytes = value.as_bytes();
    let mut index = 0;
    let mut challenges = Vec::new();
    while index < bytes.len() {
        skip_ows_and_commas(bytes, &mut index);
        if index == bytes.len() {
            break;
        }
        let scheme_start = index;
        while index < bytes.len() && is_http_token_byte(bytes[index]) {
            index += 1;
        }
        if scheme_start == index {
            return None;
        }
        let scheme = &value[scheme_start..index];
        if index == bytes.len() {
            challenges.push((scheme, ""));
            break;
        }
        if !bytes[index].is_ascii_whitespace() {
            if bytes[index] != b',' {
                return None;
            }
            challenges.push((scheme, ""));
            index += 1;
            continue;
        }
        skip_ows(bytes, &mut index);
        let parameters_start = index;
        let mut cursor = index;
        let mut quoted = false;
        let mut escaped = false;
        let mut parameters_end = bytes.len();
        while cursor < bytes.len() {
            let byte = bytes[cursor];
            if quoted {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == b'"' {
                    quoted = false;
                }
                cursor += 1;
                continue;
            }
            if byte == b'"' {
                quoted = true;
                cursor += 1;
                continue;
            }
            if byte != b',' {
                cursor += 1;
                continue;
            }

            let mut next = cursor + 1;
            skip_ows(bytes, &mut next);
            if next == bytes.len() {
                parameters_end = cursor;
                index = bytes.len();
                break;
            }
            let token_start = next;
            while next < bytes.len() && is_http_token_byte(bytes[next]) {
                next += 1;
            }
            if token_start == next {
                return None;
            }
            let mut after_token = next;
            skip_ows(bytes, &mut after_token);
            if bytes.get(after_token) == Some(&b'=') {
                cursor += 1;
                continue;
            }
            parameters_end = cursor;
            index = token_start;
            break;
        }
        if quoted || escaped {
            return None;
        }
        if cursor == bytes.len() {
            index = bytes.len();
        }
        challenges.push((scheme, value[parameters_start..parameters_end].trim()));
    }
    (!challenges.is_empty()).then_some(challenges)
}

fn parse_quoted(value: &str, bytes: &[u8], index: &mut usize) -> Option<String> {
    *index += 1;
    let mut parsed = String::new();
    let mut segment_start = *index;
    while *index < bytes.len() {
        match bytes[*index] {
            b'"' => {
                parsed.push_str(&value[segment_start..*index]);
                *index += 1;
                return Some(parsed);
            }
            b'\\' => {
                parsed.push_str(&value[segment_start..*index]);
                *index += 1;
                let escaped = *bytes.get(*index)?;
                if !escaped.is_ascii() || matches!(escaped, b'\r' | b'\n' | 0) {
                    return None;
                }
                parsed.push(char::from(escaped));
                *index += 1;
                segment_start = *index;
            }
            b'\r' | b'\n' | 0 => return None,
            _ => *index += 1,
        }
    }
    None
}

fn quoted_values(value: &str) -> Option<Vec<String>> {
    let bytes = value.as_bytes();
    let mut index = 0;
    let mut values = Vec::new();
    while index < bytes.len() {
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index == bytes.len() {
            break;
        }
        if bytes[index] != b'"' {
            return None;
        }
        values.push(parse_quoted(value, bytes, &mut index)?);
    }
    Some(values)
}

fn jwt_expiry(token: &str) -> Option<SystemTime> {
    let mut parts = token.split('.');
    let _header = parts.next()?;
    let payload = parts.next()?;
    let _signature = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    let decoded = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    let seconds = value
        .get("exp")?
        .as_u64()
        .or_else(|| value.get("exp")?.as_str()?.parse().ok())?;
    UNIX_EPOCH.checked_add(Duration::from_secs(seconds))
}

fn canonical_u64(value: &str) -> Option<u64> {
    if value.is_empty()
        || !value.bytes().all(|byte| byte.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return None;
    }
    value.parse().ok()
}

fn token_fingerprint(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

fn skip_ows(bytes: &[u8], index: &mut usize) {
    while matches!(bytes.get(*index), Some(b' ' | b'\t')) {
        *index += 1;
    }
}

fn skip_ows_and_commas(bytes: &[u8], index: &mut usize) {
    while matches!(bytes.get(*index), Some(b' ' | b'\t' | b',')) {
        *index += 1;
    }
}

fn is_parameter_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
}

fn is_http_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}
