use awiki_cli::host_runtime::listener_json_helpers::struct_to_map;
use serde::ser::{Serialize, Serializer};
use serde::Serialize as DeriveSerialize;
use serde_json::{json, Value};

#[derive(DeriveSerialize)]
struct AckCipherBody {
    ciphertext: String,
    aad: Value,
    skipped: Option<String>,
}

struct BrokenSerialize;

impl Serialize for BrokenSerialize {
    fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        Err(serde::ser::Error::custom("broken serialize"))
    }
}

#[test]
fn struct_to_map_round_trips_object_fields_like_go_json_marshal_unmarshal() {
    let body = AckCipherBody {
        ciphertext: "ciphertext-1".to_string(),
        aad: json!({"kid": "did:alice#key-3"}),
        skipped: None,
    };

    let mapped = struct_to_map(body);

    let mapped = mapped.as_object().expect("object map");
    assert_eq!(mapped.get("ciphertext"), Some(&json!("ciphertext-1")));
    assert_eq!(mapped.get("aad"), Some(&json!({"kid": "did:alice#key-3"})));
    assert_eq!(
        mapped.get("skipped"),
        Some(&Value::Null),
        "Go structToMap preserves marshaled null fields"
    );
}

#[test]
fn struct_to_map_returns_empty_map_when_marshal_fails() {
    let mapped = struct_to_map(BrokenSerialize);

    assert_eq!(mapped, json!({}));
}

#[test]
fn struct_to_map_returns_empty_map_for_non_object_json_values() {
    for value in [
        json!("text"),
        json!(42),
        json!(true),
        json!(["not", "object"]),
    ] {
        assert_eq!(
            struct_to_map(value),
            json!({}),
            "Go unmarshal into map returns error for non-object JSON values"
        );
    }
}

#[test]
fn struct_to_map_preserves_json_null_as_nil_map_equivalent() {
    assert_eq!(struct_to_map(Value::Null), Value::Null);
}
