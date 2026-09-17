use super::*;

#[test]
fn ambiguous_json_rpc_and_nested_capability_members_are_rejected_before_value_conversion() {
    for raw in [
        r#"{"jsonrpc":"2.0","result":{},"result":null}"#,
        r#"{"result":{"features":{"snapshot":false,"snapshot":true}}}"#,
        r#"{"result":{"items":[{"account_id":"a","account_id":"b"}]}}"#,
        r#"{"result":{"token-secret":"x","token\u002dsecret":"y"}}"#,
    ] {
        let error = crate::internal::json_rpc::decode_response(raw.as_bytes()).unwrap_err();
        assert!(matches!(error, crate::ImError::Serialization { .. }));
        assert!(!error.to_string().contains("token-secret"));
    }
}

#[test]
fn valid_json_values_and_trailing_data_keep_normal_decoder_behavior() {
    let raw = br#"{"result":{"a":[null,true,false,-4,18446744073709551615,1.25,"text"]}}"#;
    assert_eq!(
        decode(raw).unwrap(),
        serde_json::from_slice::<Value>(raw).unwrap()
    );
    assert!(decode(br#"{} {}"#).is_err());
}
