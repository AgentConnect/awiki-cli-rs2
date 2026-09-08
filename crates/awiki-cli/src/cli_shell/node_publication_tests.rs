use super::*;
#[test]
fn bridge_target_stays_inside_explicit_https_tenant() {
    let mut input = RequestInput { origin:"https://node.example".to_owned(),tenant_id:"11111111-1111-4111-8111-111111111111".to_owned(),method:"POST".to_owned(),path:"/v1/tenants/11111111-1111-4111-8111-111111111111/publication-intents/22222222-2222-4222-8222-222222222222/proofs".to_owned(),body_base64:None };
    assert!(target(&input).is_ok());
    for bad in [
        "https://other.example/v1/tenants/11111111-1111-4111-8111-111111111111",
        "/v1/tenants/11111111-1111-4111-8111-111111111111/../root",
        "/v1/tenants/11111111-1111-4111-8111-111111111111/%2e%2e/root",
        "/v1/tenants/11111111-1111-4111-8111-111111111111?redirect=https://other.example",
        "/v1/tenants/22222222-2222-4222-8222-222222222222",
    ] {
        input.path = bad.to_owned();
        assert!(target(&input).is_err());
    }
    for bad in [
        "http://node.example",
        "https://a:b@node.example",
        "https://node.example/path",
        "https://node.example#frag",
    ] {
        assert!(origin(bad).is_err());
    }
}
#[test]
fn scripted_confirmation_and_arbitrary_headers_are_not_bridge_inputs() {
    assert!(serde_json::from_value::<SignInput>(serde_json::json!({"origin":"https://node.example","target_service_id":"urn:uuid:11111111-1111-4111-8111-111111111111","tenant_id":"t","operation_id":"o","intent_hash":"h","snapshot_json":"{}","markdown":"x","confirmed":true})).is_err());
    assert!(serde_json::from_value::<RequestInput>(serde_json::json!({"origin":"https://node.example","tenant_id":"t","method":"POST","path":"/","body_base64":null,"headers":{"Authorization":"secret"}})).is_err());
}
