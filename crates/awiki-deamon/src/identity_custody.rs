use anp_identity::host::{
    ConvergenceOutcome, ConvergenceWorkflow, EnrollmentProposalKind, EnrollmentWorkflow,
    IdentityStatusPort, MigrationKeyPurpose, MigrationPort, MigrationPrivateKey,
    MigrationPrivateKeyEncoding, RequestSigningEnrollmentRequest,
    RequestSigningIdentityImportRequest,
};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zeroize::Zeroizing;

use crate::state::{DaemonState, UserDelegatedIdentityRecord};

const PROVIDER_KEY_ID: &str = "awiki-daemon-vault";
const REFERENCE_KIND: &str = "anp_identity";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DaemonIdentityRef {
    pub kind: String,
    pub store_id: String,
    pub identity_id: String,
    pub did: String,
    pub key_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedDaemonSubkey {
    pub user_did: String,
    pub verification_method: String,
    pub public_key_multibase: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreparedDaemonSubkeyCustody {
    pub(crate) reference: DaemonIdentityRef,
    pub(crate) public_key_multibase: String,
}

pub fn prepare_daemon_subkey(
    state: &DaemonState,
    verified_document: &Value,
) -> Result<PreparedDaemonSubkey> {
    let prepared = prepare_daemon_subkey_custody(state, verified_document)?;
    Ok(PreparedDaemonSubkey {
        user_did: prepared.reference.did,
        verification_method: prepared.reference.key_id,
        public_key_multibase: prepared.public_key_multibase,
    })
}

pub(crate) fn prepare_daemon_subkey_custody(
    state: &DaemonState,
    verified_document: &Value,
) -> Result<PreparedDaemonSubkeyCustody> {
    validate_document(verified_document)?;
    let did = document_did(verified_document)?;
    let mut manager = open_manager(state)?;
    let existing = find_identity_ref(&manager, &did)?;
    let (identity_ref, key_id, public_key_multibase) = match existing {
        Some(identity_ref) => {
            let identity = manager
                .get(&identity_ref)
                .context("open daemon identity custody")?;
            match identity.public_identity()?.state {
                anp_identity::PublicIdentityState::Enrolling => {
                    let session = manager
                        .resume_enrollment(&identity_ref)?
                        .context("daemon request-signing enrollment is incomplete")?;
                    let EnrollmentProposalKind::RequestSigning { signing_key } =
                        &session.proposal().kind
                    else {
                        bail!("daemon enrollment kind changed");
                    };
                    (
                        identity_ref,
                        signing_key.kid.clone(),
                        signing_key.public_key_multibase.clone(),
                    )
                }
                anp_identity::PublicIdentityState::Active => {
                    let public = identity.public_identity()?;
                    let key = active_request_key(&public)?;
                    (
                        identity_ref,
                        key.kid.clone(),
                        public_key_multibase(public.document.as_value(), &key.kid)?,
                    )
                }
                anp_identity::PublicIdentityState::Revoked => {
                    bail!("daemon identity is revoked")
                }
            }
        }
        None => {
            let session = manager
                .begin_request_signing_enrollment(RequestSigningEnrollmentRequest {
                    remote: verified_remote_document(verified_document, 1, 1)?,
                    fragment: "daemon-key-1".to_owned(),
                    capabilities: anp_identity::host::EnrollmentCapabilities { did_wba: true },
                })
                .context("prepare daemon request-signing enrollment")?;
            let proposal = session.proposal();
            let EnrollmentProposalKind::RequestSigning { signing_key } = &proposal.kind else {
                bail!("daemon enrollment kind changed");
            };
            (
                proposal.identity.clone(),
                signing_key.kid.clone(),
                signing_key.public_key_multibase.clone(),
            )
        }
    };
    Ok(PreparedDaemonSubkeyCustody {
        reference: reference(identity_ref, key_id),
        public_key_multibase,
    })
}

pub(crate) fn import_existing_daemon_subkey(
    state: &DaemonState,
    verified_document: &Value,
    key_id: &str,
    private_key_pem: Zeroizing<String>,
) -> Result<DaemonIdentityRef> {
    validate_document(verified_document)?;
    let did = document_did(verified_document)?;
    let mut manager = open_manager(state)?;
    if let Some(identity_ref) = find_identity_ref(&manager, &did)? {
        let identity = manager
            .get(&identity_ref)
            .context("open daemon identity custody")?;
        let public = identity.public_identity()?;
        let key = active_request_key(&public)?;
        if key.kid != key_id {
            bail!("daemon identity key binding changed");
        }
        let reference = reference(identity_ref, key.kid.clone());
        drop(identity);
        drop(manager);
        adopt_daemon_document(state, &reference, verified_document)?;
        return Ok(reference);
    }
    let private = anp::PrivateKeyMaterial::from_pem(&private_key_pem)
        .context("parse imported daemon request-signing key")?;
    let anp::PrivateKeyMaterial::Ed25519(private) = private else {
        bail!("daemon request-signing key must be Ed25519");
    };
    let identity = manager
        .import_request_signing_identity(RequestSigningIdentityImportRequest {
            remote: verified_remote_document(verified_document, 1, 1)?,
            did_wba: true,
            signing_key: MigrationPrivateKey {
                kid: key_id.to_owned(),
                purpose: MigrationKeyPurpose::Authentication,
                encoding: MigrationPrivateKeyEncoding::Raw32,
                secret: Zeroizing::new(private.to_bytes().to_vec()),
            },
        })
        .context("import daemon request-signing identity")?;
    active_request_key(&identity.public_identity()?)?;
    Ok(reference(identity.reference(), key_id.to_owned()))
}

pub(crate) fn adopt_daemon_document(
    state: &DaemonState,
    reference: &DaemonIdentityRef,
    verified_document: &Value,
) -> Result<()> {
    validate_reference(reference)?;
    validate_document(verified_document)?;
    if document_did(verified_document)? != reference.did {
        bail!("daemon identity DID changed");
    }
    let manager = open_manager(state)?;
    require_store(&manager, reference)?;
    let mut identity = manager
        .get(&identity_reference(reference))
        .context("open daemon identity")?;
    let current = identity
        .host_status()?
        .checkpoint
        .context("daemon identity checkpoint missing")?;
    let digest = anp_identity::canonical_document_digest(verified_document)?;
    let changed = digest != current.document_digest;
    let outcome = identity.adopt_verified_document(verified_remote_document(
        verified_document,
        current.document_version + u64::from(changed),
        current.registry_version + u64::from(changed),
    )?)?;
    let public = identity.public_identity()?;
    if outcome == ConvergenceOutcome::Revoked
        || public.state != anp_identity::PublicIdentityState::Active
    {
        bail!("daemon request-signing authorization was revoked");
    }
    if active_request_key(&public)?.kid != reference.key_id {
        bail!("daemon identity key binding changed");
    }
    Ok(())
}

pub(crate) fn ensure_record_custody(
    state: &DaemonState,
    record: &UserDelegatedIdentityRecord,
    verified_document: &Value,
) -> Result<(DaemonIdentityRef, anp_identity::ManagedIdentity)> {
    let encoded = record
        .private_key_ref_json
        .as_deref()
        .context("delegated identity custody reference is missing")?;
    if let Ok(reference) = serde_json::from_str::<DaemonIdentityRef>(encoded) {
        adopt_daemon_document(state, &reference, verified_document)?;
        let identity = open_referenced_identity(state, &reference)?;
        return Ok((reference, identity));
    }

    let legacy_ref: im_core::vault::SecretRef =
        serde_json::from_str(encoded).context("parse legacy daemon key reference")?;
    if legacy_ref.kind != im_core::vault::SecretKind::IdentityDaemonPrivate
        || legacy_ref.key_id != record.verification_method
    {
        bail!("legacy daemon key reference binding changed");
    }
    let secret = state.open_secret_ref(&legacy_ref)?;
    let private = Zeroizing::new(
        String::from_utf8(secret.expose_secret().to_vec())
            .context("legacy daemon key is not UTF-8")?,
    );
    let reference = import_existing_daemon_subkey(
        state,
        verified_document,
        &record.verification_method,
        private,
    )?;
    state.replace_user_delegated_identity_custody_ref(
        &record.verification_method,
        encoded,
        &serde_json::to_string(&reference)?,
    )?;
    state.delete_secret_ref(&legacy_ref)?;
    let identity = open_referenced_identity(state, &reference)?;
    Ok((reference, identity))
}

pub(crate) fn open_referenced_identity(
    state: &DaemonState,
    reference: &DaemonIdentityRef,
) -> Result<anp_identity::ManagedIdentity> {
    validate_reference(reference)?;
    let manager = open_manager(state)?;
    require_store(&manager, reference)?;
    let identity = manager
        .get(&identity_reference(reference))
        .context("open referenced daemon identity")?;
    let public = identity.public_identity()?;
    if public.state != anp_identity::PublicIdentityState::Active
        || active_request_key(&public)?.kid != reference.key_id
    {
        bail!("daemon identity reference is not active");
    }
    Ok(identity)
}

pub(crate) fn verify_reference(state: &DaemonState, encoded: &str) -> Result<()> {
    let reference: DaemonIdentityRef =
        serde_json::from_str(encoded).context("parse daemon identity custody reference")?;
    open_referenced_identity(state, &reference).map(|_| ())
}

fn open_manager(state: &DaemonState) -> Result<anp_identity::IdentityManager> {
    let root = state.identity_custody_root()?;
    let config = || -> Result<anp_identity::IdentityManagerConfig> {
        Ok(anp_identity::IdentityManagerConfig {
            state_root: root.clone(),
            root_key: anp_identity::RootKeySource::Injected(anp_identity::InjectedStoreKey::new(
                PROVIDER_KEY_ID,
                state.identity_custody_root_key()?,
            )),
        })
    };
    match anp_identity::IdentityManager::open(config()?) {
        Ok(manager) => Ok(manager),
        Err(anp_identity::IdentityError::StoreNotFound) => {
            match anp_identity::IdentityManager::initialize(config()?) {
                Ok(manager) => Ok(manager),
                Err(anp_identity::IdentityError::Conflict)
                | Err(anp_identity::IdentityError::IdentityAlreadyExists) => {
                    anp_identity::IdentityManager::open(config()?)
                        .context("open concurrently initialized daemon identity store")
                }
                Err(error) => Err(error).context("initialize daemon identity store"),
            }
        }
        Err(error) => Err(error).context("open daemon identity store"),
    }
}

fn verified_remote_document(
    document: &Value,
    document_version: u64,
    registry_version: u64,
) -> Result<anp_identity::VerifiedRemoteDocument> {
    Ok(anp_identity::VerifiedRemoteDocument {
        document: serde_json::from_value(document.clone())
            .context("project verified daemon DID document")?,
        evidence: anp_identity::VerifiedPublicationEvidence {
            document_version,
            registry_version,
            document_digest: anp_identity::canonical_document_digest(document)?,
        },
    })
}

fn validate_document(document: &Value) -> Result<()> {
    if !anp::authentication::validate_did_document_binding(document, true) {
        bail!("daemon identity document binding is invalid");
    }
    Ok(())
}

fn document_did(document: &Value) -> Result<String> {
    document
        .get("id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .context("daemon identity document is missing id")
}

fn active_request_key(
    identity: &anp_identity::PublicIdentity,
) -> Result<&anp_identity::PublicKeyDescriptor> {
    identity
        .active_keys
        .iter()
        .find(|key| {
            key.algorithm == anp_identity::KeyAlgorithm::Ed25519
                && key
                    .purposes
                    .contains(&anp_identity::KeyPurpose::Authentication)
                && key
                    .purposes
                    .contains(&anp_identity::KeyPurpose::ApplicationAssertion)
        })
        .context("active daemon request-signing key is missing")
}

fn reference(identity: anp_identity::IdentityRef, key_id: String) -> DaemonIdentityRef {
    DaemonIdentityRef {
        kind: REFERENCE_KIND.to_owned(),
        store_id: identity.store_id,
        identity_id: identity.identity_id,
        did: identity.did,
        key_id,
    }
}

fn validate_reference(reference: &DaemonIdentityRef) -> Result<()> {
    if reference.kind != REFERENCE_KIND
        || reference.store_id.trim().is_empty()
        || reference.identity_id.trim().is_empty()
        || reference.did.trim().is_empty()
        || reference.key_id != format!("{}#daemon-key-1", reference.did)
    {
        bail!("invalid daemon identity custody reference");
    }
    Ok(())
}

fn require_store(
    manager: &anp_identity::IdentityManager,
    reference: &DaemonIdentityRef,
) -> Result<()> {
    if manager.info()?.store_id != reference.store_id {
        bail!("daemon identity store binding changed");
    }
    Ok(())
}

fn identity_reference(reference: &DaemonIdentityRef) -> anp_identity::IdentityRef {
    anp_identity::IdentityRef {
        store_id: reference.store_id.clone(),
        identity_id: reference.identity_id.clone(),
        did: reference.did.clone(),
    }
}

fn find_identity_ref(
    manager: &anp_identity::IdentityManager,
    did: &str,
) -> Result<Option<anp_identity::IdentityRef>> {
    Ok(manager
        .list()?
        .into_iter()
        .find(|identity| identity.reference.did == did)
        .map(|identity| identity.reference))
}

fn public_key_multibase(document: &Value, kid: &str) -> Result<String> {
    let method = document
        .get("verificationMethod")
        .and_then(Value::as_array)
        .and_then(|methods| {
            methods
                .iter()
                .find(|method| method.get("id").and_then(Value::as_str) == Some(kid))
        })
        .context("daemon request-signing verification method is missing")?;
    if let Some(value) = method.get("publicKeyMultibase").and_then(Value::as_str) {
        return Ok(value.to_owned());
    }
    let anp::PublicKeyMaterial::Ed25519(public) =
        anp::authentication::extract_public_key(method)
            .context("parse daemon request-signing public key")?
    else {
        bail!("daemon request-signing public key must be Ed25519");
    };
    let mut multicodec = Vec::with_capacity(34);
    multicodec.extend_from_slice(&[0xed, 0x01]);
    multicodec.extend_from_slice(&public.to_bytes());
    Ok(format!("z{}", bs58::encode(multicodec).into_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sign(identity: &anp_identity::ManagedIdentity, kid: &str, payload: &[u8]) {
        identity
            .sign(anp_identity::SignRequest {
                purpose: anp_identity::SigningPurpose::Authentication,
                key: anp_identity::KeySelector::Kid(kid.to_owned()),
                payload: payload.to_vec(),
            })
            .unwrap();
    }

    #[test]
    fn daemon_enrollment_restarts_and_revocation_stops_signing() {
        let root = tempfile::tempdir().unwrap();
        let config = crate::DaemonConfig::for_state_root(root.path().join("state")).unwrap();
        config.ensure_state_layout().unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();
        let source_root = tempfile::tempdir().unwrap();
        let mut source_store =
            anp_identity::DidStore::initialize_injected(source_root.path(), "source", [91; 32])
                .unwrap();
        let mut source = source_store
            .create_identity(anp_identity::DidCreateSpec {
                profile: anp_identity::DidProfile::E1,
                domain: "example.com".to_owned(),
                port: None,
                path_segments: vec!["users".to_owned(), "daemon".to_owned()],
                capabilities: anp_identity::Capabilities { did_wba: false },
                managed_keys: vec![anp_identity::ManagedKeySpec {
                    fragment: "key-1".to_owned(),
                    role: anp_identity::KeyRole::RootControl,
                }],
                external_keys: Vec::new(),
                services: Vec::new(),
                agent_description_url: None,
                extensions: Vec::new(),
            })
            .unwrap();

        let prepared = prepare_daemon_subkey_custody(&state, source.document()).unwrap();
        let repeated = prepare_daemon_subkey_custody(&state, source.document()).unwrap();
        assert_eq!(repeated, prepared);
        let public = prepare_daemon_subkey(&state, source.document()).unwrap();
        let public_json = serde_json::to_value(&public).unwrap();
        assert_eq!(public.user_did, source.did());
        assert_eq!(public.verification_method, prepared.reference.key_id);
        assert!(public_json.get("store_id").is_none());
        assert!(public_json.get("identity_id").is_none());
        assert!(public_json.get("enrollment_id").is_none());
        let update = source
            .prepare_update(anp_identity::DocumentUpdateSpec {
                request_signing_rotation: None,
                request_signing_mutations: vec![anp_identity::RequestSigningMutationSpec::Add {
                    key: anp_identity::RequestSigningPublicKeySpec {
                        kid: prepared.reference.key_id.clone(),
                        public_key_multibase: prepared.public_key_multibase.clone(),
                    },
                }],
                device_mutations: Vec::new(),
                services: None,
            })
            .unwrap();
        source.begin_publication(&update.revision_id).unwrap();
        source.mark_published(&update.revision_id).unwrap();
        source.commit_update(&update.revision_id).unwrap();
        adopt_daemon_document(&state, &prepared.reference, source.document()).unwrap();
        sign(
            &open_referenced_identity(&state, &prepared.reference).unwrap(),
            &prepared.reference.key_id,
            b"before restart",
        );

        drop(state);
        let restarted = DaemonState::open(&config).unwrap();
        let restarted_identity = open_referenced_identity(&restarted, &prepared.reference).unwrap();
        restarted_identity.recover_identity().unwrap();
        sign(
            &restarted_identity,
            &prepared.reference.key_id,
            b"after restart and reload",
        );

        let removal = source
            .prepare_update(anp_identity::DocumentUpdateSpec {
                request_signing_rotation: None,
                request_signing_mutations: vec![anp_identity::RequestSigningMutationSpec::Remove {
                    kid: prepared.reference.key_id.clone(),
                }],
                device_mutations: Vec::new(),
                services: None,
            })
            .unwrap();
        source.begin_publication(&removal.revision_id).unwrap();
        source.mark_published(&removal.revision_id).unwrap();
        source.commit_update(&removal.revision_id).unwrap();
        assert!(adopt_daemon_document(&restarted, &prepared.reference, source.document()).is_err());
        assert!(open_referenced_identity(&restarted, &prepared.reference).is_err());
    }

    #[test]
    fn legacy_vault_record_migrates_with_cutover_before_cleanup() {
        let root = tempfile::tempdir().unwrap();
        let config = crate::DaemonConfig::for_state_root(root.path().join("state")).unwrap();
        config.ensure_state_layout().unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();
        let private_key_pem =
            crate::app_bridge::secret_store::ed25519_private_key_pem_for_test(&[93_u8; 32]);
        let public_key_multibase =
            crate::app_bridge::secret_store::public_key_multibase_from_private_material(
                &private_key_pem,
            )
            .unwrap();
        let source_root = tempfile::tempdir().unwrap();
        let mut source_store =
            anp_identity::DidStore::initialize_injected(source_root.path(), "source", [94; 32])
                .unwrap();
        let source = source_store
            .create_identity(anp_identity::DidCreateSpec {
                profile: anp_identity::DidProfile::E1,
                domain: "example.com".to_owned(),
                port: None,
                path_segments: vec!["users".to_owned(), "daemon-migration".to_owned()],
                capabilities: anp_identity::Capabilities { did_wba: false },
                managed_keys: vec![anp_identity::ManagedKeySpec {
                    fragment: "key-1".to_owned(),
                    role: anp_identity::KeyRole::RootControl,
                }],
                external_keys: vec![anp_identity::ExternalPublicKeySpec {
                    kid: "#daemon-key-1".to_owned(),
                    role: anp_identity::KeyRole::RequestSigning,
                    material: anp_identity::ExternalPublicKeyMaterial::Multibase {
                        value: public_key_multibase,
                    },
                }],
                services: Vec::new(),
                agent_description_url: None,
                extensions: Vec::new(),
            })
            .unwrap();
        let key_id = format!("{}#daemon-key-1", source.did());
        let record = UserDelegatedIdentityRecord {
            user_did: source.did().to_owned(),
            verification_method: key_id.clone(),
            app_instance_id: "app-1".to_owned(),
            controller_did: source.did().to_owned(),
            daemon_agent_did: "did:agent:daemon".to_owned(),
            public_key_multibase: source
                .key_metadata(&key_id)
                .unwrap()
                .public_key_multibase
                .clone(),
            private_key_material: private_key_pem,
            private_key_ref_json: None,
            allowed_scopes_json: serde_json::json!(["message.inbox.read.plain"]),
            status: "paired_key_received".to_owned(),
            expires_at: None,
            bootstrap_id: "bootstrap-legacy".to_owned(),
            idempotency_key: "bootstrap-legacy-key".to_owned(),
            created_at_ms: 0,
            updated_at_ms: 0,
        };
        let replay = crate::state::BootstrapReplayRecord {
            bootstrap_id: record.bootstrap_id.clone(),
            idempotency_key: record.idempotency_key.clone(),
            payload_hash: "legacy-payload-hash".to_owned(),
            user_did: record.user_did.clone(),
            verification_method: record.verification_method.clone(),
            app_instance_id: record.app_instance_id.clone(),
            daemon_agent_did: record.daemon_agent_did.clone(),
            status: record.status.clone(),
            created_at_ms: 0,
            updated_at_ms: 0,
        };
        state.store_bootstrap_state(&record, &replay).unwrap();
        let legacy = state
            .load_user_delegated_identity(&key_id)
            .unwrap()
            .unwrap();
        let legacy_ref: im_core::vault::SecretRef =
            serde_json::from_str(legacy.private_key_ref_json.as_deref().unwrap()).unwrap();
        assert_eq!(
            legacy_ref.kind,
            im_core::vault::SecretKind::IdentityDaemonPrivate
        );

        let (reference, identity) =
            ensure_record_custody(&state, &legacy, source.document()).unwrap();
        sign(&identity, &key_id, b"after migration");
        let migrated = state
            .load_user_delegated_identity(&key_id)
            .unwrap()
            .unwrap();
        let migrated_ref: DaemonIdentityRef =
            serde_json::from_str(migrated.private_key_ref_json.as_deref().unwrap()).unwrap();
        assert_eq!(migrated_ref, reference);
        assert!(!state
            .secret_vault()
            .unwrap()
            .list()
            .unwrap()
            .contains(&legacy_ref));
    }
}
