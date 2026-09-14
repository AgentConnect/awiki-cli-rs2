use std::collections::BTreeMap;

use serde_json::Value;

pub(crate) fn resolve_did_document<T>(transport: &mut T, did: &str) -> crate::ImResult<Value>
where
    T: crate::internal::transport::RawJsonTransport,
{
    let document = if did.starts_with("did:web:") {
        transport.resolve_web_document(did)?
    } else {
        let url = did_document_url(did)?;
        transport.get_json_url(
            &url,
            BTreeMap::from([("Accept".to_owned(), "application/json".to_owned())]),
        )?
    };
    validate_resolved_did_document(did, document)
}

pub(crate) async fn resolve_did_document_async<T>(
    transport: &mut T,
    did: &str,
) -> crate::ImResult<Value>
where
    T: crate::internal::transport::AsyncRawJsonTransport,
{
    let document = if did.starts_with("did:web:") {
        transport.resolve_web_document(did).await?
    } else {
        let url = did_document_url(did)?;
        transport
            .get_json_url(
                &url,
                BTreeMap::from([("Accept".to_owned(), "application/json".to_owned())]),
            )
            .await?
    };
    validate_resolved_did_document(did, document)
}

pub(crate) fn validate_resolved_did_document(did: &str, document: Value) -> crate::ImResult<Value> {
    if document.get("id").and_then(Value::as_str) != Some(did) {
        return Err(crate::ImError::invalid_input(
            Some("did_document".to_string()),
            "resolved DID document id does not match requested DID",
        ));
    }
    if !anp::authentication::validate_did_document_method(&document, true) {
        return Err(crate::ImError::invalid_input(
            Some("did_document".to_string()),
            "resolved DID document binding is invalid",
        ));
    }
    Ok(document)
}

pub(crate) fn did_document_url(did: &str) -> crate::ImResult<String> {
    if did.starts_with("did:web:") {
        return anp::authentication::build_did_web_resolution_url(did)
            .map_err(|_| invalid_did("invalid Web DID resolution URL"));
    }
    let (scheme, rest) = did
        .split_once(':')
        .ok_or_else(|| invalid_did("invalid DID format"))?;
    if scheme != "did" {
        return Err(invalid_did("invalid DID format"));
    }
    let (method, suffix) = rest
        .split_once(':')
        .ok_or_else(|| invalid_did("invalid DID format"))?;
    let mut parts = suffix.split(':');
    let domain = parts
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid_did("invalid DID format"))?;
    let domain = percent_decode_lossy(domain);
    let path_segments = parts.map(percent_decode_lossy).collect::<Vec<String>>();
    match method {
        "wba" => {
            if path_segments.is_empty() {
                Ok(format!("https://{domain}/.well-known/did.json"))
            } else {
                Ok(format!(
                    "https://{}/{}/did.json",
                    domain,
                    path_segments.join("/")
                ))
            }
        }
        _ => Err(invalid_did(
            "unsupported DID method for DID document resolution",
        )),
    }
}

fn invalid_did(message: impl Into<String>) -> crate::ImError {
    crate::ImError::invalid_input(Some("did".to_string()), message)
}

fn percent_decode_lossy(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Some(byte) = hex_pair(bytes[index + 1], bytes[index + 2]) {
                decoded.push(byte);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex_pair(high: u8, low: u8) -> Option<u8> {
    Some(hex_value(high)? * 16 + hex_value(low)?)
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn did_document_url_matches_anp_wba_and_web_resolution_paths() {
        assert_eq!(
            did_document_url("did:wba:awiki.info").unwrap(),
            "https://awiki.info/.well-known/did.json"
        );
        assert_eq!(
            did_document_url("did:wba:awiki.info:alice:e1_abc").unwrap(),
            "https://awiki.info/alice/e1_abc/did.json"
        );
        assert_eq!(
            did_document_url("did:web:example.com:user:alice").unwrap(),
            "https://example.com/user/alice/did.json"
        );
        assert_eq!(
            did_document_url("did:wba:example.com:user%20name:e1_abc").unwrap(),
            "https://example.com/user name/e1_abc/did.json"
        );
        assert!(did_document_url("did:key:z6mk").is_err());
    }

    #[test]
    fn web_url_rejects_ambiguous_and_private_authorities_before_transport() {
        for did in [
            "did:web:127.0.0.1",
            "did:web:localhost",
            "did:web:example.test:a%2Fb",
            "did:web:example.test:%2e%2e",
            "did:web:example.test:bad%",
            "did:web:example.test:user?secret",
            "did:web:example.test:user#key",
        ] {
            assert!(did_document_url(did).is_err(), "{did}");
        }
        assert_eq!(
            did_document_url("did:web:example.test%3A8443:awiki:web:abc").unwrap(),
            "https://example.test:8443/awiki/web/abc/did.json"
        );
    }

    #[tokio::test]
    async fn web_resolution_dispatches_through_the_method_transport_and_checks_exact_id() {
        struct MethodTransport;
        impl crate::internal::transport::AsyncRawJsonTransport for MethodTransport {
            async fn get_json_url(
                &mut self,
                _: &str,
                _: BTreeMap<String, String>,
            ) -> crate::ImResult<Value> {
                panic!("Web resolution must use the method transport policy")
            }
            async fn resolve_web_document(&mut self, _: &str) -> crate::ImResult<Value> {
                Ok(serde_json::json!({"id":"did:web:example.test:alice"}))
            }
        }
        assert!(
            resolve_did_document_async(&mut MethodTransport, "did:web:example.test:alice")
                .await
                .is_ok()
        );
        assert!(
            resolve_did_document_async(&mut MethodTransport, "did:web:example.test:bob")
                .await
                .is_err()
        );
    }
}
