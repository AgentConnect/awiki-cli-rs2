use crate::dto::{root_key_transfer_preparation, root_key_transfer_send_result};

fn authorization_handle() -> im_core::identity::RootKeyTransferAuthorizationHandle {
    serde_json::from_value(serde_json::Value::String("A".repeat(43))).unwrap()
}

#[test]
fn root_transfer_dtos_are_exact_device_and_redact_authorization() {
    let preparation =
        root_key_transfer_preparation(im_core::identity::RootKeyTransferPreparation {
            authorization_handle: authorization_handle(),
            recipient: im_core::identity::RootKeyTransferRecipientSummary {
                did: im_core::ids::Did::parse("did:wba:awiki.info:alice").unwrap(),
                device_id: im_core::ids::ProtocolDeviceId::parse("device-member").unwrap(),
                signing_key_id: "did:wba:awiki.info:alice#sign-member".to_owned(),
                e2ee_key_id: "did:wba:awiki.info:alice#e2ee-member".to_owned(),
                registry_version: 7,
            },
            expires_at: "2026-08-31T12:00:00Z".to_owned(),
        });
    assert_eq!(preparation.recipient.device_id, "device-member");
    assert_eq!(preparation.recipient.registry_version, "7");
    let rendered = format!("{preparation:?}");
    assert!(rendered.contains("<redacted-authorization-handle>"));
    assert!(!rendered.contains(&"A".repeat(43)));

    let sent = root_key_transfer_send_result(im_core::identity::RootKeyTransferSendResult {
        did: im_core::ids::Did::parse("did:wba:awiki.info:alice").unwrap(),
        sender_device_id: im_core::ids::ProtocolDeviceId::parse("device-admin").unwrap(),
        recipient_device_id: im_core::ids::ProtocolDeviceId::parse("device-member").unwrap(),
        message_id: im_core::ids::MessageId::parse("message-root-transfer").unwrap(),
        accepted_at: "2026-08-31T11:59:00Z".to_owned(),
    });
    assert_eq!(sent.recipient_device_id, "device-member");
    assert_eq!(sent.message_id, "message-root-transfer");
}

#[test]
fn root_transfer_errors_keep_closed_codes_and_retryability() {
    use im_core::identity::RootKeyTransferErrorCode::*;
    for (code, expected, retryable) in [
        (
            SenderNotEligible,
            "root_transfer.sender_not_eligible",
            false,
        ),
        (
            RecipientNotEligible,
            "root_transfer.recipient_not_eligible",
            false,
        ),
        (
            AuthorizationExpired,
            "root_transfer.authorization_expired",
            true,
        ),
        (
            UserPresenceDenied,
            "root_transfer.user_presence_denied",
            false,
        ),
        (StateChanged, "root_transfer.state_changed", true),
        (TransportPending, "root_transfer.transport_pending", false),
    ] {
        let safe =
            crate::error::SafeError::from_root_transfer(im_core::identity::RootKeyTransferError {
                code,
                retryable,
            });
        assert_eq!(safe.code, expected);
        assert_eq!(safe.retryable, retryable);
        assert!(!safe.safe_message.contains("device-member"));
    }
}
