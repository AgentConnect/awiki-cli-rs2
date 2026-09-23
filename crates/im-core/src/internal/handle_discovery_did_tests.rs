use super::*;
use crate::internal::directory_runtime::{
    lookup_handle_by_did_for_projection, lookup_handle_by_did_for_projection_async,
};
use crate::internal::transport::{AsyncRpcTransport, RpcTransport};
use serde_json::json;

const DID: &str = "did:wba:remote.test:agent:skill:peer";
const WEB_DID: &str = "did:web:identity.test:peer";
const HANDLE: &str = "peer.remote.test";
const URL: &str = "https://remote.test/.well-known/handle/peer";

struct Transport {
    did: &'static str,
    document: Value,
    binding: Value,
    missing: bool,
    public_reads: Vec<String>,
}

impl Transport {
    fn new(did: &'static str) -> Self {
        Self {
            did,
            document: json!({"id":did,"service":[{"id":format!("{did}#handle"),"type":"ANPHandleService","serviceEndpoint":URL}]}),
            binding: json!({"handle":HANDLE,"did":did,"status":"active","binding_generation":"1","user_id":"private-id-not-authority"}),
            missing: true,
            public_reads: Vec::new(),
        }
    }
    fn home(&self, params: Value) -> crate::ImResult<Value> {
        assert_eq!(params["did"], self.did);
        Err(crate::ImError::Service {
            status_code: Some(if self.missing { 200 } else { 401 }),
            code: Some(
                if self.missing {
                    "-32002"
                } else {
                    "unauthorized"
                }
                .into(),
            ),
            // Even a misleading body must not turn HTTP 401 into discovery.
            message: "Handle 不存在".into(),
            data: None,
        })
    }
    fn public(&mut self, url: &str, headers: BTreeMap<String, String>) -> crate::ImResult<Value> {
        assert!(!headers.contains_key("Authorization"));
        self.public_reads.push(url.to_owned());
        if url == URL {
            return Ok(self.binding.clone());
        }
        assert_eq!(
            url,
            crate::internal::discovery::did_document::did_document_url(self.did).unwrap()
        );
        Ok(self.document.clone())
    }
}
impl RpcTransport for Transport {
    fn rpc(&mut self, _: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        assert_eq!(method, "lookup");
        self.home(params)
    }
    fn directory_get_json_url(
        &mut self,
        url: &str,
        headers: BTreeMap<String, String>,
    ) -> crate::ImResult<Value> {
        self.public(url, headers)
    }
    fn directory_resolve_web_document(&mut self, did: &str) -> crate::ImResult<Value> {
        assert_eq!(did, self.did);
        self.public_reads.push("web-method-resolver".into());
        Ok(self.document.clone())
    }
}
impl AsyncRpcTransport for Transport {
    async fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        RpcTransport::rpc(self, endpoint, method, params)
    }
    async fn directory_get_json_url(
        &mut self,
        url: &str,
        headers: BTreeMap<String, String>,
    ) -> crate::ImResult<Value> {
        self.public(url, headers)
    }
    async fn directory_resolve_web_document(&mut self, did: &str) -> crate::ImResult<Value> {
        RpcTransport::directory_resolve_web_document(self, did)
    }
}

#[tokio::test]
async fn missing_home_hint_discovers_verified_foreign_persona_on_both_paths() {
    for did in [DID, WEB_DID] {
        let fixture = super::super::tests::Fixture::new("reverse-did-discovery");
        let client = fixture.client();
        let mut sync = Transport::new(did);
        let mut asynchronous = Transport::new(did);
        let expected = crate::ids::Did::parse(did).unwrap();
        let lookup = lookup_handle_by_did_for_projection(&client, &mut sync, &expected).unwrap();
        let other =
            lookup_handle_by_did_for_projection_async(&client, &mut asynchronous, &expected)
                .await
                .unwrap();
        assert_eq!(lookup, other);
        assert_eq!(lookup.did, expected);
        assert_eq!(lookup.handle.as_str(), HANDLE);
        assert_eq!(lookup.user_id, HANDLE);
        assert_eq!(lookup.binding_generation.as_deref(), Some("1"));
        crate::directory::project_handle_lookup_async(&client, &lookup)
            .await
            .unwrap();
        let db = client.core_inner().local_state_db().await.unwrap();
        assert!(db
            .filter_unresolved_peer_dids("alice-id", vec![did.to_owned()])
            .await
            .unwrap()
            .is_empty());
    }
}

#[tokio::test]
async fn foreign_discovery_does_not_mask_home_authentication_failure() {
    let fixture = super::super::tests::Fixture::new("reverse-auth-error");
    let client = fixture.client();
    let mut transport = Transport::new(DID);
    transport.missing = false;
    let did = crate::ids::Did::parse(DID).unwrap();
    assert!(matches!(
        lookup_handle_by_did_for_projection(&client, &mut transport, &did),
        Err(crate::ImError::Service {
            status_code: Some(401),
            ..
        })
    ));
    assert!(matches!(
        lookup_handle_by_did_for_projection_async(&client, &mut transport, &did).await,
        Err(crate::ImError::Service {
            status_code: Some(401),
            ..
        })
    ));
    assert!(transport.public_reads.is_empty());
}

#[tokio::test]
async fn reverse_discovery_rejects_untrusted_endpoints_before_fetching_binding() {
    let fixture = super::super::tests::Fixture::new("reverse-endpoints");
    let client = fixture.client();
    for endpoint in [
        "http://remote.test/.well-known/handle/peer",
        "https://remote.test:8443/.well-known/handle/peer",
        "https://user:pass@remote.test/.well-known/handle/peer",
        "https://remote.test/.well-known/handle/peer?q=1",
        "https://remote.test/.well-known/handle/peer#x",
        "https://remote.test/other/peer",
        "https://remote.test/.well-known/handle/peer%2Fother",
        "https://remote.test/.well-known/handle/peer/other",
        "https://awiki.test/.well-known/handle/peer",
    ] {
        let mut t = Transport::new(DID);
        t.document["service"][0]["serviceEndpoint"] = json!(endpoint);
        assert!(
            foreign_binding_from_did(&client, &mut t, DID).is_err(),
            "{endpoint}"
        );
        assert!(
            foreign_binding_from_did_async(&client, &mut t, DID)
                .await
                .is_err(),
            "{endpoint}"
        );
        assert!(!t.public_reads.iter().any(|url| url == URL));
    }
}

#[tokio::test]
async fn reverse_discovery_requires_current_matching_authority_binding() {
    let fixture = super::super::tests::Fixture::new("reverse-binding");
    let client = fixture.client();
    for (key, value) in [
        ("did", json!("did:wba:remote.test:other")),
        ("handle", json!("other.remote.test")),
        ("status", json!("inactive")),
        ("binding_generation", json!("0")),
        ("binding_generation", json!(1)),
        ("binding_generation", Value::Null),
    ] {
        let mut t = Transport::new(DID);
        t.binding[key] = value;
        assert!(
            foreign_binding_from_did(&client, &mut t, DID).is_err(),
            "{key}"
        );
        assert!(
            foreign_binding_from_did_async(&client, &mut t, DID)
                .await
                .is_err(),
            "{key}"
        );
    }
}

#[tokio::test]
async fn reverse_discovery_requires_exact_document_and_one_service() {
    let fixture = super::super::tests::Fixture::new("reverse-document");
    let client = fixture.client();
    for (key, value) in [
        ("id", json!("did:wba:remote.test:other")),
        ("service", json!([])),
        (
            "service",
            json!([{"type":"ANPHandleService","serviceEndpoint":URL},{"type":"ANPHandleService","serviceEndpoint":URL}]),
        ),
    ] {
        let mut t = Transport::new(DID);
        t.document[key] = value;
        assert!(foreign_binding_from_did(&client, &mut t, DID).is_err());
        assert!(foreign_binding_from_did_async(&client, &mut t, DID)
            .await
            .is_err());
        assert!(!t.public_reads.iter().any(|url| url == URL));
    }
    let mut t = Transport::new(WEB_DID);
    t.document["proof"] = json!({"type":"DataIntegrityProof","proofValue":"invalid"});
    assert!(foreign_binding_from_did(&client, &mut t, WEB_DID).is_err());
    assert!(foreign_binding_from_did_async(&client, &mut t, WEB_DID)
        .await
        .is_err());
    assert!(!t.public_reads.iter().any(|url| url == URL));
}

#[tokio::test]
async fn production_foreign_discovery_guards_initial_document_and_binding_reads() {
    use crate::internal::transport::{AsyncRawJsonTransport, CoreHttpTransport, RawJsonTransport};
    let fixture = super::super::tests::Fixture::new("public-network-boundary");
    let client = fixture.client();
    let mut http = CoreHttpTransport::new(&client);
    // These are real production transports, not document-returning test doubles.
    for did in ["did:wba:127.0.0.1:peer", "did:wba:169.254.169.254:peer"] {
        assert!(foreign_binding_from_did(&client, &mut http, did).is_err());
        assert!(foreign_binding_from_did_async(&client, &mut http, did)
            .await
            .is_err());
    }
    let mut discovery = DirectoryDiscovery(&mut http);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let binding_url = format!(
        "https://{}/.well-known/handle/peer",
        listener.local_addr().unwrap()
    );
    assert!(RawJsonTransport::get_json_url(&mut discovery, &binding_url, BTreeMap::new()).is_err());
    assert!(
        AsyncRawJsonTransport::get_json_url(&mut discovery, &binding_url, BTreeMap::new())
            .await
            .is_err()
    );
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}
