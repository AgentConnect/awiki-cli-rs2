use super::*;

const DID: &str = "did:web:identity.example:awiki:web:alice";

struct Transport {
    local_document: Value,
    origin_document: Value,
    method_calls: usize,
}

impl Transport {
    fn local(&mut self, did: &str) -> crate::ImResult<Value> {
        assert_eq!(did, DID);
        self.method_calls += 1;
        Ok(self.local_document.clone())
    }

    fn origin(&self, url: &str) -> crate::ImResult<Value> {
        assert_eq!(
            url,
            crate::internal::discovery::did_document::did_document_url(
                self.origin_document["id"].as_str().unwrap()
            )
            .unwrap(),
            "Web document must use the method transport"
        );
        Ok(self.origin_document.clone())
    }
}

impl crate::internal::transport::RpcTransport for Transport {
    fn rpc(&mut self, _: &str, _: &str, _: Value) -> crate::ImResult<Value> {
        panic!("notification verification only resolves documents")
    }
    fn directory_resolve_web_document(&mut self, did: &str) -> crate::ImResult<Value> {
        self.local(did)
    }
    fn directory_get_json_url(
        &mut self,
        url: &str,
        _: BTreeMap<String, String>,
    ) -> crate::ImResult<Value> {
        self.origin(url)
    }
}

impl crate::internal::transport::AsyncRpcTransport for Transport {
    async fn rpc(&mut self, _: &str, _: &str, _: Value) -> crate::ImResult<Value> {
        panic!("notification verification only resolves documents")
    }
    async fn directory_resolve_web_document(&mut self, did: &str) -> crate::ImResult<Value> {
        self.local(did)
    }
    async fn directory_get_json_url(
        &mut self,
        url: &str,
        _: BTreeMap<String, String>,
    ) -> crate::ImResult<Value> {
        self.origin(url)
    }
}

fn fixture() -> (Value, Value, Transport, DateTime<Utc>) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/multi_device_v1/system-notification-v1.json");
    let source: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let incoming = tests::fixture_incoming();
    let old_did = incoming["params"]["meta"]["target"]["did"]
        .as_str()
        .unwrap();
    let mut incoming: Value = serde_json::from_str(
        &serde_json::to_string(&incoming)
            .unwrap()
            .replace(old_did, DID),
    )
    .unwrap();
    incoming["params"]["meta"]["profile"] = json!(DIRECT_PROFILE);
    let join = &mut incoming["params"]["body"]["payload"]["payload"]["join_request"];
    join["profiles"] = json!(crate::internal::identity_device_join::join_device_profiles(
        DID
    ));
    tests::resign_join_request(join);
    tests::resign_origin_proof(&mut incoming, &source);
    let transport = Transport {
        local_document: json!({
            "id": DID,
            "service": [{
                "id": format!("{DID}#message"), "type": "ANPMessageService",
                "serviceDid": "did:wba:example.com",
                "serviceEndpoint": "https://example.com/im/rpc",
                "profiles": [DIRECT_PROFILE],
                "securityProfiles": [TRANSPORT_SECURITY],
            }],
        }),
        origin_document: source["p3_vector"]["origin_did_document"].clone(),
        method_calls: 0,
    };
    let time = DateTime::parse_from_rfc3339("2026-07-23T02:00:01Z")
        .unwrap()
        .with_timezone(&Utc);
    (incoming, source, transport, time)
}

#[tokio::test]
async fn web_join_notification_verifies_both_signatures_through_method_transport() {
    for length in [8, 6] {
        let (mut incoming, source, mut transport, time) = fixture();
        let join = &mut incoming["params"]["body"]["payload"]["payload"]["join_request"];
        join["profiles"].as_array_mut().unwrap().truncate(length);
        tests::resign_join_request(join);
        tests::resign_origin_proof(&mut incoming, &source);
        let sync = verify_with_transport(&mut transport, DID, &incoming, time).unwrap();
        let asynchronous = verify_with_transport_async(&mut transport, DID, &incoming, time)
            .await
            .unwrap();
        assert_eq!(sync, asynchronous);
        assert_eq!(transport.method_calls, 2);
    }
}

#[tokio::test]
async fn web_join_notification_rejects_invalid_document_proof_and_authority() {
    for field in ["id", "proof", "service"] {
        let (incoming, _, mut transport, time) = fixture();
        transport.local_document[field] = match field {
            "id" => json!("did:web:identity.example:bob"),
            "proof" => json!({"type": "DataIntegrityProof", "proofValue": "invalid"}),
            _ => json!([]),
        };
        assert!(
            verify_with_transport(&mut transport, DID, &incoming, time).is_err(),
            "{field}"
        );
        assert!(
            verify_with_transport_async(&mut transport, DID, &incoming, time)
                .await
                .is_err(),
            "{field}"
        );
    }
}

#[tokio::test]
async fn web_join_rejects_legacy_profiles_roles_and_each_tampered_signature() {
    for change in ["profiles", "role", "join_proof", "origin_proof"] {
        let (mut incoming, source, mut transport, time) = fixture();
        let join = &mut incoming["params"]["body"]["payload"]["payload"]["join_request"];
        match change {
            "profiles" => {
                join["profiles"] = json!(EXPECTED_PROFILES);
                tests::resign_join_request(join);
            }
            "role" => {
                join["requested_role"] = json!("admin");
                tests::resign_join_request(join);
            }
            "join_proof" => {
                join["join_request_proof"]["proof_value_b64u"] =
                    json!(URL_SAFE_NO_PAD.encode([0; 64]));
            }
            _ => {}
        }
        tests::resign_origin_proof(&mut incoming, &source);
        if change == "origin_proof" {
            incoming["params"]["auth"]["origin_proof"]["signature"] = json!("sig1=:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA:");
        }
        assert!(
            verify_with_transport(&mut transport, DID, &incoming, time).is_err(),
            "{change}"
        );
        assert!(
            verify_with_transport_async(&mut transport, DID, &incoming, time)
                .await
                .is_err(),
            "{change}"
        );
    }
}
