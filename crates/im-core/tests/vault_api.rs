use awiki_im_core::vault::{
    parse_device_vault_root_key_b64, DeviceVaultRootKey, FileSecretVault, FileSecretVaultStore,
    SealSecretRequest, SecretAccessPolicy, SecretBytes, SecretKind, SecretMetadata, SecretVault,
    DEVICE_VAULT_ROOT_KEY_LEN,
};
use base64::Engine as _;

#[test]
fn public_vault_api_seals_opens_lists_and_deletes_secret() {
    let root = tempfile::tempdir().unwrap();
    let vault = FileSecretVault::new(
        DeviceVaultRootKey::from_bytes([13_u8; 32]),
        FileSecretVaultStore::new(root.path().join("vault")),
    );
    let metadata = SecretMetadata {
        workspace_id: "workspace-a".to_owned(),
        device_id: "device-a".to_owned(),
        identity_id: Some("identity-a".to_owned()),
        did: Some("did:wba:alice@example.com".to_owned()),
        kind: SecretKind::IdentityRootPrivate,
        key_id: "key-1".to_owned(),
        key_version: 1,
        policy: SecretAccessPolicy::no_prompt_local_secret(),
    };

    let secret_ref = vault
        .seal(SealSecretRequest {
            metadata,
            plaintext: SecretBytes::from_vec(b"external-private-key".to_vec()),
        })
        .unwrap();

    let opened = vault.open(&secret_ref).unwrap();
    assert_eq!(opened.expose_secret(), b"external-private-key");
    assert_eq!(vault.list().unwrap(), vec![secret_ref.clone()]);
    assert!(!format!("{opened:?}").contains("external-private-key"));
    assert!(!format!("{:?}", DeviceVaultRootKey::from_bytes([14_u8; 32])).contains("14"));

    vault.delete(&secret_ref).unwrap();
    assert!(vault.list().unwrap().is_empty());
}

#[test]
fn public_vault_root_key_parser_accepts_base64_and_redacts_errors() {
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([21_u8; 32]);
    let root_key = parse_device_vault_root_key_b64(&raw, "TEST_VAULT_ROOT_KEY").unwrap();
    let debug = format!("{root_key:?}");

    assert!(debug.contains("DeviceVaultRootKey"));
    assert!(debug.contains(&DEVICE_VAULT_ROOT_KEY_LEN.to_string()));
    assert!(!debug.contains(&raw));
    assert!(!debug.contains("21"));

    let err = parse_device_vault_root_key_b64("not-base64-secret-value", "TEST_VAULT_ROOT_KEY")
        .unwrap_err()
        .to_string();

    assert!(err.contains("TEST_VAULT_ROOT_KEY"));
    assert!(!err.contains("not-base64-secret-value"));
}
