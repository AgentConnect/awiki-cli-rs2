use super::*;
use std::sync::Arc;

fn core_with_provider(root: &std::path::Path, provider: Arc<dyn IdentityCustody>) -> crate::ImCore {
    let base = super::super::tests::core(root, "device-a");
    crate::ImCore::new_with_options(
        base.inner().sdk_config().clone(),
        base.inner().sdk_paths().clone(),
        crate::ImCoreOpenOptions::default()
            .with_pin_recovery_tenants(base.inner().pin_recovery_tenants.clone())
            .with_identity_custody_provider(provider)
            .with_identity_secret_vault(
                crate::IdentitySecretStoragePolicy::VaultRequired,
                crate::ImCoreSecretVaultOptions::new(
                    crate::vault::DeviceVaultRootKey::from_bytes([7u8; 32]),
                    root.join("vault"),
                    "workspace",
                    "device-a",
                ),
            ),
    )
    .unwrap()
}

#[tokio::test]
async fn backup_export_revalidates_same_root_after_generation_advance_and_reopen() {
    for reopen in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let custody_root = tempfile::tempdir().unwrap();
        let manager_config = || anp_identity::IdentityManagerConfig {
            state_root: custody_root.path().to_owned(),
            root_key: anp_identity::RootKeySource::Injected(anp_identity::InjectedStoreKey::new(
                "backup-race",
                [31; 32],
            )),
        };
        let provider = Arc::new(DirectAnpIdentityCustody::new(
            anp_identity::IdentityManager::initialize(manager_config()).unwrap(),
        ));
        let identity = provider
            .create_identity(ProviderCreateIdentityRequest {
                profile: ProviderDidProfile::E1,
                domain: "control.test".into(),
                port: None,
                path_segments: vec!["user".into(), "backup-race".into()],
                capabilities: ProviderCapabilities { did_wba: true },
                managed_keys: vec![
                    ProviderManagedKeySpec {
                        fragment: "root".into(),
                        role: ProviderManagedKeyRole::RootControl,
                    },
                    ProviderManagedKeySpec {
                        fragment: "request".into(),
                        role: ProviderManagedKeyRole::RequestSigning,
                    },
                ],
                services: vec![],
                agent_description_url: None,
                extensions: vec![],
            })
            .await
            .unwrap();
        let public = identity.public_identity().await.unwrap();
        let kid = public
            .active_keys
            .iter()
            .find(|k| k.purposes.contains(&ProviderKeyPurpose::RootControl))
            .unwrap()
            .kid
            .clone();
        let method = anp::authentication::find_verification_method(&public.document, &kid).unwrap();
        let anp::PublicKeyMaterial::Ed25519(key) =
            crate::internal::identity_wire::document::extract_identity_public_key(&method).unwrap()
        else {
            panic!("fixture must have an Ed25519 root")
        };
        let mut target = super::super::tests::target();
        target.did = public.reference.did.clone();
        target.root_kid = kid.clone();
        target.root_public_jwk.x = crypto::b64(&key.to_bytes());
        let mut core = core_with_provider(root.path(), provider.clone());
        let (reference, generation) = backup_identity(&core, &target).await.unwrap();
        let mut session = super::super::tests::session();
        session.selected = Some(target.clone());
        session.targets = vec![target];
        let mut operation =
            super::super::new_operation(&session, crate::pin_recovery::PinRecoveryKind::Enable)
                .unwrap();
        operation.identity = Some(reference);
        operation.identity_generation = Some(generation);
        operation.phase = crate::pin_recovery::PinRecoveryPhase::AwaitingPin;

        // This is the same public adoption boundary as sibling device-document
        // convergence. A fresh checkpoint advances generation with the same root.
        identity
            .adopt_verified_sibling_document(ProviderVerifiedRemoteDocument {
                document: public.document.clone(),
                evidence: ProviderPublicationEvidence {
                    document_version: 1,
                    registry_version: 1,
                    document_digest: format!(
                        "sha256:{}",
                        crypto::hash(&crypto::canonical(&public.document).unwrap())
                    ),
                },
            })
            .await
            .unwrap();
        let advanced = identity.pin_root_generation().await.unwrap();
        assert!(advanced > generation);
        assert_eq!(
            identity
                .export_root_for_pin_backup(ProviderPinRootBackupRequest {
                    expected_generation: generation,
                    root_kid: kid,
                    user_presence_confirmed: true,
                })
                .await
                .err()
                .unwrap()
                .code,
            IdentityProviderErrorCode::Conflict,
            "the lower custody CAS must still reject an old snapshot",
        );
        if reopen {
            let store = super::super::Store::new(&core).unwrap();
            store.save_operation(&operation).unwrap();
            drop(store);
            drop(core);
            let reopened = Arc::new(DirectAnpIdentityCustody::new(
                anp_identity::IdentityManager::open(manager_config()).unwrap(),
            ));
            core = core_with_provider(root.path(), reopened);
            operation = super::super::Store::new(&core)
                .unwrap()
                .operation(&operation.id)
                .unwrap();
        }
        let material = export(&core, &mut operation).await;
        assert!(
            material.is_ok(),
            "same-root pending backup must converge: {:?}",
            material.err()
        );
        assert_eq!(operation.identity_generation, Some(advanced));

        let original_reference = operation.identity.clone();
        operation.identity.as_mut().unwrap().identity_id = super::super::uuid();
        assert_eq!(
            super::super::error_code(&export(&core, &mut operation).await.err().unwrap()),
            "binding_changed",
        );
        assert_eq!(operation.identity_generation, Some(advanced));
        operation.identity = original_reference;
        operation.target.root_public_jwk.x = crypto::b64(&[0; 32]);
        assert!(export(&core, &mut operation).await.is_err());
        assert_eq!(operation.identity_generation, Some(advanced));
    }
}
