use super::*;
use std::{cell::RefCell, rc::Rc};

fn capabilities() -> Value {
    let fixture: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/testdata/message-sync/community-sync-v1.json"
    )))
    .unwrap();
    let mut raw = fixture["cases"]["C01"]["response"]["result"].clone();
    raw["service_did"] = serde_json::json!("did:wba:example.test");
    raw
}

#[test]
fn community_registration_requires_closed_declaration_and_one_configured_home() {
    let root = tempfile::tempdir().unwrap();
    let core = crate::ImCore::new(test_config(), test_paths(root.path())).unwrap();
    require_community_registration(&core, &capabilities()).unwrap();
    for raw in [
        serde_json::json!({}),
        serde_json::json!({"edition":"community"}),
        serde_json::json!({"supported_profiles":["awiki.message-sync.explicit-negotiation.v1","sync.snapshot_paging.v1"]}),
        {
            let mut raw = capabilities();
            raw["service_did"] = serde_json::json!("did:wba:other.test");
            raw
        },
        {
            let mut raw = capabilities();
            raw["features"]["community_sync"]["snapshot"] = serde_json::json!(true);
            raw
        },
    ] {
        assert!(require_community_registration(&core, &raw).is_err());
    }
    let mut config = test_config();
    config.user_service_endpoint =
        Some(crate::ServiceEndpoint::parse("https://other.test").unwrap());
    let split = crate::ImCore::new(config, test_paths(root.path())).unwrap();
    assert!(require_community_registration(&split, &capabilities()).is_err());
}

struct DiscoveryOnly {
    result: Option<crate::ImResult<Value>>,
    calls: Rc<RefCell<Vec<String>>>,
}
impl AsyncRpcTransport for DiscoveryOnly {
    async fn rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        _params: Value,
    ) -> crate::ImResult<Value> {
        assert_eq!(endpoint, "/im/rpc");
        assert_eq!(method, "anp.get_capabilities");
        self.calls.borrow_mut().push(method.to_owned());
        self.result.take().unwrap()
    }
}
impl AsyncRestTransport for DiscoveryOnly {
    async fn rest_post(&mut self, _: &str, _: &str, _: Value) -> crate::ImResult<Value> {
        panic!("no contact verification may run")
    }
    async fn rest_get(
        &mut self,
        _: &str,
        _: &str,
        _: &std::collections::BTreeMap<String, String>,
    ) -> crate::ImResult<Value> {
        panic!("no contact verification may run")
    }
}

#[tokio::test]
async fn community_registration_discovery_failure_creates_no_identity_and_never_registers() {
    for result in [
        Ok(serde_json::json!({})),
        Ok(
            serde_json::json!({"supported_profiles":["awiki.message-sync.explicit-negotiation.v1","sync.snapshot_paging.v1"]}),
        ),
        Err(crate::ImError::TransportUnavailable {
            detail: "discovery unavailable".into(),
        }),
    ] {
        let root = tempfile::tempdir().unwrap();
        let core = crate::ImCore::new(test_config(), test_paths(root.path())).unwrap();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut request = request();
        request.verification = crate::identity::VerificationInput::Community;
        let runtime = IdentityRegistrationRuntime::new(
            &core,
            DiscoveryOnly {
                result: Some(result),
                calls: calls.clone(),
            },
        );
        assert!(runtime.register_handle_async(request).await.is_err());
        assert_eq!(calls.borrow().as_slice(), &["anp.get_capabilities"]);
        assert!(core.identities().list_async().await.unwrap().is_empty());
    }
}

#[test]
fn community_registration_wire_contains_no_fake_contact_or_privileged_grant() {
    let (_root, _core, pending) = pending_with_core();
    let mut request = request();
    request.verification = crate::identity::VerificationInput::Community;
    let call = register_call(&pending, &request, None).unwrap();
    assert_eq!(call.method, "register");
    for key in [
        "phone",
        "otp_code",
        "email",
        "verification_token",
        "provision_operation_id",
    ] {
        assert!(call.params.get(key).is_none());
    }
    assert!(call.params.get("did_document").is_some());
    assert_eq!(
        pending_verification_kind(&request.verification),
        "community"
    );
}
