use awiki_im_core as im_core;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

fn authorization_handle() -> im_core::identity::RootKeyTransferAuthorizationHandle {
    serde_json::from_value(serde_json::Value::String(
        URL_SAFE_NO_PAD.encode([0x88_u8; 32]),
    ))
    .unwrap()
}

#[test]
fn authorization_handle_is_closed_canonical_and_debug_redacted() {
    let handle = authorization_handle();
    let encoded = serde_json::to_value(&handle).unwrap();
    assert_eq!(encoded.as_str().unwrap().len(), 43);
    assert!(!format!("{handle:?}").contains(encoded.as_str().unwrap()));

    for invalid in [
        serde_json::json!("short"),
        serde_json::json!("iIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIg="),
        serde_json::json!("*******************************************"),
    ] {
        assert!(
            serde_json::from_value::<im_core::identity::RootKeyTransferAuthorizationHandle>(
                invalid
            )
            .is_err()
        );
    }
}

#[test]
fn prepare_and_send_dtos_match_the_secret_free_closed_host_contract() {
    let preparation = im_core::identity::RootKeyTransferPreparation {
        authorization_handle: authorization_handle(),
        recipient: im_core::identity::RootKeyTransferRecipientSummary {
            did: im_core::ids::Did::parse("did:example:alice").unwrap(),
            device_id: im_core::ids::ProtocolDeviceId::parse("dev-recipient").unwrap(),
            signing_key_id: "did:example:alice#dev-recipient-sign".to_owned(),
            e2ee_key_id: "did:example:alice#dev-recipient-e2ee".to_owned(),
            registry_version: 7,
        },
        expires_at: "2026-07-24T00:02:00Z".to_owned(),
    };
    let value = serde_json::to_value(&preparation).unwrap();
    let fields = value
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        fields,
        vec!["authorization_handle", "expires_at", "recipient"]
    );
    let serialized = serde_json::to_string(&value).unwrap();
    for forbidden in [
        "root_private_key",
        "prekey_bundle",
        "document_hash",
        "ciphertext",
        "nonce",
        "proof",
    ] {
        assert!(!serialized.contains(forbidden));
    }

    let result = im_core::identity::RootKeyTransferSendResult {
        did: im_core::ids::Did::parse("did:example:alice").unwrap(),
        sender_device_id: im_core::ids::ProtocolDeviceId::parse("dev-sender").unwrap(),
        recipient_device_id: im_core::ids::ProtocolDeviceId::parse("dev-recipient").unwrap(),
        message_id: im_core::ids::MessageId::parse("msg-root-transfer").unwrap(),
        accepted_at: "2026-07-24T00:01:02.123456Z".to_owned(),
    };
    let result_value = serde_json::to_value(result).unwrap();
    assert_eq!(result_value.as_object().unwrap().len(), 5);
    assert!(result_value.get("status").is_none());
    assert!(result_value.get("completed_at").is_none());
}

#[test]
fn root_transfer_errors_are_a_closed_code_and_retryable_pair() {
    use im_core::identity::{RootKeyTransferError, RootKeyTransferErrorCode};

    let cases = [
        (RootKeyTransferErrorCode::Unsupported, false),
        (RootKeyTransferErrorCode::SenderNotEligible, false),
        (RootKeyTransferErrorCode::RecipientNotEligible, false),
        (RootKeyTransferErrorCode::PrekeyUnavailable, true),
        (RootKeyTransferErrorCode::PrekeyInvalid, false),
        (RootKeyTransferErrorCode::AuthorizationExpired, true),
        (
            RootKeyTransferErrorCode::AuthorizationAlreadyConsumed,
            false,
        ),
        (RootKeyTransferErrorCode::TransportPending, false),
        (RootKeyTransferErrorCode::TransportRejected, true),
    ];
    for (code, retryable) in cases {
        let error = RootKeyTransferError { code, retryable };
        let value = serde_json::to_value(error).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 2);
        assert_eq!(value["retryable"], retryable);
        assert!(value["code"]
            .as_str()
            .unwrap()
            .starts_with("root_transfer."));
    }
}

#[test]
fn host_cannot_supply_identity_or_message_id_to_either_request() {
    assert!(
        serde_json::from_value::<im_core::identity::RootKeyTransferPrepareRequest>(
            serde_json::json!({
                "recipient_device_id": "dev-recipient",
                "identity": {"type": "default"}
            })
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<im_core::identity::RootKeyTransferSendRequest>(
            serde_json::json!({
                "authorization_handle": URL_SAFE_NO_PAD.encode([0x88_u8; 32]),
                "user_presence_confirmed": true,
                "message_id": "host-chosen"
            })
        )
        .is_err()
    );
}
