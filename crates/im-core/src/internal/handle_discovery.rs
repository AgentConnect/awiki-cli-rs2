//! Handle discovery routes and validation.
//!
//! Public WNS documents authorize a permanent full Handle, current DID,
//! status, and binding generation. Cross-domain identity deliberately does not
//! treat deployment-private account identifiers in that document as
//! authoritative.

use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DirectHandleResolution {
    pub(crate) target_did: String,
    pub(crate) full_handle: String,
    pub(crate) authority_subject_id: String,
}

impl DirectHandleResolution {
    pub(crate) fn peer_scope(
        &self,
    ) -> crate::ImResult<crate::internal::local_state::owner_scope::DirectPeerScope> {
        crate::internal::local_state::owner_scope::DirectPeerScope::new(
            self.authority_subject_id.clone(),
            self.full_handle.clone(),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PublicHandleBinding {
    handle: crate::ids::Handle,
    did: crate::ids::Did,
    domain: String,
    status: String,
    binding_generation: String,
}

pub(crate) fn resolve_direct_handle(
    client: &crate::core::ImClient,
    raw_handle: &str,
) -> crate::ImResult<DirectHandleResolution> {
    match resolution_route_for_client(client, raw_handle)? {
        HandleResolutionRoute::Local { full_handle } => {
            let lookup = client
                .directory()
                .lookup_handle(crate::ids::Handle::parse(full_handle.as_str(), "")?)?;
            resolution_from_lookup(full_handle.as_str(), lookup)
        }
        HandleResolutionRoute::Public { full_handle, url } => {
            let mut transport = crate::internal::transport::CoreHttpTransport::new(client);
            let raw = fetch_public_binding_document(&mut transport, &full_handle, url.as_str())?;
            finish_public_direct_resolution(client, full_handle.as_str(), raw)
        }
    }
}

pub(crate) async fn resolve_direct_handle_async(
    client: &crate::core::ImClient,
    raw_handle: &str,
) -> crate::ImResult<DirectHandleResolution> {
    match resolution_route_for_client(client, raw_handle)? {
        HandleResolutionRoute::Local { full_handle } => {
            let lookup = client
                .directory()
                .lookup_handle_async(crate::ids::Handle::parse(full_handle.as_str(), "")?)
                .await?;
            resolution_from_lookup(full_handle.as_str(), lookup)
        }
        HandleResolutionRoute::Public { full_handle, url } => {
            let mut transport = crate::internal::transport::CoreHttpTransport::new(client);
            let raw =
                fetch_public_binding_document_async(&mut transport, &full_handle, url.as_str())
                    .await?;
            finish_public_direct_resolution_async(client, full_handle.as_str(), raw).await
        }
    }
}

fn finish_public_direct_resolution(
    _client: &crate::core::ImClient,
    full_handle: &str,
    raw: Value,
) -> crate::ImResult<DirectHandleResolution> {
    let lookup = authoritative_lookup_from_public_document(full_handle, &raw)?;
    let resolved = resolution_from_lookup(full_handle, lookup.clone())?;
    #[cfg(feature = "sqlite")]
    crate::directory::project_handle_lookup(_client, &lookup)?;
    Ok(resolved)
}

async fn finish_public_direct_resolution_async(
    _client: &crate::core::ImClient,
    full_handle: &str,
    raw: Value,
) -> crate::ImResult<DirectHandleResolution> {
    let lookup = authoritative_lookup_from_public_document(full_handle, &raw)?;
    let resolved = resolution_from_lookup(full_handle, lookup.clone())?;
    #[cfg(feature = "sqlite")]
    crate::directory::project_handle_lookup_async(_client, &lookup).await?;
    Ok(resolved)
}

/// Recovery and device Join can inspect the public binding before an identity has
/// been projected locally. This uses the same WNS authority and validation as
/// an existing client, without requiring an authenticated local credential.
pub(crate) async fn resolve_authoritative_recovery_binding_async(
    core: &crate::core::ImCore,
    raw_handle: &str,
) -> crate::ImResult<crate::directory::HandleLookupResult> {
    let config = core.inner().sdk_config();
    let handle = normalize_handle_with_default_domain(raw_handle, config.did_domain.as_str())?;
    let url = authoritative_discovery_url(config, &handle);
    let mut transport = crate::internal::transport::CorePlainTransport::new(core);
    let raw =
        fetch_public_binding_document_async(&mut transport, &handle.full_handle, &url).await?;
    authoritative_lookup_from_public_document(&handle.full_handle, &raw)
}

pub(crate) async fn resolve_authoritative_handle_binding_async(
    client: &crate::core::ImClient,
    raw_handle: &str,
) -> crate::ImResult<crate::directory::HandleLookupResult> {
    let normalized = normalize_handle_for_client(client, raw_handle)?;
    let url = authoritative_discovery_url_for_client(client, &normalized);
    let mut transport = crate::internal::transport::CoreHttpTransport::new(client);
    let raw =
        fetch_public_binding_document_async(&mut transport, &normalized.full_handle, &url).await?;
    authoritative_lookup_from_public_document(&normalized.full_handle, &raw)
}

pub(crate) async fn resolve_authoritative_direct_rebind_async(
    client: &crate::core::ImClient,
    raw_handle: &str,
) -> crate::ImResult<crate::directory::HandleLookupResult> {
    match resolution_route_for_client(client, raw_handle)? {
        HandleResolutionRoute::Local { full_handle } => {
            let lookup = crate::internal::directory_runtime::DirectoryRuntime::new(
                client,
                crate::internal::transport::CoreHttpTransport::new(client),
            )
            .lookup_handle_async(crate::ids::Handle::parse(&full_handle, "")?)
            .await?;
            let normalized = normalize_handle(&full_handle)?;
            let url = authoritative_discovery_url_for_client(client, &normalized);
            let mut transport = crate::internal::transport::CoreHttpTransport::new(client);
            let raw =
                fetch_public_binding_document_async(&mut transport, &full_handle, &url).await?;
            merge_local_directory_with_public_binding(&full_handle, lookup, &raw)
        }
        HandleResolutionRoute::Public { full_handle, url } => {
            let mut transport = crate::internal::transport::CoreHttpTransport::new(client);
            let raw =
                fetch_public_binding_document_async(&mut transport, &full_handle, &url).await?;
            authoritative_lookup_from_public_document(&full_handle, &raw)
        }
    }
}

pub(crate) fn resolve_authoritative_handle_binding(
    client: &crate::core::ImClient,
    raw_handle: &str,
) -> crate::ImResult<crate::directory::HandleLookupResult> {
    let normalized = normalize_handle_for_client(client, raw_handle)?;
    let url = authoritative_discovery_url_for_client(client, &normalized);
    let mut transport = crate::internal::transport::CoreHttpTransport::new(client);
    let raw = fetch_public_binding_document(&mut transport, &normalized.full_handle, &url)?;
    authoritative_lookup_from_public_document(&normalized.full_handle, &raw)
}

pub(crate) fn resolve_authoritative_direct_rebind(
    client: &crate::core::ImClient,
    raw_handle: &str,
) -> crate::ImResult<crate::directory::HandleLookupResult> {
    match resolution_route_for_client(client, raw_handle)? {
        HandleResolutionRoute::Local { full_handle } => {
            let lookup = crate::internal::directory_runtime::DirectoryRuntime::new(
                client,
                crate::internal::transport::CoreHttpTransport::new(client),
            )
            .lookup_handle(crate::ids::Handle::parse(&full_handle, "")?)?;
            let normalized = normalize_handle(&full_handle)?;
            let url = authoritative_discovery_url_for_client(client, &normalized);
            let mut transport = crate::internal::transport::CoreHttpTransport::new(client);
            let raw = fetch_public_binding_document(&mut transport, &full_handle, &url)?;
            merge_local_directory_with_public_binding(&full_handle, lookup, &raw)
        }
        HandleResolutionRoute::Public { full_handle, url } => {
            let mut transport = crate::internal::transport::CoreHttpTransport::new(client);
            let raw = fetch_public_binding_document(&mut transport, &full_handle, &url)?;
            authoritative_lookup_from_public_document(&full_handle, &raw)
        }
    }
}

// Directory transports expose anonymous discovery separately from authenticated RPC.
struct DirectoryDiscovery<'a, T>(&'a mut T);

impl<T: crate::internal::transport::RpcTransport> crate::internal::transport::RawJsonTransport
    for DirectoryDiscovery<'_, T>
{
    fn resolve_web_document(&mut self, did: &str) -> crate::ImResult<Value> {
        self.0.directory_resolve_web_document(did)
    }

    fn get_json_url(
        &mut self,
        url: &str,
        headers: BTreeMap<String, String>,
    ) -> crate::ImResult<Value> {
        self.0.directory_get_json_url(url, headers)
    }
}

impl<T: crate::internal::transport::AsyncRpcTransport>
    crate::internal::transport::AsyncRawJsonTransport for DirectoryDiscovery<'_, T>
{
    async fn resolve_web_document(&mut self, did: &str) -> crate::ImResult<Value> {
        self.0.directory_resolve_web_document(did).await
    }

    async fn get_json_url(
        &mut self,
        url: &str,
        headers: BTreeMap<String, String>,
    ) -> crate::ImResult<Value> {
        self.0.directory_get_json_url(url, headers).await
    }
}

/// None means the authenticated home Directory remains the authority.
pub(crate) fn foreign_directory_lookup<T: crate::internal::transport::RpcTransport>(
    client: &crate::core::ImClient,
    transport: &mut T,
    handle: &str,
) -> crate::ImResult<Option<Value>> {
    match resolution_route_for_client(client, handle)? {
        HandleResolutionRoute::Local { .. } => Ok(None),
        HandleResolutionRoute::Public { full_handle, url } => {
            let raw = fetch_public_binding_document(
                &mut DirectoryDiscovery(transport),
                &full_handle,
                &url,
            )?;
            Ok(Some(public_directory_value(&full_handle, &raw)?))
        }
    }
}

pub(crate) async fn foreign_directory_lookup_async<
    T: crate::internal::transport::AsyncRpcTransport,
>(
    client: &crate::core::ImClient,
    transport: &mut T,
    handle: &str,
) -> crate::ImResult<Option<Value>> {
    match resolution_route_for_client(client, handle)? {
        HandleResolutionRoute::Local { .. } => Ok(None),
        HandleResolutionRoute::Public { full_handle, url } => {
            let raw = fetch_public_binding_document_async(
                &mut DirectoryDiscovery(transport),
                &full_handle,
                &url,
            )
            .await?;
            Ok(Some(public_directory_value(&full_handle, &raw)?))
        }
    }
}

fn public_directory_value(handle: &str, raw: &Value) -> crate::ImResult<Value> {
    let lookup = authoritative_lookup_from_public_document(handle, raw)?;
    // Only verified fields enter the identity projection. Provider-private IDs
    // and the home Directory's foreign profile cannot become authority input.
    Ok(serde_json::json!({
        "handle": lookup.handle.as_str(), "did": lookup.did.as_str(),
        "user_id": lookup.user_id, "domain": lookup.domain,
        "status": lookup.status, "binding_generation": lookup.binding_generation,
    }))
}

fn fetch_public_binding_document<T>(
    transport: &mut T,
    handle: &str,
    url: &str,
) -> crate::ImResult<Value>
where
    T: crate::internal::transport::RawJsonTransport,
{
    let raw = transport.get_json_url(url, BTreeMap::new())?;
    let binding = public_handle_binding_from_value(handle, &raw)?;
    if binding.did.as_str().starts_with("did:web:") {
        let document = crate::internal::discovery::did_document::resolve_did_document(
            transport,
            binding.did.as_str(),
        )?;
        validate_web_handle_provider(&binding, &document)?;
    }
    Ok(raw)
}

async fn fetch_public_binding_document_async<T>(
    transport: &mut T,
    handle: &str,
    url: &str,
) -> crate::ImResult<Value>
where
    T: crate::internal::transport::AsyncRawJsonTransport,
{
    let raw = transport.get_json_url(url, BTreeMap::new()).await?;
    let binding = public_handle_binding_from_value(handle, &raw)?;
    if binding.did.as_str().starts_with("did:web:") {
        let document = crate::internal::discovery::did_document::resolve_did_document_async(
            transport,
            binding.did.as_str(),
        )
        .await?;
        validate_web_handle_provider(&binding, &document)?;
    }
    Ok(raw)
}

fn validate_web_handle_provider(
    binding: &PublicHandleBinding,
    document: &Value,
) -> crate::ImResult<()> {
    // The full Handle supplies continuity; its Provider can differ from the
    // Web DID host. The DID must independently declare that same Provider.
    let verified = anp::wns::extract_handle_service_from_did_document(document)
        .iter()
        .filter_map(|service| service.get("serviceEndpoint").and_then(Value::as_str))
        .filter_map(|endpoint| reqwest::Url::parse(endpoint).ok())
        .any(|endpoint| {
            endpoint.scheme() == "https"
                && endpoint
                    .host_str()
                    .is_some_and(|host| host.eq_ignore_ascii_case(&binding.domain))
                && endpoint.port_or_known_default() == Some(443)
                && endpoint.username().is_empty()
                && endpoint.password().is_none()
        });
    if !verified {
        return Err(authority_conflict(
            "Web DID document does not confirm the Handle Provider domain",
        ));
    }
    Ok(())
}

fn merge_local_directory_with_public_binding(
    expected_handle: &str,
    mut lookup: crate::directory::HandleLookupResult,
    public_document: &Value,
) -> crate::ImResult<crate::directory::HandleLookupResult> {
    validate_handle_match(expected_handle, lookup.handle.as_str())?;
    let persona = lookup.peer_persona()?;
    let public = public_handle_binding_from_value(expected_handle, public_document)?;

    if persona.full_handle != public.handle.as_str() {
        return Err(authority_conflict(
            "local Directory and public WNS returned different Handles",
        ));
    }
    if persona.authority_namespace != public.domain {
        return Err(authority_conflict(
            "local Directory and public WNS returned different provider domains",
        ));
    }
    if lookup.did != public.did {
        return Err(authority_conflict(
            "local Directory and public WNS returned different current DIDs",
        ));
    }
    if let Some(directory_generation) = lookup.binding_generation.as_deref() {
        crate::internal::local_state::sync_v2::validate_positive_decimal(
            "binding_generation",
            directory_generation,
        )?;
        if directory_generation != public.binding_generation {
            return Err(authority_conflict(
                "local Directory and public WNS returned different binding generations",
            ));
        }
    }
    lookup.binding_generation = Some(public.binding_generation);
    Ok(lookup)
}

fn authority_conflict(detail: &str) -> crate::ImError {
    crate::ImError::IdentityBindingConflict {
        detail: detail.to_owned(),
    }
}

fn resolution_from_lookup(
    expected_handle: &str,
    lookup: crate::directory::HandleLookupResult,
) -> crate::ImResult<DirectHandleResolution> {
    let persona = lookup.peer_persona()?;
    let full_handle = persona.full_handle;
    validate_handle_match(expected_handle, full_handle.as_str())?;
    Ok(DirectHandleResolution {
        target_did: lookup.did.as_str().to_owned(),
        full_handle,
        authority_subject_id: persona.authority_subject_id,
    })
}

#[cfg(test)]
fn resolution_from_public_document(
    expected_handle: &str,
    raw: Value,
) -> crate::ImResult<DirectHandleResolution> {
    let binding = public_handle_binding_from_value(expected_handle, &raw)?;
    let full_handle = binding.handle.as_str().to_owned();
    // A full Handle is permanently reserved by its provider. It is therefore
    // the cross-domain authority subject; provider-private account IDs are not
    // part of WNS and must not affect the Persona or conversation scope.
    let authority_subject_id = full_handle.clone();
    let persona = crate::internal::canonical_identity::PeerPersona::from_verified_handle(
        &binding.domain,
        &authority_subject_id,
        &full_handle,
        Some(&binding.status),
    )?;
    Ok(DirectHandleResolution {
        target_did: binding.did.as_str().to_owned(),
        full_handle: persona.full_handle,
        authority_subject_id: persona.authority_subject_id,
    })
}

fn authoritative_lookup_from_public_document(
    expected_handle: &str,
    raw: &Value,
) -> crate::ImResult<crate::directory::HandleLookupResult> {
    let binding = public_handle_binding_from_value(expected_handle, raw)?;
    let authority_subject_id = binding.handle.as_str().to_owned();
    Ok(crate::directory::HandleLookupResult {
        handle: binding.handle,
        did: binding.did,
        user_id: authority_subject_id,
        domain: Some(binding.domain),
        status: Some(binding.status),
        binding_generation: Some(binding.binding_generation),
        profile: None,
        warnings: Vec::new(),
    })
}

fn public_handle_binding_from_value(
    expected_handle: &str,
    raw: &Value,
) -> crate::ImResult<PublicHandleBinding> {
    let expected = normalize_handle(expected_handle)?;
    let status = string_field(raw, "status")?;
    validate_active_status(&expected.full_handle, &status)?;
    // `handle` is the ANP-04 field. `full_handle` and provider-private subject
    // identifiers are intentionally ignored on the public WNS boundary.
    let handle = string_field(raw, "handle")?;
    validate_handle_match(&expected.full_handle, &handle)?;
    let normalized = normalize_handle(&handle)?;
    let did = crate::ids::Did::parse(&string_field(raw, "did")?)?;
    validate_did_matches_handle_domain(&normalized.full_handle, did.as_str())?;
    let generation = raw
        .get("binding_generation")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            crate::ImError::invalid_input(
                Some("binding_generation".to_owned()),
                "public WNS document requires a canonical positive decimal binding_generation",
            )
        })?;
    let binding_generation = anp::wns::BindingGeneration::new(generation.to_owned())
        .map_err(|_| {
            crate::ImError::invalid_input(
                Some("binding_generation".to_owned()),
                "public WNS document requires a canonical positive decimal binding_generation",
            )
        })?
        .to_string();
    Ok(PublicHandleBinding {
        handle: crate::ids::Handle::parse(&normalized.full_handle, "")?,
        did,
        domain: normalized.domain,
        status,
        binding_generation,
    })
}

fn validate_active_status(handle: &str, status: &str) -> crate::ImResult<()> {
    if status.trim().eq_ignore_ascii_case("active") {
        return Ok(());
    }
    Err(crate::ImError::PeerNotFound {
        peer: handle.to_owned(),
    })
}

fn validate_handle_match(expected: &str, actual: &str) -> crate::ImResult<()> {
    let expected = normalize_handle(expected)?;
    let actual = normalize_handle(actual)?;
    if expected.full_handle == actual.full_handle {
        return Ok(());
    }
    Err(crate::ImError::InvalidInput {
        field: Some("handle".to_owned()),
        message: format!(
            "handle discovery returned {} for {}",
            actual.full_handle, expected.full_handle
        ),
    })
}

fn validate_did_matches_handle_domain(handle: &str, did: &str) -> crate::ImResult<()> {
    let normalized = normalize_handle(handle)?;
    if did.starts_with("did:web:") {
        anp::authentication::build_did_web_resolution_url(did).map_err(|_| {
            crate::ImError::invalid_input(
                Some("did".to_owned()),
                "handle discovery returned an invalid Web DID",
            )
        })?;
        // The network boundary additionally checks the reverse Provider declaration.
        return Ok(());
    }
    let Some(did_domain) = did_wba_domain(did) else {
        return Err(crate::ImError::InvalidInput {
            field: Some("did".to_owned()),
            message: format!("handle discovery DID {did} uses an unsupported DID method"),
        });
    };
    if did_domain == normalized.domain {
        return Ok(());
    }
    Err(crate::ImError::InvalidInput {
        field: Some("did".to_owned()),
        message: format!(
            "handle discovery DID domain {did_domain} does not match handle domain {}",
            normalized.domain
        ),
    })
}

pub(crate) fn is_local_handle(client: &crate::core::ImClient, full_handle: &str) -> bool {
    let Ok(normalized) = normalize_handle(full_handle) else {
        return false;
    };
    let local_domain = normalize_domain(client.core_inner().sdk_config().did_domain.as_str());
    normalized.domain == local_domain
}

fn normalize_handle_for_client(
    client: &crate::core::ImClient,
    raw: &str,
) -> crate::ImResult<NormalizedHandle> {
    normalize_handle_with_default_domain(raw, client.core_inner().sdk_config().did_domain.as_str())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum HandleResolutionRoute {
    Local { full_handle: String },
    Public { full_handle: String, url: String },
}

fn resolution_route_for_client(
    client: &crate::core::ImClient,
    raw_handle: &str,
) -> crate::ImResult<HandleResolutionRoute> {
    let normalized = normalize_handle_for_client(client, raw_handle)?;
    if is_local_handle(client, normalized.full_handle.as_str()) {
        return Ok(HandleResolutionRoute::Local {
            full_handle: normalized.full_handle,
        });
    }
    Ok(HandleResolutionRoute::Public {
        url: discovery_url(normalized.domain.as_str(), normalized.local_part.as_str()),
        full_handle: normalized.full_handle,
    })
}

fn discovery_url(domain: &str, local_part: &str) -> String {
    format!(
        "https://{}/.well-known/handle/{}",
        domain.trim().trim_end_matches('.'),
        percent_encode_path_segment(local_part)
    )
}

fn authoritative_discovery_url_for_client(
    client: &crate::core::ImClient,
    handle: &NormalizedHandle,
) -> String {
    authoritative_discovery_url(client.core_inner().sdk_config(), handle)
}

fn authoritative_discovery_url(config: &crate::ImCoreConfig, handle: &NormalizedHandle) -> String {
    if handle
        .domain
        .eq_ignore_ascii_case(config.did_domain.as_str())
    {
        let configured_base = config
            .user_service_endpoint
            .as_ref()
            .unwrap_or(&config.service_base_url)
            .as_str()
            .trim_end_matches('/');
        if is_loopback_http_base(configured_base) {
            return format!(
                "{configured_base}/.well-known/handle/{}",
                percent_encode_path_segment(&handle.local_part)
            );
        }
    }
    discovery_url(&handle.domain, &handle.local_part)
}

fn is_loopback_http_base(base: &str) -> bool {
    let Some(rest) = base.strip_prefix("http://") else {
        return false;
    };
    let authority = rest.split('/').next().unwrap_or_default();
    let host = if let Some(bracketed) = authority.strip_prefix('[') {
        bracketed.split(']').next().unwrap_or_default()
    } else {
        authority.split(':').next().unwrap_or_default()
    };
    host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1" || host == "::1"
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalizedHandle {
    full_handle: String,
    local_part: String,
    domain: String,
}

fn normalize_handle(raw: &str) -> crate::ImResult<NormalizedHandle> {
    normalize_handle_with_default_domain(raw, "")
}

fn normalize_handle_with_default_domain(
    raw: &str,
    default_domain: &str,
) -> crate::ImResult<NormalizedHandle> {
    let handle = crate::ids::Handle::parse(raw.trim(), default_domain)?;
    let full_handle = handle
        .as_str()
        .trim()
        .trim_start_matches('@')
        .to_ascii_lowercase();
    let (local_part, domain) =
        full_handle
            .split_once('.')
            .ok_or_else(|| crate::ImError::InvalidInput {
                field: Some("handle".to_owned()),
                message: "cross-domain handle must include a domain".to_owned(),
            })?;
    if local_part.trim().is_empty() || domain.trim().is_empty() {
        return Err(crate::ImError::InvalidInput {
            field: Some("handle".to_owned()),
            message: "handle must include local part and domain".to_owned(),
        });
    }
    let local_part = local_part.to_owned();
    let domain = normalize_domain(domain);
    Ok(NormalizedHandle {
        full_handle,
        local_part,
        domain,
    })
}

fn normalize_domain(raw: &str) -> String {
    raw.trim().trim_end_matches('.').to_ascii_lowercase()
}

fn did_wba_domain(did: &str) -> Option<String> {
    did.strip_prefix("did:wba:")
        .and_then(|rest| rest.split(':').next())
        .map(normalize_domain)
        .filter(|domain| !domain.is_empty())
}

fn string_field(value: &Value, key: &str) -> crate::ImResult<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| crate::ImError::PeerNotFound {
            peer: format!("handle discovery missing {key}"),
        })
}

fn first_string_field(value: &Value, keys: &[&str]) -> crate::ImResult<String> {
    first_non_empty_string(value, keys).ok_or_else(|| crate::ImError::PeerNotFound {
        peer: "handle discovery missing handle".to_owned(),
    })
}

fn first_non_empty_string(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|key| value.get(key).and_then(Value::as_str))
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn percent_encode_path_segment(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.as_bytes() {
        let ch = *byte as char;
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '~') {
            encoded.push(ch);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn normalize_handle_splits_local_part_and_domain() {
        let handle = super::normalize_handle(" Alice.AWiki.Info ").unwrap();

        assert_eq!(handle.full_handle, "alice.awiki.info");
        assert_eq!(handle.local_part, "alice");
        assert_eq!(handle.domain, "awiki.info");
    }

    #[test]
    fn normalize_handle_for_client_expands_bare_local_handle() {
        let fixture = Fixture::new("bare-local-handle");
        let client = fixture.client();

        let handle = super::normalize_handle_for_client(&client, "Alice").unwrap();

        assert_eq!(handle.full_handle, "alice.awiki.test");
        assert_eq!(handle.local_part, "alice");
        assert_eq!(handle.domain, "awiki.test");
    }

    #[test]
    fn route_for_client_keeps_local_handles_on_local_rpc() {
        let fixture = Fixture::new("local-route");
        let client = fixture.client();

        let route = super::resolution_route_for_client(&client, "Alice").unwrap();

        assert_eq!(
            route,
            super::HandleResolutionRoute::Local {
                full_handle: "alice.awiki.test".to_owned(),
            }
        );
    }

    #[test]
    fn route_for_client_sends_remote_handles_to_public_discovery() {
        let fixture = Fixture::new("remote-route");
        let client = fixture.client();

        let route = super::resolution_route_for_client(&client, "Peer.AWiki.Info").unwrap();

        assert_eq!(
            route,
            super::HandleResolutionRoute::Public {
                full_handle: "peer.awiki.info".to_owned(),
                url: "https://awiki.info/.well-known/handle/peer".to_owned(),
            }
        );
    }

    #[test]
    fn authoritative_url_uses_handle_provider_instead_of_https_service_host() {
        let fixture = Fixture::new("authoritative-provider-route");
        let client = fixture.client();
        let handle = super::normalize_handle_for_client(&client, "Alice").unwrap();

        assert_eq!(
            super::authoritative_discovery_url_for_client(&client, &handle),
            "https://awiki.test/.well-known/handle/alice"
        );
    }

    #[test]
    fn did_wba_domain_reads_first_wba_segment() {
        assert_eq!(
            super::did_wba_domain("did:wba:Awiki.Info:user:alice:e1").as_deref(),
            Some("awiki.info")
        );
    }

    #[test]
    fn public_document_resolution_accepts_standard_four_field_document() {
        let resolved = super::resolution_from_public_document(
            "peer.awiki.info",
            json!({
                "status": "active",
                "handle": "peer.awiki.info",
                "did": "did:wba:awiki.info:user:peer:e1",
                "binding_generation": "7",
            }),
        )
        .unwrap();

        assert_eq!(resolved.full_handle, "peer.awiki.info");
        assert_eq!(resolved.target_did, "did:wba:awiki.info:user:peer:e1");
        assert_eq!(resolved.authority_subject_id, "peer.awiki.info");
        assert_eq!(resolved.peer_scope().unwrap().user_id, "peer.awiki.info");
    }

    #[test]
    fn public_document_resolution_ignores_private_subject_fields() {
        let base = json!({
            "status": "active",
            "handle": "peer.awiki.info",
            "did": "did:wba:awiki.info:user:peer:e1",
            "binding_generation": "7",
        });
        let expected =
            super::resolution_from_public_document("peer.awiki.info", base.clone()).unwrap();

        for (user_id, subject_id) in [
            ("user-one", "subject-one"),
            ("user-two", "subject-two"),
            ("conflict", "different-conflict"),
        ] {
            let mut raw = base.clone();
            raw["user_id"] = Value::String(user_id.to_owned());
            raw["subject_id"] = Value::String(subject_id.to_owned());
            raw["userId"] = Value::String("camel-user".to_owned());
            raw["subjectId"] = Value::String("camel-subject".to_owned());
            raw["full_handle"] = Value::String("ignored.internal.example".to_owned());
            assert_eq!(
                super::resolution_from_public_document("peer.awiki.info", raw).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn public_document_resolution_requires_canonical_unbounded_generation() {
        let base = json!({
            "status": "active",
            "handle": "peer.awiki.info",
            "did": "did:wba:awiki.info:user:peer:e1",
            "binding_generation": "7",
        });
        let mut large = base.clone();
        large["binding_generation"] = Value::String(format!("1{}", "0".repeat(100)));
        super::resolution_from_public_document("peer.awiki.info", large).unwrap();

        let mut missing = base.clone();
        missing
            .as_object_mut()
            .unwrap()
            .remove("binding_generation");
        assert!(super::resolution_from_public_document("peer.awiki.info", missing).is_err());

        let mut numeric = base.clone();
        numeric["binding_generation"] = json!(7);
        assert!(super::resolution_from_public_document("peer.awiki.info", numeric).is_err());

        for invalid in ["0", "07", "+7", "-1", "", " 7"] {
            let mut raw = base.clone();
            raw["binding_generation"] = Value::String(invalid.to_owned());
            assert!(super::resolution_from_public_document("peer.awiki.info", raw).is_err());
        }
    }

    #[test]
    fn public_subject_isolates_same_local_part_across_domains() {
        let awiki = super::resolution_from_public_document(
            "peer.awiki.info",
            json!({
                "status": "active",
                "handle": "peer.awiki.info",
                "did": "did:wba:awiki.info:user:peer:e1",
                "binding_generation": "7",
                "user_id": "same-private-id",
            }),
        )
        .unwrap();
        let example = super::resolution_from_public_document(
            "peer.example.com",
            json!({
                "status": "active",
                "handle": "peer.example.com",
                "did": "did:wba:example.com:user:peer:e1",
                "binding_generation": "7",
                "user_id": "same-private-id",
            }),
        )
        .unwrap();

        assert_ne!(awiki.authority_subject_id, example.authority_subject_id);
        assert_ne!(
            crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(
                &awiki.peer_scope().unwrap()
            ),
            crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(
                &example.peer_scope().unwrap()
            )
        );
    }

    #[test]
    fn same_domain_directory_resolution_keeps_internal_subject() {
        let resolved = super::resolution_from_lookup(
            "peer.awiki.test",
            crate::directory::HandleLookupResult {
                handle: crate::ids::Handle::parse("peer.awiki.test", "").unwrap(),
                did: crate::ids::Did::parse("did:wba:awiki.test:user:peer:e1").unwrap(),
                user_id: "internal-user-peer".to_owned(),
                domain: Some("awiki.test".to_owned()),
                status: Some("active".to_owned()),
                binding_generation: None,
                profile: None,
                warnings: Vec::new(),
            },
        )
        .unwrap();

        assert_eq!(resolved.authority_subject_id, "internal-user-peer");
    }

    #[test]
    fn same_domain_authoritative_binding_merges_stable_subject_with_public_generation() {
        let lookup = crate::directory::HandleLookupResult {
            handle: crate::ids::Handle::parse("peer.awiki.test", "").unwrap(),
            did: crate::ids::Did::parse("did:wba:awiki.test:user:peer:e1-new").unwrap(),
            user_id: "internal-user-peer".to_owned(),
            domain: Some("awiki.test".to_owned()),
            status: Some("active".to_owned()),
            binding_generation: None,
            profile: None,
            warnings: vec!["directory-warning".to_owned()],
        };

        let merged = super::merge_local_directory_with_public_binding(
            "peer.awiki.test",
            lookup,
            &json!({
                "status": "active",
                "handle": "peer.awiki.test",
                "did": "did:wba:awiki.test:user:peer:e1-new",
                "binding_generation": "12"
            }),
        )
        .unwrap();

        assert_eq!(merged.user_id, "internal-user-peer");
        assert_eq!(merged.binding_generation.as_deref(), Some("12"));
        assert_eq!(merged.warnings, vec!["directory-warning"]);
        assert_eq!(
            merged.peer_persona().unwrap().authority_subject_id,
            "internal-user-peer"
        );
    }

    #[test]
    fn same_domain_authoritative_binding_rejects_cross_source_mismatch() {
        let lookup = |did: &str, domain: &str, generation: Option<&str>| {
            crate::directory::HandleLookupResult {
                handle: crate::ids::Handle::parse("peer.awiki.test", "").unwrap(),
                did: crate::ids::Did::parse(did).unwrap(),
                user_id: "internal-user-peer".to_owned(),
                domain: Some(domain.to_owned()),
                status: Some("active".to_owned()),
                binding_generation: generation.map(ToOwned::to_owned),
                profile: None,
                warnings: Vec::new(),
            }
        };
        let public = json!({
            "status": "active",
            "handle": "peer.awiki.test",
            "did": "did:wba:awiki.test:user:peer:e1-new",
            "binding_generation": "12"
        });

        for directory in [
            lookup("did:wba:awiki.test:user:peer:e1-old", "awiki.test", None),
            lookup("did:wba:awiki.test:user:peer:e1-new", "other.test", None),
            lookup(
                "did:wba:awiki.test:user:peer:e1-new",
                "awiki.test",
                Some("11"),
            ),
        ] {
            assert!(matches!(
                super::merge_local_directory_with_public_binding(
                    "peer.awiki.test",
                    directory,
                    &public,
                ),
                Err(crate::ImError::IdentityBindingConflict { .. })
            ));
        }
    }

    #[test]
    fn authoritative_group_lookup_uses_only_public_binding_fields() {
        let base = json!({
            "status": "active",
            "handle": "peer.awiki.info",
            "did": "did:wba:awiki.info:user:peer:e1",
            "binding_generation": "8",
        });
        let expected =
            super::authoritative_lookup_from_public_document("peer.awiki.info", &base).unwrap();
        let mut with_conflicting_private_ids = base;
        with_conflicting_private_ids["user_id"] = Value::String("private-user".to_owned());
        with_conflicting_private_ids["subject_id"] =
            Value::String("different-private-subject".to_owned());
        let actual = super::authoritative_lookup_from_public_document(
            "peer.awiki.info",
            &with_conflicting_private_ids,
        )
        .unwrap();

        assert_eq!(actual, expected);
        assert_eq!(actual.user_id, "peer.awiki.info");
        assert_eq!(actual.binding_generation.as_deref(), Some("8"));
    }

    #[test]
    fn public_document_resolution_rejects_handle_mismatch() {
        let err = super::resolution_from_public_document(
            "peer.awiki.info",
            json!({
                "status": "active",
                "handle": "mallory.awiki.info",
                "did": "did:wba:awiki.info:user:peer:e1",
            }),
        )
        .unwrap_err();

        assert!(matches!(
            err,
            crate::ImError::InvalidInput {
                field: Some(ref field),
                ..
            } if field == "handle"
        ));
    }

    #[test]
    fn public_document_resolution_rejects_inactive_status() {
        let err = super::resolution_from_public_document(
            "peer.awiki.info",
            json!({
                "status": "revoked",
                "handle": "peer.awiki.info",
                "did": "did:wba:awiki.info:user:peer:e1",
            }),
        )
        .unwrap_err();

        assert!(matches!(
            err,
            crate::ImError::PeerNotFound { ref peer } if peer == "peer.awiki.info"
        ));
    }

    #[test]
    fn public_document_resolution_rejects_did_domain_mismatch() {
        let err = super::resolution_from_public_document(
            "peer.awiki.info",
            json!({
                "status": "active",
                "handle": "peer.awiki.info",
                "did": "did:wba:rwiki.cn:user:peer:e1",
            }),
        )
        .unwrap_err();

        assert!(matches!(
            err,
            crate::ImError::InvalidInput {
                field: Some(ref field),
                ..
            } if field == "did"
        ));
    }

    #[test]
    fn validate_did_rejects_non_wba() {
        let err =
            super::validate_did_matches_handle_domain("alice.awiki.info", "did:example:alice")
                .unwrap_err();

        assert!(matches!(
            err,
            crate::ImError::InvalidInput {
                field: Some(ref field),
                ..
            } if field == "did"
        ));
    }

    pub(super) struct Fixture {
        pub(super) root: PathBuf,
    }

    impl Fixture {
        pub(super) fn new(prefix: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "im-core-handle-discovery-{prefix}-{}-{nanos}",
                std::process::id()
            ));
            let identity_root = root.join("identities");
            let identity_dir = identity_root.join("alice");
            fs::create_dir_all(&identity_dir).unwrap();
            fs::write(identity_root.join("default"), "alice\n").unwrap();
            fs::write(
                identity_root.join("registry.json"),
                r#"{
                  "default_identity": "alice",
                  "identities": [{
                    "id": "alice-id",
                    "did": "did:example:alice",
                    "local_alias": "alice",
                    "ready_for_auth": true,
                    "ready_for_messaging": true,
                    "missing": []
                  }]
                }"#,
            )
            .unwrap();
            fs::write(identity_dir.join("did.json"), "{}").unwrap();
            Self { root }
        }

        pub(super) fn client(&self) -> crate::core::ImClient {
            crate::core::ImCore::new(
                crate::ImCoreConfig {
                    service_base_url: crate::ServiceEndpoint::parse("https://example.test")
                        .unwrap(),
                    did_domain: "awiki.test".to_owned(),
                    client_version_info: None,
                    user_service_endpoint: None,
                    message_service_endpoint: None,
                    mail_service_endpoint: None,
                    anp_service_endpoint: None,
                    anp_service_did: None,
                    ca_bundle: None,
                    transport_policy: crate::MessageTransportPolicy::HttpOnly,
                },
                crate::ImCorePaths {
                    identities: crate::IdentityRegistryPaths {
                        identity_root_dir: self.root.join("identities"),
                        registry_path: self.root.join("identities").join("registry.json"),
                        default_identity_path: Some(self.root.join("identities").join("default")),
                    },
                    local_state: crate::LocalStatePaths {
                        sqlite_path: self.root.join("local").join("im.sqlite"),
                    },
                    runtime: crate::RuntimePaths {
                        cache_dir: self.root.join("cache"),
                        temp_dir: self.root.join("tmp"),
                    },
                },
            )
            .unwrap()
            .client(crate::identity::IdentitySelector::LocalAlias(
                "alice".to_owned(),
            ))
            .unwrap()
        }
    }
}

#[cfg(test)]
#[path = "handle_discovery_web_tests.rs"]
mod web_tests;

#[cfg(all(test, feature = "sqlite"))]
#[path = "handle_discovery_projection_tests.rs"]
mod projection_tests;

#[cfg(all(test, feature = "sqlite"))]
#[path = "handle_discovery_directory_tests.rs"]
mod directory_tests;
