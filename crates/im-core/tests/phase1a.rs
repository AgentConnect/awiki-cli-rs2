use std::path::PathBuf;

use im_core::prelude::*;

#[test]
fn im_core_new_can_construct() {
    let core = test_core();
    let report = core.bootstrap().validate_paths().unwrap();
    assert_eq!(report.checked.len(), 6);
}

#[test]
fn identity_selector_local_alias_can_construct() {
    let selector = IdentitySelector::LocalAlias("alice".to_string());
    assert!(matches!(selector, IdentitySelector::LocalAlias(alias) if alias == "alice"));
}

#[test]
fn message_target_direct_and_group_can_construct() {
    let peer = PeerRef::parse("bob", "awiki.info").unwrap();
    let group = GroupRef::parse("did:example:group").unwrap();

    let direct = MessageTarget::Direct(peer);
    let group = MessageTarget::Group(group);

    assert!(matches!(direct, MessageTarget::Direct(_)));
    assert!(matches!(group, MessageTarget::Group(_)));
}

#[test]
fn secure_direct_and_group_e2ee_security_modes_can_construct() {
    let secure_direct = MessageSecurityMode::SecureDirect;
    let group_e2ee = MessageSecurityMode::GroupE2ee;
    let required = MessageSecurityPolicy::E2eeRequired;

    assert!(matches!(secure_direct, MessageSecurityMode::SecureDirect));
    assert!(matches!(group_e2ee, MessageSecurityMode::GroupE2ee));
    assert!(matches!(required, MessageSecurityMode::E2eeRequired));
}

#[cfg(not(feature = "blocking"))]
#[test]
fn message_security_mode_mismatches_fail_closed() {
    let core = test_core();
    let client = core
        .client(IdentitySelector::LocalAlias("alice".to_string()))
        .unwrap();
    let peer = PeerRef::parse("did:example:bob", "").unwrap();

    let secure = client.messages().send(SendMessageRequest {
        target: MessageTarget::Direct(peer.clone()),
        body: MessageBody::Text {
            text: "secret".to_string(),
            kind: MessageKind::Text,
        },
        security: MessageSecurityMode::SecureDirect,
        client_message_id: None,
        delivery: MessageDeliveryOptions::default(),
        delegated_signing: None,
    });
    assert!(matches!(
        secure,
        Err(ImError::UnsupportedCapability { capability }) if capability == "sync-secure-direct-send"
    ));

    let group_e2ee = client.messages().send(SendMessageRequest {
        target: MessageTarget::Direct(peer),
        body: MessageBody::Text {
            text: "group secret".to_string(),
            kind: MessageKind::Text,
        },
        security: MessageSecurityMode::GroupE2ee,
        client_message_id: None,
        delivery: MessageDeliveryOptions::default(),
        delegated_signing: None,
    });
    assert!(matches!(
        group_e2ee,
        Err(ImError::UnsupportedCapability { capability }) if capability == "group-e2ee"
    ));

    let attachment = client.messages().send(SendMessageRequest {
        target: MessageTarget::Group(GroupRef::parse("did:example:group").unwrap()),
        body: MessageBody::Attachment {
            input: AttachmentInput::LocalFile(PathBuf::from("image.png")),
            caption: None,
            mime_type: None,
            filename: None,
        },
        security: MessageSecurityMode::Plain,
        client_message_id: None,
        delivery: MessageDeliveryOptions::default(),
        delegated_signing: None,
    });
    assert!(matches!(
        attachment,
        Err(ImError::InvalidInput { field: Some(field), .. }) if field == "service_did"
    ));
}

#[cfg(not(feature = "blocking"))]
#[test]
fn e2ee_required_attachment_routes_to_secure_path_without_plain_fallback() {
    let core = test_core();
    let client = core
        .client(IdentitySelector::LocalAlias("alice".to_string()))
        .unwrap();

    let result = client.messages().send(SendMessageRequest {
        target: MessageTarget::Direct(PeerRef::parse("did:example:bob", "").unwrap()),
        body: MessageBody::Attachment {
            input: AttachmentInput::LocalFile(PathBuf::from("image.png")),
            caption: None,
            mime_type: None,
            filename: None,
        },
        security: MessageSecurityPolicy::E2eeRequired,
        client_message_id: None,
        delivery: MessageDeliveryOptions::default(),
        delegated_signing: None,
    });

    assert!(matches!(
        result,
        Err(ImError::UnsupportedCapability { capability }) if capability == "sync-secure-direct-send"
    ));
}

#[cfg(not(feature = "blocking"))]
#[test]
fn e2ee_required_policy_routes_direct_fail_closed_without_plaintext_fallback() {
    let core = test_core();
    let client = core
        .client(IdentitySelector::LocalAlias("alice".to_string()))
        .unwrap();
    let peer = PeerRef::parse("did:example:bob", "").unwrap();

    let result = client.messages().send(SendMessageRequest {
        target: MessageTarget::Direct(peer),
        body: MessageBody::Text {
            text: "secret".to_string(),
            kind: MessageKind::Text,
        },
        security: MessageSecurityPolicy::E2eeRequired,
        client_message_id: None,
        delivery: MessageDeliveryOptions::default(),
        delegated_signing: None,
    });

    assert!(matches!(
        result,
        Err(ImError::UnsupportedCapability { capability }) if capability == "sync-secure-direct-send"
    ));
}

#[test]
fn empty_text_returns_invalid_input() {
    let core = test_core();
    let client = core
        .client(IdentitySelector::LocalAlias("alice".to_string()))
        .unwrap();
    let peer = PeerRef::parse("bob", "awiki.info").unwrap();

    let result = client.messages().send(SendMessageRequest {
        target: MessageTarget::Direct(peer),
        body: MessageBody::Text {
            text: "   ".to_string(),
            kind: MessageKind::Text,
        },
        security: MessageSecurityMode::Plain,
        client_message_id: None,
        delivery: MessageDeliveryOptions::default(),
        delegated_signing: None,
    });

    assert!(matches!(
        result,
        Err(ImError::InvalidInput { field: Some(field), .. }) if field == "text"
    ));
}

fn test_core() -> ImCore {
    ImCore::new(test_config(), test_paths()).unwrap()
}

fn test_config() -> ImCoreConfig {
    ImCoreConfig {
        service_base_url: ServiceEndpoint::parse("https://example.test").unwrap(),
        did_domain: "awiki.info".to_string(),
        user_service_endpoint: None,
        message_service_endpoint: None,
        mail_service_endpoint: None,
        anp_service_endpoint: None,
        anp_service_did: None,
        ca_bundle: None,
        transport_policy: MessageTransportPolicy::Auto,
    }
}

fn test_paths() -> ImCorePaths {
    let root = unique_temp_root();
    ImCorePaths {
        identities: IdentityRegistryPaths {
            identity_root_dir: root.join("identities"),
            registry_path: root.join("identities").join("registry.json"),
            default_identity_path: Some(root.join("identities").join("default")),
        },
        local_state: LocalStatePaths {
            sqlite_path: root.join("local").join("im.sqlite"),
        },
        runtime: RuntimePaths {
            cache_dir: root.join("cache"),
            temp_dir: root.join("tmp"),
        },
    }
}

fn unique_temp_root() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("im-core-phase1a-{}-{nanos}", std::process::id()))
}
