use std::collections::BTreeMap;
use std::fmt;

/// Maximum buffered and replayable body accepted by external HTTP auth.
pub const EXTERNAL_HTTP_AUTH_MAX_BODY_BYTES: usize = 4 * 1024 * 1024;

const MANAGED_HEADERS: [&str; 5] = [
    "authorization",
    "signature-input",
    "signature",
    "content-digest",
    "x-awiki-client-version",
];

/// One validated HTTP field. Debug output always redacts the value.
#[derive(Clone, PartialEq, Eq)]
pub struct ExternalHttpHeader {
    name: String,
    value: String,
}

impl ExternalHttpHeader {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> crate::ImResult<Self> {
        let name = name.into();
        let value = value.into();
        if name.is_empty() || !name.bytes().all(is_header_name_byte) {
            return Err(crate::ImError::invalid_input(
                Some("external_http_header.name".to_owned()),
                "HTTP header name is invalid",
            ));
        }
        if value.bytes().any(|byte| matches!(byte, b'\r' | b'\n' | 0)) {
            return Err(crate::ImError::invalid_input(
                Some("external_http_header.value".to_owned()),
                "HTTP header value is invalid",
            ));
        }
        Ok(Self { name, value })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn value(&self) -> &str {
        &self.value
    }
}

impl fmt::Debug for ExternalHttpHeader {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExternalHttpHeader")
            .field("name", &self.name)
            .field("value", &"<redacted>")
            .finish()
    }
}

/// Exact HTTP request metadata and buffered bytes supplied by a trusted host.
pub struct ExternalHttpRequest {
    pub(crate) url: String,
    pub(crate) method: String,
    pub(crate) headers: BTreeMap<String, String>,
    pub(crate) body: Option<Vec<u8>>,
}

impl ExternalHttpRequest {
    pub fn new(
        url: impl Into<String>,
        method: impl Into<String>,
        headers: Vec<ExternalHttpHeader>,
        body: Option<Vec<u8>>,
    ) -> crate::ImResult<Self> {
        if body
            .as_ref()
            .is_some_and(|body| body.len() > EXTERNAL_HTTP_AUTH_MAX_BODY_BYTES)
        {
            return Err(crate::ImError::invalid_input(
                Some("external_http_request.body".to_owned()),
                "external HTTP request body exceeds the 4 MiB limit",
            ));
        }
        let method = method.into();
        if method.is_empty()
            || method != method.to_ascii_uppercase()
            || !method.bytes().all(is_header_name_byte)
        {
            return Err(crate::ImError::invalid_input(
                Some("external_http_request.method".to_owned()),
                "HTTP method must be an uppercase token",
            ));
        }
        let headers = canonical_headers(headers, true)?;
        Ok(Self {
            url: url.into(),
            method,
            headers,
            body,
        })
    }
}

impl fmt::Debug for ExternalHttpRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExternalHttpRequest")
            .field("url", &"<redacted-url>")
            .field("method", &self.method)
            .field("header_names", &self.headers.keys().collect::<Vec<_>>())
            .field(
                "body",
                &self
                    .body
                    .as_ref()
                    .map(|body| format!("<redacted:{} bytes>", body.len())),
            )
            .finish()
    }
}

/// HTTP response metadata observed without consuming the response body.
pub struct ExternalHttpResponse {
    pub(crate) status_code: u16,
    pub(crate) headers: BTreeMap<String, String>,
}

impl ExternalHttpResponse {
    pub fn new(status_code: u16, headers: Vec<ExternalHttpHeader>) -> crate::ImResult<Self> {
        if !(100..=599).contains(&status_code) {
            return Err(crate::ImError::invalid_input(
                Some("external_http_response.status_code".to_owned()),
                "HTTP status code must be between 100 and 599",
            ));
        }
        Ok(Self {
            status_code,
            headers: canonical_headers(headers, false)?,
        })
    }
}

impl fmt::Debug for ExternalHttpResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExternalHttpResponse")
            .field("status_code", &self.status_code)
            .field("header_names", &self.headers.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// A single-use authentication attempt. It is intentionally not Clone.
pub struct ExternalHttpAuthAttempt {
    pub(crate) header_patch: Vec<ExternalHttpHeader>,
    pub(crate) request: PreparedRequest,
    pub(crate) token_key: TokenKey,
    pub(crate) credential: AttemptCredential,
    pub(crate) retry_count: u8,
}

impl ExternalHttpAuthAttempt {
    pub fn header_patch(&self) -> &[ExternalHttpHeader] {
        &self.header_patch
    }

    /// Canonical target URI covered by the signature and required for send.
    pub fn target_url(&self) -> &str {
        &self.request.url
    }

    /// Canonical uppercase HTTP method covered by the signature.
    pub fn method(&self) -> &str {
        &self.request.method
    }

    pub fn retry_count(&self) -> u8 {
        self.retry_count
    }
}

impl fmt::Debug for ExternalHttpAuthAttempt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExternalHttpAuthAttempt")
            .field(
                "header_names",
                &self
                    .header_patch
                    .iter()
                    .map(|h| h.name())
                    .collect::<Vec<_>>(),
            )
            .field("origin", &self.request.origin)
            .field("credential", &self.credential)
            .field("retry_count", &self.retry_count)
            .finish()
    }
}

/// Response handling either completes or returns the only allowed retry.
pub enum ExternalHttpAuthDecision {
    Complete,
    Retry(ExternalHttpAuthAttempt),
}

impl fmt::Debug for ExternalHttpAuthDecision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Complete => formatter.write_str("Complete"),
            Self::Retry(attempt) => formatter.debug_tuple("Retry").field(attempt).finish(),
        }
    }
}

pub(crate) struct PreparedRequest {
    pub(crate) url: String,
    pub(crate) method: String,
    pub(crate) origin: String,
    pub(crate) authority: String,
    pub(crate) host: String,
    pub(crate) headers: BTreeMap<String, String>,
    pub(crate) body: Option<Vec<u8>>,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct TokenKey {
    pub(crate) identity_id: String,
    pub(crate) did: String,
    pub(crate) signing_key_id: String,
    pub(crate) origin: String,
}

pub(crate) enum AttemptCredential {
    Bearer { fingerprint: [u8; 32] },
    Signature,
}

impl fmt::Debug for AttemptCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bearer { .. } => formatter.write_str("Bearer(<redacted>)"),
            Self::Signature => formatter.write_str("Signature(<redacted>)"),
        }
    }
}

fn canonical_headers(
    headers: Vec<ExternalHttpHeader>,
    reject_managed: bool,
) -> crate::ImResult<BTreeMap<String, String>> {
    let mut canonical = BTreeMap::new();
    for header in headers {
        let name = header.name.to_ascii_lowercase();
        if reject_managed && MANAGED_HEADERS.contains(&name.as_str()) {
            return Err(crate::ImError::invalid_input(
                Some("external_http_request.headers".to_owned()),
                "request already contains an SDK-managed authentication header",
            ));
        }
        if canonical.insert(name, header.value).is_some() {
            return Err(crate::ImError::invalid_input(
                Some("external_http_headers".to_owned()),
                "duplicate HTTP header names are not supported",
            ));
        }
    }
    Ok(canonical)
}

fn is_header_name_byte(byte: u8) -> bool {
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
