//! Public P6 v2 `group.e2ee.notice` consumption.
//!
//! Notice bytes are control-plane input only. This module parses the standard
//! wire shape, resolves current DID documents through their standard HTTPS
//! DID URLs, and advances only the exact local DID/device MLS store. It never
//! produces an ordinary chat projection.

use std::collections::BTreeSet;

use anp::group_e2ee::operations::v2::{V2DidDocument, V2ProcessNoticeInput, V2ProcessNoticeOutput};
use anp::group_e2ee::{V2E2eeNotice, V2GroupNoticeMetadata};
use serde_json::{json, Value};

use crate::internal::transport::{
    AsyncAuthenticatedRpcTransport, AsyncRawJsonTransport, AuthenticatedRpcTransport,
    RawJsonTransport,
};

use super::v2_runtime::GroupE2eeV2Runtime;

const MESSAGE_RPC_ENDPOINT: &str = "/im/rpc";
const GROUP_MEMBER_RESOLUTION_LIMIT: i64 = 500;

/// Returns true only for the standard P6 v2 notice profile and notice shape.
///
/// History/inbox responses may retain only `meta` and `body`, so a flat
/// transport-protected notice is accepted in addition to the JSON-RPC wrapper.
pub(crate) fn is_v2_notice_candidate(value: &Value) -> bool {
    let meta = notice_meta_value(value);
    if meta
        .and_then(|meta| meta.get("profile"))
        .and_then(Value::as_str)
        != Some(anp::group_e2ee::GROUP_E2EE_PROFILE_V2)
    {
        return false;
    }
    if value.get("method").is_some() {
        return value.get("method").and_then(Value::as_str)
            == Some(anp::group_e2ee::METHOD_GROUP_NOTICE_V2);
    }
    meta.and_then(|meta| meta.get("security_profile"))
        .and_then(Value::as_str)
        == Some(anp::group_e2ee::GROUP_E2EE_TRANSPORT_PROFILE_V2)
        && notice_body_value(value)
            .and_then(|body| body.get("notice_type"))
            .and_then(Value::as_str)
            .is_some()
}

/// Any explicit notice method is a control message and must never fall
/// through to ordinary chat projection, including malformed/unknown profiles.
pub(crate) fn is_explicit_notice_control(value: &Value) -> bool {
    value.get("method").and_then(Value::as_str) == Some(anp::group_e2ee::METHOD_GROUP_NOTICE_V2)
}

pub(crate) fn consume_for_client(
    client: &crate::core::ImClient,
    value: &Value,
) -> crate::ImResult<V2ProcessNoticeOutput> {
    let (meta, notice) = parse_notice(value)?;
    let member_documents = resolve_member_documents(client, &notice)?;
    consume_with_runtime(
        &super::v2_runtime::runtime_for_client(client)?,
        meta,
        notice,
        member_documents,
        crate::internal::wire::common::now_rfc3339(),
        format!(
            "p6-v2-notice-{}",
            crate::internal::wire::common::generate_operation_id()
        ),
    )
}

pub(crate) async fn consume_for_client_async(
    client: &crate::core::ImClient,
    value: &Value,
) -> crate::ImResult<V2ProcessNoticeOutput> {
    let (meta, notice) = parse_notice(value)?;
    let member_documents = resolve_member_documents_async(client, &notice).await?;
    consume_with_runtime(
        &super::v2_runtime::runtime_for_client(client)?,
        meta,
        notice,
        member_documents,
        crate::internal::wire::common::now_rfc3339(),
        format!(
            "p6-v2-notice-{}",
            crate::internal::wire::common::generate_operation_id()
        ),
    )
}

pub(crate) fn consume_with_runtime(
    runtime: &GroupE2eeV2Runtime,
    meta: V2GroupNoticeMetadata,
    notice: V2E2eeNotice,
    member_documents: Vec<V2DidDocument>,
    now: String,
    request_id: String,
) -> crate::ImResult<V2ProcessNoticeOutput> {
    let scope = runtime.owner_scope()?;
    runtime.process_notice(V2ProcessNoticeInput {
        recipient_did: scope.owner_did,
        recipient_device_id: scope.device_id,
        meta,
        notice,
        member_documents,
        now,
        draft_extension_negotiated: true,
        request_id,
    })
}

pub(crate) fn parse_notice(
    value: &Value,
) -> crate::ImResult<(V2GroupNoticeMetadata, V2E2eeNotice)> {
    let mut value = value.clone();
    let object = value
        .as_object_mut()
        .ok_or(crate::ImError::PermissionDenied)?;
    if let Some(jsonrpc) = object.remove("jsonrpc") {
        if jsonrpc.as_str() != Some("2.0") {
            return Err(crate::ImError::PermissionDenied);
        }
    }
    let wire = if object.get("method").is_some() {
        value
    } else {
        json!({
            "method": anp::group_e2ee::METHOD_GROUP_NOTICE_V2,
            "params": {
                "meta": notice_meta_value(&value).cloned().ok_or(crate::ImError::PermissionDenied)?,
                "body": notice_body_value(&value).cloned().ok_or(crate::ImError::PermissionDenied)?,
            }
        })
    };
    anp::group_e2ee::parse_group_notice_notification_v2(&wire)
        .map_err(|_| crate::ImError::PermissionDenied)
}

fn resolve_member_documents(
    client: &crate::core::ImClient,
    notice: &V2E2eeNotice,
) -> crate::ImResult<Vec<V2DidDocument>> {
    let mut transport = crate::internal::transport::CoreHttpTransport::new(client);
    let params = crate::internal::wire::group::build_group_members_rpc_params(
        client.did().as_str(),
        &notice.group_did,
        GROUP_MEMBER_RESOLUTION_LIMIT,
    )?;
    let raw = AuthenticatedRpcTransport::authenticated_rpc(
        &mut transport,
        MESSAGE_RPC_ENDPOINT,
        "group.list_members",
        params,
    )?;
    let dids = collect_member_dids(client.did().as_str(), notice, &raw)?;
    dids.into_iter()
        .map(|did| {
            resolve_did_document(client, &mut transport, &did)
                .map(|document| V2DidDocument { did, document })
        })
        .collect()
}

async fn resolve_member_documents_async(
    client: &crate::core::ImClient,
    notice: &V2E2eeNotice,
) -> crate::ImResult<Vec<V2DidDocument>> {
    let mut transport = crate::internal::transport::CoreHttpTransport::new(client);
    let params = crate::internal::wire::group::build_group_members_rpc_params(
        client.did().as_str(),
        &notice.group_did,
        GROUP_MEMBER_RESOLUTION_LIMIT,
    )?;
    let raw = AsyncAuthenticatedRpcTransport::authenticated_rpc(
        &mut transport,
        MESSAGE_RPC_ENDPOINT,
        "group.list_members",
        params,
    )
    .await?;
    let dids = collect_member_dids(client.did().as_str(), notice, &raw)?;
    let mut documents = Vec::with_capacity(dids.len());
    for did in dids {
        let document = resolve_did_document_async(client, &mut transport, &did).await?;
        documents.push(V2DidDocument { did, document });
    }
    Ok(documents)
}

fn collect_member_dids(
    owner_did: &str,
    notice: &V2E2eeNotice,
    raw: &Value,
) -> crate::ImResult<BTreeSet<String>> {
    if raw
        .get("group_did")
        .and_then(Value::as_str)
        .is_some_and(|group_did| group_did != notice.group_did)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let members = raw
        .get("members")
        .and_then(Value::as_array)
        .ok_or(crate::ImError::PermissionDenied)?;
    if raw.get("has_more").and_then(Value::as_bool) == Some(true) {
        return Err(crate::ImError::PermissionDenied);
    }
    if response_total(raw).is_some_and(|total| total > members.len() as u64) {
        return Err(crate::ImError::PermissionDenied);
    }

    let mut dids = BTreeSet::from([
        required_did(owner_did)?.to_owned(),
        required_did(&notice.subject_did)?.to_owned(),
    ]);
    for member in members {
        let Some(member) = member.as_object() else {
            return Err(crate::ImError::PermissionDenied);
        };
        let member_did = member
            .get("member_did")
            .or_else(|| member.get("did"))
            .and_then(Value::as_str);
        let credential_did = member
            .get("member_credential_did")
            .or_else(|| member.get("credential_did"))
            .and_then(Value::as_str);
        let fallback_agent_did = (member_did.is_none() && credential_did.is_none())
            .then(|| member.get("agent_did").and_then(Value::as_str))
            .flatten();
        let mut found = false;
        for did in [member_did, credential_did, fallback_agent_did]
            .into_iter()
            .flatten()
        {
            dids.insert(required_did(did)?.to_owned());
            found = true;
        }
        if !found {
            return Err(crate::ImError::PermissionDenied);
        }
    }
    Ok(dids)
}

pub(super) fn resolve_did_document(
    client: &crate::core::ImClient,
    transport: &mut impl RawJsonTransport,
    did: &str,
) -> crate::ImResult<Value> {
    match resolve_did_document_fresh(client, transport, did) {
        Ok(document) => Ok(document),
        Err(error) => match crate::internal::identity_document_cache::load_local_did_document(
            &client.core_inner().sdk_paths().identities,
            did,
        )? {
            Some(document) => validate_document_id(did, document),
            None => Err(error),
        },
    }
}

/// Resolves the current DID Document without consulting the local document
/// cache. P6 authorization and Add eligibility must be based on the currently
/// published P2 Manifest, including when the requested DID is our own.
pub(super) fn resolve_did_document_fresh(
    _client: &crate::core::ImClient,
    transport: &mut impl RawJsonTransport,
    did: &str,
) -> crate::ImResult<Value> {
    crate::internal::discovery::did_document::resolve_did_document(transport, did)
}

pub(super) async fn resolve_did_document_async(
    client: &crate::core::ImClient,
    transport: &mut impl AsyncRawJsonTransport,
    did: &str,
) -> crate::ImResult<Value> {
    match resolve_did_document_fresh_async(client, transport, did).await {
        Ok(document) => Ok(document),
        Err(error) => {
            match crate::internal::identity_document_cache::load_local_did_document_async(
                &client.core_inner().sdk_paths().identities,
                did,
            )
            .await?
            {
                Some(document) => validate_document_id(did, document),
                None => Err(error),
            }
        }
    }
}

/// Resolves the current DID Document without consulting the local document
/// cache, using the same standard public DID URL as the synchronous path.
pub(super) async fn resolve_did_document_fresh_async(
    _client: &crate::core::ImClient,
    transport: &mut impl AsyncRawJsonTransport,
    did: &str,
) -> crate::ImResult<Value> {
    crate::internal::discovery::did_document::resolve_did_document_async(transport, did).await
}

fn validate_document_id(did: &str, document: Value) -> crate::ImResult<Value> {
    if document.get("id").and_then(Value::as_str) == Some(did)
        && document.get("verificationMethod").is_some()
    {
        Ok(document)
    } else {
        Err(crate::ImError::PermissionDenied)
    }
}

fn required_did(value: &str) -> crate::ImResult<&str> {
    let value = value.trim();
    if value.starts_with("did:") && value.len() > 4 {
        Ok(value)
    } else {
        Err(crate::ImError::PermissionDenied)
    }
}

fn response_total(raw: &Value) -> Option<u64> {
    raw.get("total").and_then(|value| {
        value
            .as_u64()
            .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
    })
}

fn notice_meta_value(value: &Value) -> Option<&Value> {
    value.get("meta").or_else(|| value.pointer("/params/meta"))
}

fn notice_body_value(value: &Value) -> Option<&Value> {
    value.get("body").or_else(|| value.pointer("/params/body"))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RawResolveTransport {
        calls: Vec<(String, std::collections::BTreeMap<String, String>)>,
        document: Option<Value>,
    }

    impl RawResolveTransport {
        fn failing() -> Self {
            Self {
                calls: Vec::new(),
                document: None,
            }
        }

        fn returning(document: Value) -> Self {
            Self {
                calls: Vec::new(),
                document: Some(document),
            }
        }
    }

    impl crate::internal::transport::RawJsonTransport for RawResolveTransport {
        fn get_json_url(
            &mut self,
            url: &str,
            headers: std::collections::BTreeMap<String, String>,
        ) -> crate::ImResult<Value> {
            self.calls.push((url.to_owned(), headers));
            self.document
                .clone()
                .ok_or_else(|| crate::ImError::TransportUnavailable {
                    detail: "test DID resolver unavailable".to_owned(),
                })
        }
    }

    impl crate::internal::transport::AsyncRawJsonTransport for RawResolveTransport {
        async fn get_json_url(
            &mut self,
            url: &str,
            headers: std::collections::BTreeMap<String, String>,
        ) -> crate::ImResult<Value> {
            self.calls.push((url.to_owned(), headers));
            self.document
                .clone()
                .ok_or_else(|| crate::ImError::TransportUnavailable {
                    detail: "test DID resolver unavailable".to_owned(),
                })
        }
    }

    #[test]
    fn notice_classification_is_method_profile_and_transport_scoped() {
        let wrapped = json!({
            "method": anp::group_e2ee::METHOD_GROUP_NOTICE_V2,
            "params": {
                "meta": {
                    "profile": anp::group_e2ee::GROUP_E2EE_PROFILE_V2,
                    "security_profile": anp::group_e2ee::GROUP_E2EE_TRANSPORT_PROFILE_V2
                },
                "body": {"notice_type": "commit-delivery"}
            }
        });
        assert!(is_v2_notice_candidate(&wrapped));
        assert!(is_explicit_notice_control(&wrapped));

        let flat = json!({
            "meta": {
                "profile": anp::group_e2ee::GROUP_E2EE_PROFILE_V2,
                "security_profile": anp::group_e2ee::GROUP_E2EE_TRANSPORT_PROFILE_V2
            },
            "body": {"notice_type": "welcome-delivery"}
        });
        assert!(is_v2_notice_candidate(&flat));

        let mut wrong_profile = wrapped.clone();
        wrong_profile["params"]["meta"]["profile"] = json!("anp.group.e2ee.unknown");
        assert!(!is_v2_notice_candidate(&wrong_profile));
        assert!(is_explicit_notice_control(&wrong_profile));

        let mut wrong_method = wrapped;
        wrong_method["method"] = json!("group.incoming");
        assert!(!is_v2_notice_candidate(&wrong_method));
    }

    #[test]
    fn notice_parser_accepts_json_rpc_notification_envelope() {
        let mut wire = test_notice_wire();
        wire.as_object_mut()
            .unwrap()
            .insert("jsonrpc".to_owned(), json!("2.0"));

        let (meta, notice) = parse_notice(&wire).unwrap();

        assert_eq!(meta.operation_id, "operation-1");
        assert_eq!(notice.notice_id.as_deref(), Some("notice-1"));
    }

    #[test]
    fn notice_parser_rejects_invalid_json_rpc_version() {
        for invalid in [json!("1.0"), json!(2.0), Value::Null] {
            let mut wire = test_notice_wire();
            wire.as_object_mut()
                .unwrap()
                .insert("jsonrpc".to_owned(), invalid);

            assert_eq!(parse_notice(&wire), Err(crate::ImError::PermissionDenied));
        }
    }

    #[test]
    fn notice_parser_rejects_unknown_top_level_field() {
        let mut wire = test_notice_wire();
        let object = wire.as_object_mut().unwrap();
        object.insert("jsonrpc".to_owned(), json!("2.0"));
        object.insert("unexpected".to_owned(), json!(true));

        assert_eq!(parse_notice(&wire), Err(crate::ImError::PermissionDenied));
    }

    #[test]
    fn notice_parser_accepts_flat_fallback() {
        let wire = test_notice_wire();
        let flat = json!({
            "meta": wire.pointer("/params/meta").unwrap(),
            "body": wire.pointer("/params/body").unwrap(),
        });

        let (meta, notice) = parse_notice(&flat).unwrap();

        assert_eq!(meta.operation_id, "operation-1");
        assert_eq!(notice.notice_id.as_deref(), Some("notice-1"));
    }

    #[test]
    fn member_resolution_requires_complete_group_page_and_exact_anchor() {
        let notice = test_notice();
        let dids = collect_member_dids(
            "did:example:alice",
            &notice,
            &json!({
                "group_did": "did:example:group",
                "members": [
                    {"member_did": "did:example:alice"},
                    {
                        "member_did": "did:example:bob",
                        "member_credential_did": "did:example:bob-credential",
                        "agent_did": "did:example:auxiliary-agent"
                    }
                ],
                "total": "2",
                "has_more": false
            }),
        )
        .unwrap();
        assert_eq!(
            dids.into_iter().collect::<Vec<_>>(),
            vec![
                "did:example:alice",
                "did:example:bob",
                "did:example:bob-credential"
            ]
        );

        assert!(collect_member_dids(
            "did:example:alice",
            &notice,
            &json!({"members": [], "total": 2})
        )
        .is_err());
        assert!(collect_member_dids(
            "did:example:alice",
            &notice,
            &json!({"group_did": "did:example:other", "members": []})
        )
        .is_err());
        assert!(collect_member_dids(
            "did:example:alice",
            &notice,
            &json!({"group_did": "did:example:group", "members": [{}]})
        )
        .is_err());
    }

    #[tokio::test]
    async fn fresh_resolution_never_falls_back_to_a_cached_did_document() {
        let (client, root, _) = resolver_test_client();
        let target_bundle = anp::authentication::create_did_wba_document(
            "example.test",
            anp::authentication::DidDocumentOptions {
                path_segments: vec!["bob".to_owned()],
                domain: Some("example.test".to_owned()),
                challenge: Some("p6-cached-resolve-test".to_owned()),
                ..anp::authentication::DidDocumentOptions::default()
            },
        )
        .unwrap();
        let target_did = target_bundle.did().unwrap();
        let identity_paths = &client.core_inner().sdk_paths().identities;
        let target_dir = identity_paths.identity_root_dir.join("bob-cache");
        std::fs::create_dir_all(&target_dir).unwrap();
        std::fs::write(
            &identity_paths.registry_path,
            serde_json::to_vec(&json!({
                "identities": [{
                    "id": "bob-cache",
                    "did": target_did,
                    "local_alias": "bob-cache"
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            target_dir.join("did.json"),
            serde_json::to_vec(&target_bundle.did_document).unwrap(),
        )
        .unwrap();

        let mut cached_transport = RawResolveTransport::failing();
        assert!(resolve_did_document(&client, &mut cached_transport, target_did).is_ok());
        assert_standard_did_request(target_did, &cached_transport.calls);

        let mut fresh_transport = RawResolveTransport::failing();
        assert!(
            resolve_did_document_fresh(&client, &mut fresh_transport, target_did).is_err(),
            "P6 Add must not authorize a stale cached Manifest"
        );
        assert_standard_did_request(target_did, &fresh_transport.calls);

        let mut async_cached_transport = RawResolveTransport::failing();
        assert!(
            resolve_did_document_async(&client, &mut async_cached_transport, target_did)
                .await
                .is_ok()
        );
        assert_standard_did_request(target_did, &async_cached_transport.calls);

        let mut async_fresh_transport = RawResolveTransport::failing();
        assert!(
            resolve_did_document_fresh_async(&client, &mut async_fresh_transport, target_did,)
                .await
                .is_err(),
            "async P6 authorization must not authorize a stale cached Manifest"
        );
        assert_standard_did_request(target_did, &async_fresh_transport.calls);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn fresh_resolution_uses_standard_did_url_and_accepts_the_public_document() {
        let (client, root, public_document) = resolver_test_client();
        let target_did = client.did().as_str();

        let mut sync_transport = RawResolveTransport::returning(public_document.clone());
        let sync_document =
            resolve_did_document_fresh(&client, &mut sync_transport, target_did).unwrap();
        assert_eq!(sync_document, public_document);
        assert_standard_did_request(target_did, &sync_transport.calls);

        let mut async_fresh_transport = RawResolveTransport::returning(public_document.clone());
        let async_fresh_document =
            resolve_did_document_fresh_async(&client, &mut async_fresh_transport, target_did)
                .await
                .unwrap();
        assert_eq!(async_fresh_document, public_document);
        assert_standard_did_request(target_did, &async_fresh_transport.calls);

        let mut async_transport = RawResolveTransport::returning(public_document.clone());
        let async_document = resolve_did_document_async(&client, &mut async_transport, target_did)
            .await
            .unwrap();
        assert_eq!(async_document, public_document);
        assert_standard_did_request(target_did, &async_transport.calls);
        let _ = std::fs::remove_dir_all(root);
    }

    fn assert_standard_did_request(
        did: &str,
        calls: &[(String, std::collections::BTreeMap<String, String>)],
    ) {
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].0,
            crate::internal::discovery::did_document::did_document_url(did).unwrap()
        );
        assert!(calls[0].0.starts_with("https://example.test/"));
        assert!(calls[0].0.ends_with("/did.json"));
        assert_eq!(
            calls[0].1.get("Accept").map(String::as_str),
            Some("application/json")
        );
    }

    fn resolver_test_client() -> (crate::core::ImClient, std::path::PathBuf, Value) {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "im-core-p6-fresh-resolve-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(root.join("identities")).unwrap();
        let bundle = anp::authentication::create_did_wba_document(
            "example.test",
            anp::authentication::DidDocumentOptions {
                path_segments: vec!["alice".to_owned()],
                domain: Some("example.test".to_owned()),
                challenge: Some("p6-fresh-resolve-test".to_owned()),
                ..anp::authentication::DidDocumentOptions::default()
            },
        )
        .unwrap();
        let did = crate::ids::Did::parse(bundle.did().unwrap()).unwrap();
        let did_document = bundle.did_document.clone();
        let client = crate::core::ImCore::new_with_options(
            crate::ImCoreConfig {
                service_base_url: crate::ServiceEndpoint::parse("https://example.test").unwrap(),
                did_domain: "example.test".to_owned(),
                user_service_endpoint: None,
                message_service_endpoint: None,
                mail_service_endpoint: None,
                anp_service_endpoint: None,
                anp_service_did: Some(crate::ids::Did::parse("did:example:service").unwrap()),
                ca_bundle: None,
                transport_policy: crate::MessageTransportPolicy::HttpOnly,
            },
            crate::ImCorePaths {
                identities: crate::paths::IdentityRegistryPaths {
                    identity_root_dir: root.join("identities"),
                    registry_path: root.join("identities").join("registry.json"),
                    default_identity_path: Some(root.join("identities").join("default")),
                },
                local_state: crate::paths::LocalStatePaths {
                    sqlite_path: root.join("local").join("im.sqlite"),
                },
                runtime: crate::paths::RuntimePaths {
                    cache_dir: root.join("cache"),
                    temp_dir: root.join("tmp"),
                },
            },
            crate::ImCoreOpenOptions::default(),
        )
        .unwrap()
        .client(crate::identity::IdentitySelector::Did(did))
        .unwrap();
        (client, root, did_document)
    }

    fn test_notice() -> V2E2eeNotice {
        V2E2eeNotice {
            notice_id: Some("notice-1".to_owned()),
            notice_type: "commit-delivery".to_owned(),
            group_did: "did:example:group".to_owned(),
            group_state_ref: anp::group_e2ee::V2GroupStateRef {
                group_did: "did:example:group".to_owned(),
                group_state_version: "1".to_owned(),
                policy_hash: Some("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned()),
                roster_hash: Some("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned()),
            },
            crypto_group_id_b64u: "AA".to_owned(),
            epoch: "1".to_owned(),
            subject_did: "did:example:bob".to_owned(),
            subject_device_id: "bob-device".to_owned(),
            subject_status: "active".to_owned(),
            commit_b64u: Some("AA".to_owned()),
            welcome_b64u: None,
            ratchet_tree_b64u: None,
            epoch_authenticator: None,
            group_receipt: None,
        }
    }

    fn test_notice_wire() -> Value {
        anp::group_e2ee::group_notice_notification_v2(
            V2GroupNoticeMetadata {
                anp_version: Some("2.0".to_owned()),
                profile: anp::group_e2ee::GROUP_E2EE_PROFILE_V2.to_owned(),
                security_profile: anp::group_e2ee::GROUP_E2EE_TRANSPORT_PROFILE_V2.to_owned(),
                sender_did: "did:example:group".to_owned(),
                target: anp::group_e2ee::V2Target {
                    kind: "agent".to_owned(),
                    did: "did:example:bob".to_owned(),
                },
                recipient_device_id: "bob-device".to_owned(),
                operation_id: "operation-1".to_owned(),
                created_at: None,
            },
            test_notice(),
        )
        .unwrap()
    }
}
