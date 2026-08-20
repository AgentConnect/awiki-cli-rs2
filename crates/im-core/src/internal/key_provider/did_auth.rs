use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

pub(crate) struct ProviderBackedDidAuth {
    provider: Arc<dyn super::IdentitySigner>,
    auth_mode: anp::authentication::AuthMode,
    tokens: HashMap<String, String>,
}

impl ProviderBackedDidAuth {
    pub(crate) fn new(
        provider: Arc<dyn super::IdentitySigner>,
        auth_mode: anp::authentication::AuthMode,
    ) -> Self {
        Self {
            provider,
            auth_mode,
            tokens: HashMap::new(),
        }
    }

    pub(crate) fn get_auth_header(
        &mut self,
        server_url: &str,
        force_new: bool,
        method: &str,
        headers: Option<&BTreeMap<String, String>>,
        body: Option<&[u8]>,
    ) -> crate::ImResult<BTreeMap<String, String>> {
        let token_origin = extract_origin(server_url);
        if !force_new {
            if let Some(token) = self.tokens.get(&token_origin) {
                return Ok(BTreeMap::from([(
                    "Authorization".to_string(),
                    format!("Bearer {token}"),
                )]));
            }
        }

        let key_id = self.provider.request_signing_key_id()?;
        match self.auth_mode {
            anp::authentication::AuthMode::HttpSignatures | anp::authentication::AuthMode::Auto => {
                self.provider
                    .http_signature_headers(
                        &key_id,
                        server_url,
                        method,
                        headers,
                        body,
                        anp::authentication::HttpSignatureOptions {
                            ..anp::authentication::HttpSignatureOptions::default()
                        },
                    )
                    .map_err(|err| crate::ImError::TransportUnavailable {
                        detail: format!("DID-WBA HTTP signature generation failed: {err}"),
                    })
            }
            anp::authentication::AuthMode::LegacyDidWba => {
                let value = self
                    .provider
                    .legacy_did_wba_header(&key_id, &extract_domain(server_url), "1.1")
                    .map_err(|err| crate::ImError::TransportUnavailable {
                        detail: format!("DID-WBA legacy auth generation failed: {err}"),
                    })?;
                Ok(BTreeMap::from([("Authorization".to_string(), value)]))
            }
        }
    }

    pub(crate) fn update_token(
        &mut self,
        server_url: &str,
        headers: &BTreeMap<String, String>,
    ) -> crate::ImResult<Option<String>> {
        let token = Self::response_token(headers)?;
        if let Some(token) = token.as_ref() {
            self.store_token(server_url, token);
        }
        Ok(token)
    }

    pub(crate) fn response_token(
        headers: &BTreeMap<String, String>,
    ) -> crate::ImResult<Option<String>> {
        let authentication_info = get_header_case_insensitive(headers, "Authentication-Info")
            .and_then(|value| parse_header_params(value).remove("access_token"))
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        let authorization = get_header_case_insensitive(headers, "Authorization")
            .and_then(|value| {
                value
                    .trim()
                    .strip_prefix("Bearer ")
                    .or_else(|| value.trim().strip_prefix("bearer "))
            })
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        if let (Some(left), Some(right)) = (&authentication_info, &authorization) {
            if left != right {
                return Err(crate::ImError::PermissionDenied);
            }
        }
        Ok(authentication_info.or(authorization))
    }

    pub(crate) fn store_token(&mut self, server_url: &str, token: &str) {
        self.tokens
            .insert(extract_origin(server_url), token.to_owned());
    }

    pub(crate) fn clear_token(&mut self, server_url: &str) {
        self.tokens.remove(&extract_origin(server_url));
    }

    pub(crate) fn should_retry_after_401(
        &self,
        response_headers: &BTreeMap<String, String>,
    ) -> bool {
        let Some(www_authenticate) =
            get_header_case_insensitive(response_headers, "WWW-Authenticate")
        else {
            return false;
        };
        let challenge = parse_www_authenticate(www_authenticate);
        if challenge.contains_key("nonce") {
            return true;
        }
        !matches!(
            challenge.get("error").map(|value| value.as_str()),
            Some("invalid_did") | Some("invalid_verification_method") | Some("forbidden_did")
        )
    }

    pub(crate) fn get_challenge_auth_header(
        &mut self,
        server_url: &str,
        response_headers: &BTreeMap<String, String>,
        method: &str,
        headers: Option<&BTreeMap<String, String>>,
        body: Option<&[u8]>,
    ) -> crate::ImResult<BTreeMap<String, String>> {
        let www_authenticate = get_header_case_insensitive(response_headers, "WWW-Authenticate");
        let accept_signature = get_header_case_insensitive(response_headers, "Accept-Signature");
        let challenge = www_authenticate
            .map(|value| parse_www_authenticate(value))
            .unwrap_or_default();
        let covered_components = normalize_covered_components(
            accept_signature
                .map(|value| parse_accept_signature(value))
                .as_ref(),
            headers,
            body,
        );
        let nonce = challenge.get("nonce").cloned();

        let key_id = self.provider.request_signing_key_id()?;
        match self.auth_mode {
            anp::authentication::AuthMode::HttpSignatures | anp::authentication::AuthMode::Auto => {
                self.provider
                    .http_signature_headers(
                        &key_id,
                        server_url,
                        method,
                        headers,
                        body,
                        anp::authentication::HttpSignatureOptions {
                            nonce,
                            covered_components,
                            ..anp::authentication::HttpSignatureOptions::default()
                        },
                    )
                    .map_err(|err| crate::ImError::TransportUnavailable {
                        detail: format!("DID-WBA challenge signature generation failed: {err}"),
                    })
            }
            anp::authentication::AuthMode::LegacyDidWba => {
                let value = self
                    .provider
                    .legacy_did_wba_header(&key_id, &extract_domain(server_url), "1.1")
                    .map_err(|err| crate::ImError::TransportUnavailable {
                        detail: format!("DID-WBA challenge legacy auth generation failed: {err}"),
                    })?;
                Ok(BTreeMap::from([("Authorization".to_string(), value)]))
            }
        }
    }
}

fn extract_origin(server_url: &str) -> String {
    reqwest::Url::parse(server_url)
        .ok()
        .and_then(|url| {
            let host = url.host_str()?;
            let mut origin = format!("{}://{host}", url.scheme().to_ascii_lowercase());
            if let Some(port) = url.port() {
                origin.push(':');
                origin.push_str(&port.to_string());
            }
            Some(origin)
        })
        .unwrap_or_else(|| server_url.trim().to_ascii_lowercase())
}

fn extract_domain(server_url: &str) -> String {
    server_url
        .split_once("://")
        .map(|(_, rest)| rest)
        .and_then(|rest| rest.split(['/', '?', '#']).next())
        .and_then(|authority| {
            authority
                .rsplit_once('@')
                .map(|(_, host)| host)
                .or(Some(authority))
        })
        .map(|authority| {
            if let Some(stripped) = authority.strip_prefix('[') {
                stripped
                    .split_once(']')
                    .map(|(host, _)| host.to_string())
                    .unwrap_or_else(|| authority.to_string())
            } else {
                authority
                    .split_once(':')
                    .map(|(host, _)| host.to_string())
                    .unwrap_or_else(|| authority.to_string())
            }
        })
        .filter(|host| !host.is_empty())
        .unwrap_or_else(|| server_url.to_string())
}

fn get_header_case_insensitive<'a>(
    headers: &'a BTreeMap<String, String>,
    name: &str,
) -> Option<&'a String> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value)
}

fn parse_header_params(value: &str) -> HashMap<String, String> {
    value
        .split(',')
        .filter_map(|item| item.trim().split_once('='))
        .map(|(key, raw)| {
            (
                key.trim().to_string(),
                raw.trim().trim_matches('"').to_string(),
            )
        })
        .collect()
}

fn parse_www_authenticate(value: &str) -> HashMap<String, String> {
    let normalized = value
        .trim()
        .strip_prefix("DIDWba ")
        .or_else(|| value.trim().strip_prefix("didwba "))
        .unwrap_or(value.trim());
    parse_header_params(normalized)
}

fn parse_accept_signature(value: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut remaining = value;
    while let Some((_, after_open)) = remaining.split_once('"') {
        let Some((component, after_close)) = after_open.split_once('"') else {
            break;
        };
        if !component.trim().is_empty() {
            result.push(component.to_string());
        }
        remaining = after_close;
    }
    result
}

fn normalize_covered_components(
    covered_components: Option<&Vec<String>>,
    headers: Option<&BTreeMap<String, String>>,
    body: Option<&[u8]>,
) -> Option<Vec<String>> {
    let covered_components = covered_components?;
    let body_present = body.map(|bytes| !bytes.is_empty()).unwrap_or(false);
    let normalized_headers = headers
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(key, value)| (!value.is_empty()).then(|| (key.to_ascii_lowercase(), value)))
        .collect::<BTreeMap<_, _>>();

    let mut result = Vec::new();
    for component in covered_components {
        let lower = component.to_ascii_lowercase();
        if lower == "content-digest" && !body_present {
            continue;
        }
        if lower == "content-length"
            && !body_present
            && !normalized_headers.contains_key("content-length")
        {
            continue;
        }
        if lower == "content-type" && !normalized_headers.contains_key("content-type") {
            continue;
        }
        if !lower.starts_with('@')
            && lower != "content-length"
            && lower != "content-digest"
            && !normalized_headers.contains_key(&lower)
        {
            continue;
        }
        result.push(component.clone());
    }
    (!result.is_empty()).then_some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal::key_provider::FileBackedIdentitySigner;

    #[test]
    fn provider_did_auth_generates_http_signature_headers() {
        let bundle = anp::authentication::create_did_wba_document(
            "example.com",
            anp::authentication::DidDocumentOptions::default(),
        )
        .expect("DID creation should succeed");
        let root = tempfile::tempdir().unwrap();
        let identity_dir = root.path().join("identity");
        std::fs::create_dir_all(&identity_dir).unwrap();
        std::fs::write(
            identity_dir.join("did.json"),
            serde_json::to_vec(&bundle.did_document).unwrap(),
        )
        .unwrap();
        std::fs::write(
            identity_dir.join("private.key"),
            &bundle.keys["key-1"].private_key_pem,
        )
        .unwrap();

        let provider = Arc::new(FileBackedIdentitySigner::new(identity_dir));
        let mut auth =
            ProviderBackedDidAuth::new(provider, anp::authentication::AuthMode::HttpSignatures);
        let headers = auth
            .get_auth_header("https://api.example.com/orders", false, "GET", None, None)
            .expect("headers should generate");

        assert!(headers.contains_key("Signature-Input"));
        assert!(headers.contains_key("Signature"));
    }

    #[test]
    fn provider_did_auth_challenge_uses_http_signature_nonce() {
        let bundle = anp::authentication::create_did_wba_document(
            "example.com",
            anp::authentication::DidDocumentOptions::default(),
        )
        .expect("DID creation should succeed");
        let root = tempfile::tempdir().unwrap();
        let identity_dir = root.path().join("identity");
        std::fs::create_dir_all(&identity_dir).unwrap();
        std::fs::write(
            identity_dir.join("did.json"),
            serde_json::to_vec(&bundle.did_document).unwrap(),
        )
        .unwrap();
        std::fs::write(
            identity_dir.join("private.key"),
            &bundle.keys["key-1"].private_key_pem,
        )
        .unwrap();

        let provider = Arc::new(FileBackedIdentitySigner::new(identity_dir));
        let mut auth =
            ProviderBackedDidAuth::new(provider, anp::authentication::AuthMode::HttpSignatures);
        let request_headers =
            BTreeMap::from([("Content-Type".to_string(), "application/json".to_string())]);
        let challenge_headers = BTreeMap::from([(
            "WWW-Authenticate".to_string(),
            r#"DIDWba nonce="server-nonce-42""#.to_string(),
        )]);
        let signed_headers = auth
            .get_challenge_auth_header(
                "https://api.example.com/orders",
                &challenge_headers,
                "POST",
                Some(&request_headers),
                Some(br#"{"ok":true}"#),
            )
            .expect("challenge signature should generate");

        let metadata = anp::authentication::extract_signature_metadata(&signed_headers)
            .expect("signature metadata should parse");
        assert_eq!(metadata.nonce.as_deref(), Some("server-nonce-42"));
    }

    #[test]
    fn provider_did_auth_reuses_cached_bearer_token_without_reading_key() {
        let root = tempfile::tempdir().unwrap();
        let identity_dir = root.path().join("identity");
        std::fs::create_dir_all(&identity_dir).unwrap();

        let provider = Arc::new(FileBackedIdentitySigner::new(identity_dir));
        let mut auth =
            ProviderBackedDidAuth::new(provider, anp::authentication::AuthMode::HttpSignatures);
        auth.update_token(
            "https://api.example.com/orders",
            &BTreeMap::from([(
                "Authentication-Info".to_string(),
                r#"access_token="cached-token""#.to_string(),
            )]),
        )
        .unwrap();

        let headers = auth
            .get_auth_header("https://api.example.com/orders", false, "GET", None, None)
            .expect("cached token should be used before key material is read");

        assert_eq!(
            headers.get("Authorization").map(String::as_str),
            Some("Bearer cached-token")
        );
    }

    #[test]
    fn provider_did_auth_accepts_authorization_bearer_and_scopes_cache_to_origin() {
        let root = tempfile::tempdir().unwrap();
        let provider = Arc::new(FileBackedIdentitySigner::new(
            root.path().join("missing-identity"),
        ));
        let mut auth =
            ProviderBackedDidAuth::new(provider, anp::authentication::AuthMode::HttpSignatures);
        auth.update_token(
            "https://api.example.com/first",
            &BTreeMap::from([(
                "Authorization".to_owned(),
                "Bearer response-token".to_owned(),
            )]),
        )
        .unwrap();

        let cached = auth
            .get_auth_header("https://api.example.com/second", false, "GET", None, None)
            .unwrap();
        assert_eq!(
            cached.get("Authorization").map(String::as_str),
            Some("Bearer response-token")
        );
        assert!(auth
            .get_auth_header(
                "https://api.example.com:8443/second",
                false,
                "GET",
                None,
                None
            )
            .is_err());
    }

    #[test]
    fn provider_did_auth_rejects_conflicting_response_token_headers() {
        let root = tempfile::tempdir().unwrap();
        let provider = Arc::new(FileBackedIdentitySigner::new(
            root.path().join("missing-identity"),
        ));
        let mut auth =
            ProviderBackedDidAuth::new(provider, anp::authentication::AuthMode::HttpSignatures);
        let result = auth.update_token(
            "https://api.example.com/first",
            &BTreeMap::from([
                (
                    "Authentication-Info".to_owned(),
                    r#"access_token="token-one""#.to_owned(),
                ),
                ("Authorization".to_owned(), "Bearer token-two".to_owned()),
            ]),
        );
        assert_eq!(result, Err(crate::ImError::PermissionDenied));
    }
}
