use super::*;
use serde_json::json;

fn snapshot() -> Value {
    json!({"schema_version":1,"complete":true,"groups":[{
        "group_did":"did:example:group","host_service_did":"did:example:home",
        "group_state_version":"2","epoch":"3","revision":format!("sha256:{}", "a".repeat(43)),
        "local_identity_coverage":true
    }]})
}

#[test]
fn missing_or_future_observation_is_not_an_empty_current_inventory() {
    assert_eq!(parse(None).unwrap(), None);
    assert_eq!(parse(Some(&json!({"schema_version":2}))).unwrap(), None);
    let mut partial = snapshot();
    partial["complete"] = json!(false);
    assert!(!parse(Some(&partial)).unwrap().unwrap().complete);
}

#[test]
fn supported_observations_reject_duplicates_malformed_versions_and_null() {
    assert!(parse(Some(&snapshot())).unwrap().is_some());
    assert!(parse(Some(&Value::Null)).is_err());
    let mut duplicate = snapshot();
    let group = duplicate["groups"][0].clone();
    duplicate["groups"].as_array_mut().unwrap().push(group);
    assert!(parse(Some(&duplicate)).is_err());
    for field in ["group_state_version", "epoch"] {
        let mut wrong = snapshot();
        wrong["groups"][0][field] = json!("01");
        assert!(parse(Some(&wrong)).is_err());
    }
    let mut wrong = snapshot();
    wrong["groups"][0]["revision"] = json!("unscoped");
    assert!(parse(Some(&wrong)).is_err());
}
