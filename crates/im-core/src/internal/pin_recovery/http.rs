//! Exact-body DPoP transport. No redirects, response bodies or credentials in errors.
use super::{
    crypto, failure,
    pending::{ExactRequest, Session, Store},
};
use p256::ecdsa::{signature::Signer, Signature, SigningKey};
use serde::{de::DeserializeOwned, Serialize};
use std::collections::BTreeMap;
use zeroize::Zeroizing;

pub(crate) fn generate_key() -> String {
    crypto::b64(&SigningKey::random(&mut rand::rngs::OsRng).to_bytes())
}
pub(crate) fn public_jwk(private: &str) -> crate::ImResult<serde_json::Value> {
    let bytes = crypto::decode(private)?;
    let key = SigningKey::from_slice(&bytes).map_err(|_| failure("dpop_key_unavailable", false))?;
    let point = key.verifying_key().to_encoded_point(false);
    Ok(
        serde_json::json!({"kty":"EC","crv":"P-256","x":crypto::b64(point.x().ok_or_else(||failure("dpop_key_unavailable",false))?),"y":crypto::b64(point.y().ok_or_else(||failure("dpop_key_unavailable",false))?)}),
    )
}
pub(crate) fn jkt(private: &str) -> crate::ImResult<String> {
    Ok(crypto::hash(&crypto::canonical(&public_jwk(private)?)?))
}
pub(crate) fn proof(
    session: &Session,
    method: &str,
    url: &str,
    body: &[u8],
    token: Option<&str>,
    ik: Option<&str>,
) -> crate::ImResult<String> {
    let url = reqwest::Url::parse(url).map_err(|_| failure("untrusted_endpoint", false))?;
    if url.query().is_some() || url.fragment().is_some() {
        return Err(failure("untrusted_endpoint", false));
    }
    let origin = url.origin().ascii_serialization();
    let nonce = session
        .nonces
        .get(&origin)
        .cloned()
        .unwrap_or_else(|| crypto::b64(&crypto::random32()[..]));
    let mut payload = serde_json::json!({"jti":uuid::Uuid::new_v4().to_string(),"htm":method,"htu":url.as_str(),"iat":crypto::now(),"nonce":nonce,"awiki_bht":crypto::hash(body)});
    if let Some(token) = token {
        payload["ath"] = serde_json::Value::String(crypto::hash(token.as_bytes()));
    }
    if let Some(ik) = ik {
        payload["awiki_ik"] = serde_json::Value::String(ik.into());
    }
    let header = serde_json::json!({"typ":"dpop+jwt","alg":"ES256","jwk":public_jwk(&session.dpop_private)?});
    let input = format!(
        "{}.{}",
        crypto::b64(&crypto::canonical(&header)?),
        crypto::b64(&crypto::canonical(&payload)?)
    );
    let bytes = crypto::decode(&session.dpop_private)?;
    let key = SigningKey::from_slice(&bytes).map_err(|_| failure("dpop_key_unavailable", false))?;
    let signature: Signature = key.sign(input.as_bytes());
    Ok(format!("{input}.{}", crypto::b64(&signature.to_bytes())))
}
/// Store before sending. A caller may retry this logical step only with identical bytes.
pub(crate) fn prepare(
    store: &Store,
    session: &mut Session,
    key: &str,
    method: &str,
    url: &str,
    body: Vec<u8>,
) -> crate::ImResult<ExactRequest> {
    let body = Zeroizing::new(body);
    if let Some(exact) = session.exact_requests.get(key) {
        if exact.method != method
            || exact.url != url
            || crypto::decode(&exact.body_b64u)?.as_slice() != body.as_slice()
        {
            return Err(failure("idempotency_conflict", false));
        }
        return Ok(exact.clone());
    }
    let exact = ExactRequest {
        method: method.into(),
        operation_id: key
            .split(':')
            .next()
            .filter(|id| uuid::Uuid::parse_str(id).is_ok())
            .map(str::to_owned),
        url: url.into(),
        body_b64u: crypto::b64(&body),
        idempotency_key: uuid::Uuid::new_v4().to_string(),
    };
    session.exact_requests.insert(key.into(), exact.clone());
    store.save_session(session)?;
    Ok(exact)
}
pub(crate) async fn send(
    core: &crate::ImCore,
    store: &Store,
    session: &mut Session,
    exact: &ExactRequest,
    token: Option<&str>,
    realm: bool,
) -> crate::ImResult<(u16, BTreeMap<String, String>, Zeroizing<Vec<u8>>)> {
    if session.config.exp <= crypto::now() {
        let (renewed, hash) = super::fetch_config(core, &session.tenant_id).await?;
        if hash != session.config_hash {
            return Err(failure("binding_changed", false));
        }
        session.config = renewed;
        store.save_session(session)?;
    }
    crypto::validate_url(
        &exact.url,
        core.inner()
            .external_http_allow_insecure_loopback_for_testing(),
    )?;
    let body = crypto::decode(&exact.body_b64u)?;
    let mut transient_retry = 0;
    let mut nonce_resent = false;
    for _send_attempt in 0..3 {
        let mut headers = base_headers(core, realm);
        headers.insert(
            "DPoP".into(),
            proof(
                session,
                &exact.method,
                &exact.url,
                &body,
                token,
                if exact.method == "GET" {
                    None
                } else {
                    Some(&exact.idempotency_key)
                },
            )?,
        );
        if let Some(token) = token {
            headers.insert("Authorization".into(), format!("DPoP {token}"));
        }
        if exact.method != "GET" {
            headers.insert("Idempotency-Key".into(), exact.idempotency_key.clone());
        }
        let response = match core
            .network()
            .send(&exact.method, &exact.url, headers, body.to_vec(), false)
            .await
        {
            Ok(r) => r,
            Err(_e) if transient_retry == 0 => {
                transient_retry += 1;
                continue;
            }
            Err(_) => return Err(failure("network_unavailable", true)),
        };
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(k, v)| {
                v.to_str()
                    .ok()
                    .map(|v| (k.as_str().to_owned(), v.to_owned()))
            })
            .collect::<BTreeMap<_, _>>();
        if response
            .content_length()
            .is_some_and(|size| size > if realm { 8192 } else { 32768 })
        {
            return Err(failure("response_too_large", false));
        }
        let mut response = response;
        let mut bytes = Zeroizing::new(Vec::new());
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| failure("network_unavailable", true))?
        {
            bytes.extend_from_slice(&chunk);
            if bytes.len() > if realm { 8192 } else { 32768 } {
                return Err(failure("response_too_large", false));
            }
        }
        if let Some(nonce) = headers.get("dpop-nonce") {
            if crypto::decode(nonce)?.len() != 32 {
                return Err(failure("invalid_nonce", false));
            }
            let origin = reqwest::Url::parse(&exact.url)
                .map_err(|_| failure("untrusted_endpoint", false))?
                .origin()
                .ascii_serialization();
            session.nonces.insert(origin, nonce.clone());
            store.save_session(session)?;
        }
        if status == 401
            && !nonce_resent
            && headers.contains_key("dpop-nonce")
            && serde_json::from_slice::<serde_json::Value>(&bytes)
                .ok()
                .and_then(|v| {
                    v.get("error")
                        .and_then(|e| e.get("code"))
                        .and_then(|c| c.as_str())
                        .map(|c| c == "use_dpop_nonce")
                })
                .unwrap_or(false)
        {
            nonce_resent = true;
            continue;
        }
        if (300..400).contains(&status) {
            return Err(failure("redirect_rejected", false));
        }
        return Ok((status, headers, bytes));
    }
    Err(failure("network_unavailable", true))
}
pub(crate) async fn request<T: Serialize, R: DeserializeOwned>(
    core: &crate::ImCore,
    store: &Store,
    session: &mut Session,
    key: &str,
    method: &str,
    suffix: &str,
    body: Option<&T>,
    token: Option<&str>,
) -> crate::ImResult<R> {
    let url = format!(
        "{}/{}",
        session.config.control_base_url.trim_end_matches('/'),
        suffix.trim_start_matches('/')
    );
    let raw = match body {
        Some(b) => crypto::canonical(b)?,
        None => Vec::new(),
    };
    let exact = prepare(store, session, key, method, &url, raw)?;
    let (status, headers, bytes) = send(core, store, session, &exact, token, false).await?;
    if !(200..300).contains(&status) {
        return Err(response_error(status, &headers, &bytes));
    }
    serde_json::from_slice(&bytes).map_err(|_| failure("invalid_wire", false))
}
pub(crate) fn response_error(
    status: u16,
    headers: &BTreeMap<String, String>,
    bytes: &[u8],
) -> crate::ImError {
    let value = serde_json::from_slice::<serde_json::Value>(bytes).ok();
    let code = value
        .as_ref()
        .and_then(|v| v.get("error"))
        .and_then(|v| v.get("code"))
        .and_then(|c| c.as_str())
        .filter(|c| {
            c.len() <= 80
                && c.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        })
        .unwrap_or("remote_rejected");
    crate::ImError::Service {
        status_code: Some(status),
        code: Some(code.into()),
        message: "PIN recovery request rejected".into(),
        data: headers
            .get("retry-after")
            .and_then(|v| v.parse::<u32>().ok())
            .map(|retry| serde_json::json!({"retry_after_seconds":retry})),
    }
}

pub(super) fn control_version_headers(core: &crate::ImCore) -> BTreeMap<String, String> {
    let mut headers = BTreeMap::new();
    if let Some(version) = core.inner().sdk_config().client_version_info.as_ref() {
        headers.insert(crate::CLIENT_VERSION_HEADER.into(), version.header_value());
    }
    headers
}
pub(super) fn base_headers(core: &crate::ImCore, realm: bool) -> BTreeMap<String, String> {
    let mut headers = if realm {
        BTreeMap::new()
    } else {
        control_version_headers(core)
    };
    headers.insert(
        "Content-Type".into(),
        if realm {
            "application/octet-stream"
        } else {
            "application/json"
        }
        .into(),
    );
    if realm {
        headers.insert("X-Juicebox-Version".into(), "0.3.7".into());
        headers.insert(
            "X-Awiki-Recovery-Protocol".into(),
            super::wire::PROTOCOL.into(),
        );
    }
    headers
}

pub(super) fn bind_operation(
    store: &Store,
    session: &mut Session,
    key: &str,
    exact: &mut ExactRequest,
    operation_id: &str,
) -> crate::ImResult<()> {
    if exact
        .operation_id
        .as_deref()
        .is_some_and(|id| id != operation_id)
    {
        return Err(failure("idempotency_conflict", false));
    }
    exact.operation_id = Some(operation_id.into());
    session
        .exact_requests
        .get_mut(key)
        .ok_or_else(|| failure("corrupt_pending", false))?
        .operation_id = Some(operation_id.into());
    store.save_session(session)
}
