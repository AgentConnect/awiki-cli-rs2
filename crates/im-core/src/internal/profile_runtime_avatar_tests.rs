use super::avatar_fields;
use serde_json::json;

#[test]
fn avatar_presence_clear_alias_and_legacy_capability_contract() {
    assert_eq!(
        avatar_fields(&json!({})).unwrap(),
        (false, None, None, false)
    );
    assert_eq!(avatar_fields(&json!({"avatar_uri":null,"avatar_url":"https://old.example/a.jpg","avatar_thumbnail_uri":"https://old.example/t.jpg"})).unwrap(), (true, None, None, false));
    let value = avatar_fields(&json!({"avatar_url":"https://legacy.example/a.jpg"})).unwrap();
    assert_eq!(value.1.as_deref(), Some("https://legacy.example/a.jpg"));
    assert!(value.2.is_none());
    let value = avatar_fields(&json!({"avatar_uri":"https://example/a.jpg","avatar_thumbnail_uri":"https://example/t.jpg","avatar_upload_enabled":true})).unwrap();
    assert_eq!(value.2.as_deref(), Some("https://example/t.jpg"));
    assert!(value.3);
    assert!(avatar_fields(&json!({"avatar_uri":42})).is_err());
}

#[test]
fn avatar_conflict_machine_code_survives_rpc_decode() {
    let error = crate::internal::json_rpc::decode_response(&serde_json::to_vec(&json!({
        "jsonrpc":"2.0", "id":"1", "error":{"code":-32003,"message":"conflict","data":{"code":"avatar.version_conflict","profile_version":"8"}}
    })).unwrap()).unwrap_err();
    let crate::ImError::Service { code, data, .. } = error else {
        panic!("service error expected")
    };
    assert_eq!(code.as_deref(), Some("avatar.version_conflict"));
    assert_eq!(data.unwrap()["profile_version"], "8");
}
