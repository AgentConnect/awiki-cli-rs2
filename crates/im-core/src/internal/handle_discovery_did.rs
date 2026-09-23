//! Discover a foreign Handle when the home Directory has no reverse DID hint.
use super::*;

pub(crate) fn foreign_binding_from_did<T: crate::internal::transport::RpcTransport>(
    client: &crate::core::ImClient,
    transport: &mut T,
    did: &str,
) -> crate::ImResult<Value> {
    let mut discovery = DirectoryDiscovery(transport);
    let document =
        crate::internal::discovery::did_document::resolve_did_document(&mut discovery, did)?;
    let (handle, url) = foreign_handle_endpoint(client, &document)?;
    let raw = fetch_public_binding_document(&mut discovery, &handle, &url)?;
    checked_binding(did, &handle, &raw)
}

pub(crate) async fn foreign_binding_from_did_async<
    T: crate::internal::transport::AsyncRpcTransport,
>(
    client: &crate::core::ImClient,
    transport: &mut T,
    did: &str,
) -> crate::ImResult<Value> {
    let mut discovery = DirectoryDiscovery(transport);
    let document =
        crate::internal::discovery::did_document::resolve_did_document_async(&mut discovery, did)
            .await?;
    let (handle, url) = foreign_handle_endpoint(client, &document)?;
    let raw = fetch_public_binding_document_async(&mut discovery, &handle, &url).await?;
    checked_binding(did, &handle, &raw)
}

fn foreign_handle_endpoint(
    client: &crate::core::ImClient,
    document: &Value,
) -> crate::ImResult<(String, String)> {
    let services = anp::wns::extract_handle_service_from_did_document(document);
    if services.len() != 1 {
        return Err(authority_conflict(
            "DID must declare one unambiguous Handle service",
        ));
    }
    let endpoint = services[0]
        .get("serviceEndpoint")
        .and_then(Value::as_str)
        .ok_or_else(|| authority_conflict("Handle service endpoint must be a URL"))?;
    let url = reqwest::Url::parse(endpoint)
        .map_err(|_| authority_conflict("Handle service endpoint is invalid"))?;
    if url.scheme() != "https"
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(authority_conflict(
            "Handle service endpoint must be canonical HTTPS",
        ));
    }
    let local = url
        .path()
        .strip_prefix("/.well-known/handle/")
        .filter(|s| !s.is_empty() && !s.contains('/') && !s.contains('%'))
        .ok_or_else(|| authority_conflict("Handle service endpoint path is invalid"))?;
    let host = url
        .host_str()
        .ok_or_else(|| authority_conflict("Handle service endpoint host is missing"))?;
    if host
        .trim_matches(['[', ']'])
        .parse::<std::net::IpAddr>()
        .is_ok()
        || !host.contains('.')
        || host.ends_with(".localhost")
        || host.ends_with(".local")
    {
        return Err(authority_conflict(
            "Handle service requires a public Provider domain",
        ));
    }
    let handle = normalize_handle(&format!("{local}.{host}"))?;
    validate_did_matches_handle_domain(
        &handle.full_handle,
        document
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    )?;
    let HandleResolutionRoute::Public {
        full_handle,
        url: expected,
    } = resolution_route_for_client(client, &handle.full_handle)?
    else {
        return Err(authority_conflict(
            "Home Handle requires its local Directory binding",
        ));
    };
    if url.as_str() != expected {
        return Err(authority_conflict(
            "Handle service endpoint does not match its Handle",
        ));
    }
    Ok((full_handle, expected))
}

fn checked_binding(did: &str, handle: &str, raw: &Value) -> crate::ImResult<Value> {
    let value = public_directory_value(handle, raw)?;
    if value.get("did").and_then(Value::as_str) != Some(did) {
        return Err(authority_conflict(
            "Public Handle binding returned a different DID",
        ));
    }
    Ok(value)
}

#[cfg(all(test, feature = "sqlite"))]
#[path = "handle_discovery_did_tests.rs"]
mod tests;
