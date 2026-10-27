use super::*;
use serde_json::json;

#[test]
fn direct_identity_observations_do_not_treat_missing_or_future_schema_as_current() {
    assert!(parse(None).unwrap().is_none());
    assert!(parse(Some(&json!({"schema_version":2}))).unwrap().is_none());
    assert!(parse(Some(&Value::Null)).is_err());
    let partial = parse(Some(
        &json!({"schema_version":1,"complete":false,"identities":[]}),
    ))
    .unwrap()
    .unwrap();
    assert!(!partial.complete);
}

#[test]
fn direct_identity_observations_require_canonical_versions_and_unique_dids() {
    let identity = json!({"did":"did:wba:example.com:user:alice","document_version":"1","document_hash":format!("sha256:{}","A".repeat(43))});
    let mut value = json!({"schema_version":1,"complete":true,"identities":[identity.clone()]});
    assert!(parse(Some(&value)).unwrap().is_some());
    value["identities"]
        .as_array_mut()
        .unwrap()
        .push(identity.clone());
    assert!(parse(Some(&value)).is_err());
    value["identities"] = json!([identity]);
    for version in ["0", "01", "-1"] {
        value["identities"][0]["document_version"] = json!(version);
        assert!(parse(Some(&value)).is_err());
    }
}
