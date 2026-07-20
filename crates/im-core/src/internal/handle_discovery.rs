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

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct RecoveryHandleBinding {
    pub(crate) handle: crate::ids::Handle,
    pub(crate) local_part: String,
    pub(crate) domain: String,
    pub(crate) did: crate::ids::Did,
    pub(crate) mapping_generation: u64,
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
            let raw = crate::internal::transport::RawJsonTransport::get_json_url(
                &mut transport,
                url.as_str(),
                BTreeMap::new(),
            )?;
            resolution_from_public_document(full_handle.as_str(), raw)
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
            let raw = crate::internal::transport::AsyncRawJsonTransport::get_json_url(
                &mut transport,
                url.as_str(),
                BTreeMap::new(),
            )
            .await?;
            resolution_from_public_document(full_handle.as_str(), raw)
        }
    }
}

pub(crate) async fn resolve_authoritative_handle_binding_async(
    client: &crate::core::ImClient,
    raw_handle: &str,
) -> crate::ImResult<crate::directory::HandleLookupResult> {
    let normalized = normalize_handle_for_client(client, raw_handle)?;
    let url = authoritative_discovery_url_for_client(client, &normalized);
    let mut transport = crate::internal::transport::CoreHttpTransport::new(client);
    let raw = crate::internal::transport::AsyncRawJsonTransport::get_json_url(
        &mut transport,
        &url,
        BTreeMap::new(),
    )
    .await?;
    authoritative_lookup_from_public_document(&normalized.full_handle, &raw)
}

/// Resolves the current same-domain Handle mapping without requiring an
/// existing identity. Recovery uses this value only for AWiki-internal CAS;
/// the generation is not an ANP or DID Document field.
pub(crate) async fn resolve_recovery_handle_binding_async(
    core: &crate::core::ImCore,
    raw_handle: &str,
) -> crate::ImResult<RecoveryHandleBinding> {
    let normalized = normalize_handle_with_default_domain(
        raw_handle,
        core.inner().sdk_config().did_domain.as_str(),
    )?;
    let configured_domain = normalize_domain(core.inner().sdk_config().did_domain.as_str());
    if normalized.domain != configured_domain {
        return Err(crate::ImError::unsupported(
            "handle_recovery_same_domain_only",
        ));
    }
    let url = authoritative_discovery_url_for_core(core, &normalized);
    let mut transport = crate::internal::transport::CorePlainTransport::new(core);
    let raw = crate::internal::transport::AsyncRawJsonTransport::get_json_url(
        &mut transport,
        &url,
        BTreeMap::new(),
    )
    .await?;
    recovery_handle_binding_from_value(&normalized.full_handle, &raw)
}

pub(crate) fn recovery_handle_binding_from_value(
    expected_handle: &str,
    raw: &Value,
) -> crate::ImResult<RecoveryHandleBinding> {
    let normalized = normalize_handle(expected_handle)?;
    let status = string_field(raw, "status")?;
    validate_active_status(&normalized.full_handle, &status)?;
    let full_handle = first_string_field(raw, &["full_handle", "handle"])?;
    validate_handle_match(&normalized.full_handle, &full_handle)?;
    let did = crate::ids::Did::parse(&string_field(raw, "did")?)?;
    validate_did_matches_handle_domain(&full_handle, did.as_str())?;
    let generation = raw
        .get("binding_generation")
        .and_then(Value::as_str)
        .ok_or(crate::ImError::PermissionDenied)?;
    let mapping_generation = canonical_positive_generation(generation)?;
    Ok(RecoveryHandleBinding {
        handle: crate::ids::Handle::parse(&full_handle, "")?,
        local_part: normalized.local_part,
        domain: normalized.domain,
        did,
        mapping_generation,
    })
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
    let Some(did_domain) = did_wba_domain(did) else {
        return Err(crate::ImError::InvalidInput {
            field: Some("did".to_owned()),
            message: format!("handle discovery DID {did} is not did:wba"),
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

fn is_local_handle(client: &crate::core::ImClient, full_handle: &str) -> bool {
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
    if is_local_handle(client, &handle.full_handle) {
        let config = client.core_inner().sdk_config();
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

fn authoritative_discovery_url_for_core(
    core: &crate::core::ImCore,
    handle: &NormalizedHandle,
) -> String {
    let configured_base = core
        .inner()
        .sdk_config()
        .user_service_endpoint
        .as_ref()
        .unwrap_or(&core.inner().sdk_config().service_base_url)
        .as_str()
        .trim_end_matches('/');
    if is_loopback_http_base(configured_base) {
        return format!(
            "{configured_base}/.well-known/handle/{}",
            percent_encode_path_segment(&handle.local_part)
        );
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

fn canonical_positive_generation(value: &str) -> crate::ImResult<u64> {
    let trimmed = value.trim();
    let parsed = value
        .parse::<u64>()
        .ok()
        .filter(|generation| *generation > 0)
        .filter(|generation| value == trimmed && generation.to_string() == value)
        .ok_or(crate::ImError::PermissionDenied)?;
    Ok(parsed)
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
    fn recovery_binding_requires_canonical_positive_generation() {
        let base = json!({
            "did": "did:wba:awiki.info:user:alice:e1_current",
            "full_handle": "alice.awiki.info",
            "status": "active",
            "binding_generation": "3"
        });
        let binding = super::recovery_handle_binding_from_value("alice.awiki.info", &base).unwrap();
        assert_eq!(binding.mapping_generation, 3);

        for invalid in ["0", "03", "+3", "-1", "", " 3"] {
            let mut raw = base.clone();
            raw["binding_generation"] = Value::String(invalid.to_owned());
            assert!(super::recovery_handle_binding_from_value("alice.awiki.info", &raw).is_err());
        }
    }

    #[test]
    fn recovery_binding_uses_only_public_wns_authority_fields() {
        let base = json!({
            "did": "did:wba:awiki.info:user:alice:e1_current",
            "handle": "alice.awiki.info",
            "status": "active",
            "binding_generation": "3"
        });
        let without_internal_subject =
            super::recovery_handle_binding_from_value("alice.awiki.info", &base).unwrap();

        let mut with_untrusted_internal_subject = base;
        with_untrusted_internal_subject["user_id"] = Value::String("untrusted-user".to_owned());
        with_untrusted_internal_subject["subject_id"] =
            Value::String("untrusted-subject".to_owned());
        let ignored_internal_subject = super::recovery_handle_binding_from_value(
            "alice.awiki.info",
            &with_untrusted_internal_subject,
        )
        .unwrap();

        assert_eq!(without_internal_subject, ignored_internal_subject);
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

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new(prefix: &str) -> Self {
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

        fn client(&self) -> crate::core::ImClient {
            crate::core::ImCore::new(
                crate::ImCoreConfig {
                    service_base_url: crate::ServiceEndpoint::parse("https://example.test")
                        .unwrap(),
                    did_domain: "awiki.test".to_owned(),
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
