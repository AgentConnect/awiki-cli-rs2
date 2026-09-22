use super::*;
use serde_json::json;

const HANDLE: &str = "alice.provider.example";
const DID: &str = "did:web:identity.example:alice";
const URL: &str = "https://provider.example/.well-known/handle/alice";

struct Transport {
    forward: Value,
    document: Value,
    method_calls: usize,
}

impl Transport {
    fn new() -> Self {
        Self {
            forward: json!({
                "handle": HANDLE, "did": DID, "status": "active", "binding_generation": "7",
                "user_id": "ignored-provider-private-id",
            }),
            document: json!({
                "id": DID,
                "service": [{
                    "id": format!("{DID}#handle"), "type": "ANPHandleService",
                    "serviceEndpoint": URL,
                }],
            }),
            method_calls: 0,
        }
    }

    fn forward(&self, url: &str) -> crate::ImResult<Value> {
        assert_eq!(url, URL, "DID resolution must use the method transport");
        Ok(self.forward.clone())
    }

    fn document(&mut self, did: &str) -> crate::ImResult<Value> {
        assert_eq!(did, DID);
        self.method_calls += 1;
        Ok(self.document.clone())
    }
}

impl crate::internal::transport::RawJsonTransport for Transport {
    fn get_json_url(&mut self, url: &str, _: BTreeMap<String, String>) -> crate::ImResult<Value> {
        self.forward(url)
    }
    fn resolve_web_document(&mut self, did: &str) -> crate::ImResult<Value> {
        self.document(did)
    }
}

impl crate::internal::transport::AsyncRawJsonTransport for Transport {
    async fn get_json_url(
        &mut self,
        url: &str,
        _: BTreeMap<String, String>,
    ) -> crate::ImResult<Value> {
        self.forward(url)
    }
    async fn resolve_web_document(&mut self, did: &str) -> crate::ImResult<Value> {
        self.document(did)
    }
}

#[tokio::test]
async fn web_provider_evidence_is_required_on_both_transport_paths() {
    for endpoint in [
        URL,
        "https://provider.example:443/.well-known/handle/alice",
        "https://identity.example/.well-known/handle/alice",
        "https://provider.example:8443/.well-known/handle/alice",
        "http://provider.example/.well-known/handle/alice",
        "https://user@provider.example/.well-known/handle/alice",
        "https://provider.example.attacker.example/handle/alice",
        "",
    ] {
        let expected = endpoint == URL || endpoint.contains(":443/");
        let mut transport = Transport::new();
        transport.document["service"][0]["serviceEndpoint"] = json!(endpoint);
        let sync = fetch_public_binding_document(&mut transport, HANDLE, URL);
        let asynchronous = fetch_public_binding_document_async(&mut transport, HANDLE, URL).await;
        assert_eq!(sync.is_ok(), expected, "{endpoint}");
        assert_eq!(asynchronous.is_ok(), expected, "{endpoint}");
        assert_eq!(transport.method_calls, 2);
        if expected {
            let resolved = resolution_from_public_document(HANDLE, sync.unwrap()).unwrap();
            assert_eq!(resolved.target_did, DID);
            assert_eq!(resolved.authority_subject_id, HANDLE);
            assert_eq!(resolved.peer_scope().unwrap().user_id, HANDLE);
        }
    }
}

#[tokio::test]
async fn web_forward_and_document_failures_never_become_authority() {
    for (field, value) in [
        ("handle", json!("bob.provider.example")),
        ("status", json!("inactive")),
        ("binding_generation", json!("0")),
        ("binding_generation", json!("07")),
        ("binding_generation", json!(7)),
        ("binding_generation", Value::Null),
        ("did", json!("did:web:127.0.0.1:alice")),
        ("did", json!("did:web:identity.example:alice%2Fadmin")),
        ("did", json!("did:key:z6mk")),
    ] {
        let mut transport = Transport::new();
        transport.forward[field] = value;
        assert!(
            fetch_public_binding_document(&mut transport, HANDLE, URL).is_err(),
            "{field}"
        );
        assert!(
            fetch_public_binding_document_async(&mut transport, HANDLE, URL)
                .await
                .is_err(),
            "{field}"
        );
        assert_eq!(transport.method_calls, 0);
    }
    for (field, value) in [
        ("id", json!("did:web:identity.example:bob")),
        (
            "proof",
            json!({"type": "DataIntegrityProof", "proofValue": "invalid"}),
        ),
        ("service", json!([])),
    ] {
        let mut transport = Transport::new();
        transport.document[field] = value;
        assert!(
            fetch_public_binding_document(&mut transport, HANDLE, URL).is_err(),
            "{field}"
        );
        assert!(
            fetch_public_binding_document_async(&mut transport, HANDLE, URL)
                .await
                .is_err(),
            "{field}"
        );
    }
}

#[test]
fn web_public_binding_does_not_replace_conflicting_local_account_facts() {
    let transport = Transport::new();
    let public = authoritative_lookup_from_public_document(HANDLE, &transport.forward).unwrap();
    let mut local = public.clone();
    local.user_id = "stable-local-account".to_owned();
    assert_eq!(
        merge_local_directory_with_public_binding(HANDLE, local.clone(), &transport.forward)
            .unwrap()
            .user_id,
        "stable-local-account"
    );
    local.did = crate::ids::Did::parse("did:web:identity.example:bob").unwrap();
    assert!(merge_local_directory_with_public_binding(HANDLE, local, &transport.forward).is_err());
    let mut local = public;
    local.binding_generation = Some("8".to_owned());
    assert!(merge_local_directory_with_public_binding(HANDLE, local, &transport.forward).is_err());
}

#[tokio::test]
async fn wba_forward_validation_keeps_existing_provider_and_transport_rules() {
    let mut transport = Transport::new();
    transport.forward["did"] = json!("did:wba:provider.example:user:alice:e1_alice");
    fetch_public_binding_document(&mut transport, HANDLE, URL).unwrap();
    fetch_public_binding_document_async(&mut transport, HANDLE, URL)
        .await
        .unwrap();
    assert_eq!(transport.method_calls, 0);
    transport.forward["did"] = json!("did:wba:other.example:user:alice:e1_alice");
    assert!(fetch_public_binding_document(&mut transport, HANDLE, URL).is_err());
    assert!(
        fetch_public_binding_document_async(&mut transport, HANDLE, URL)
            .await
            .is_err()
    );
}
