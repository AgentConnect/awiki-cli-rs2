//! V2 timing and the durable boundary before provider custody import.

use super::*;
use rusqlite::OptionalExtension;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ImportTiming {
    pub(super) completion_contract: String,
    pub(super) delivery_issued_at: String,
    pub(super) input_hash: String,
}

pub(super) struct ImportGuard {
    _local: tokio::sync::OwnedMutexGuard<()>,
    _process: std::fs::File,
}

pub(super) async fn lock_process(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    message_id: &str,
    local: tokio::sync::OwnedMutexGuard<()>,
) -> crate::ImResult<ImportGuard> {
    let scope = serde_json::to_vec(&(
        client.current_identity().id.as_str(),
        client.did().as_str(),
        client.exact_protocol_device_id()?,
        message_id,
    ))
    .map_err(redacted_serialization)?;
    let path = core
        .inner()
        .sdk_paths()
        .identities
        .identity_root_dir
        .join(format!(".root-import-{:x}.lock", Sha256::digest(scope)));
    lock_file(&path, local).await
}

async fn lock_file(
    path: &std::path::Path,
    local: tokio::sync::OwnedMutexGuard<()>,
) -> crate::ImResult<ImportGuard> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    loop {
        match fs2::FileExt::try_lock_exclusive(&file) {
            Ok(()) => {
                return Ok(ImportGuard {
                    _local: local,
                    _process: file,
                })
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
            Err(error) => return Err(error.into()),
        }
    }
}

pub(super) fn input_hash(
    metadata: &V2DirectMetadata,
    body: &V2DirectBody,
    delivery: &TrustedDirectDeliveryContext,
) -> crate::ImResult<String> {
    let body = match body {
        V2DirectBody::Init(value) => serde_json::to_value(value),
        V2DirectBody::Cipher(value) => serde_json::to_value(value),
    }
    .map_err(redacted_serialization)?;
    // Physical receive source can change from realtime to mailbox on recovery.
    // The authoritative route and exact encrypted body cannot change.
    let value = serde_json::json!({
        "metadata": metadata, "body": body,
        "route": {
            "message_id": delivery.message_id, "operation_id": delivery.operation_id,
            "accepted_at": delivery.accepted_at, "sender_did": delivery.sender_did,
            "sender_device_id": delivery.sender_device_id, "recipient_did": delivery.recipient_did,
            "recipient_device_id": delivery.recipient_device_id, "method": delivery.method,
            "target_kind": delivery.target_kind, "profile": delivery.profile,
            "security_profile": delivery.security_profile, "content_type": delivery.content_type,
        },
    });
    let canonical = serde_json_canonicalizer::to_vec(&value).map_err(redacted_serialization)?;
    Ok(format!(
        "sha256:{}",
        URL_SAFE_NO_PAD.encode(Sha256::digest(canonical))
    ))
}

pub(super) fn load_plan(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    message_id: &str,
) -> crate::ImResult<Option<RootImportSealedPlan>> {
    let connection = crate::internal::local_state::open_writable(
        &core.inner().sdk_paths().local_state.sqlite_path,
    )?;
    load_plan_connection(
        &connection,
        client.current_identity().id.as_str(),
        &client.exact_protocol_device_id()?,
        message_id,
    )
}

pub(super) fn load_plan_connection(
    connection: &rusqlite::Connection,
    owner: &str,
    device: &str,
    message: &str,
) -> crate::ImResult<Option<RootImportSealedPlan>> {
    let row: Option<(String, String)> = connection.query_row(
        "SELECT owner_did, plan_json FROM identity_root_import_plan_v2 WHERE owner_identity_id=?1 AND local_device_id=?2 AND message_id=?3",
        rusqlite::params![owner, device, message], |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional().map_err(crate::internal::local_state::local_state_unavailable)?;
    row.map(|(did, json)| {
        let plan: RootImportSealedPlan =
            serde_json::from_str(&json).map_err(redacted_serialization)?;
        if plan.owner_identity_id != owner
            || plan.owner_did != did
            || plan.local_device_id != device
            || plan.message_id != message
            || plan
                .v2_timing
                .as_ref()
                .is_none_or(|timing| !is_extended_completion_contract(&timing.completion_contract))
        {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(plan)
    })
    .transpose()
}

pub(super) fn freeze_plan(
    core: &crate::core::ImCore,
    plan: &mut RootImportSealedPlan,
) -> crate::ImResult<()> {
    if plan.v2_timing.is_none() {
        return Ok(());
    }
    let connection = crate::internal::local_state::open_writable(
        &core.inner().sdk_paths().local_state.sqlite_path,
    )?;
    if load_plan_connection(
        &connection,
        &plan.owner_identity_id,
        &plan.local_device_id,
        &plan.message_id,
    )?
    .is_none()
    {
        // All route/key/Root checks have finished. Never freeze the earlier
        // network-start timestamp as the first actual import operation time.
        start_import_time(plan, root_import_now(core))?;
    }
    freeze_plan_connection(&connection, plan)
}

fn start_import_time(plan: &mut RootImportSealedPlan, now: OffsetDateTime) -> crate::ImResult<()> {
    let accepted = parse_six_microsecond_time("accepted_at", &plan.accepted_at)?;
    let ceiling = accepted
        .replace_nanosecond(0)
        .map_err(|_| crate::ImError::PermissionDenied)?
        + Duration::seconds(i64::from(accepted.nanosecond() != 0));
    let current = now
        .replace_nanosecond(0)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    plan.imported_at = format_time(ceiling.max(current))?;
    plan.now = format_time(now)?;
    Ok(())
}

fn freeze_plan_connection(
    connection: &rusqlite::Connection,
    plan: &RootImportSealedPlan,
) -> crate::ImResult<()> {
    let transaction = connection
        .unchecked_transaction()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let json = serde_json::to_string(plan).map_err(redacted_serialization)?;
    transaction.execute(
        "INSERT INTO identity_root_import_plan_v2 (owner_identity_id, owner_did, local_device_id, message_id, plan_json) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(owner_identity_id,local_device_id,message_id) DO NOTHING",
        rusqlite::params![plan.owner_identity_id, plan.owner_did, plan.local_device_id, plan.message_id, json],
    ).map_err(crate::internal::local_state::local_state_unavailable)?;
    if load_plan_connection(
        &transaction,
        &plan.owner_identity_id,
        &plan.local_device_id,
        &plan.message_id,
    )?
    .as_ref()
        != Some(plan)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    transaction
        .commit()
        .map_err(crate::internal::local_state::local_state_unavailable)
}

pub(super) fn mark_handoff(
    transaction: &rusqlite::Transaction<'_>,
    plan: &RootImportSealedPlan,
) -> crate::ImResult<()> {
    if plan.v2_timing.is_none() {
        return Ok(());
    }
    let exact = transaction.execute(
        "UPDATE identity_root_import_plan_v2 SET handoff=1 WHERE owner_identity_id=?1 AND local_device_id=?2 AND message_id=?3 AND plan_json=?4",
        rusqlite::params![plan.owner_identity_id, plan.local_device_id, plan.message_id,
            serde_json::to_string(plan).map_err(redacted_serialization)?],
    ).map_err(crate::internal::local_state::local_state_unavailable)?;
    if exact != 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

pub(super) fn proof_expired(error: &crate::ImError) -> bool {
    matches!(error, crate::ImError::Service { data: Some(data), .. }
        if data.get("awiki_code").and_then(Value::as_str) == Some("device.root_import.expired"))
}

pub(super) fn deserialize_completion_contract<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if !is_extended_completion_contract(&value) {
        return Err(serde::de::Error::custom(
            "unsupported Root completion contract",
        ));
    }
    Ok(Some(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> RootImportSealedPlan {
        RootImportSealedPlan {
            v2_timing: Some(ImportTiming {
                completion_contract: ROOT_COMPLETION_V2.to_owned(),
                delivery_issued_at: "2026-07-24T00:00:00Z".into(),
                input_hash: "sha256:encrypted-input".into(),
            }),
            owner_identity_id: "owner".into(),
            owner_did: "did:wba:example.test:e1_test".into(),
            local_device_id: "recipient".into(),
            message_id: "message".into(),
            sender_device_id: "sender".into(),
            recipient_device_id: "recipient".into(),
            sender_e2ee_key_id: "sender-key".into(),
            recipient_e2ee_key_id: "recipient-key".into(),
            accepted_at: "2026-07-24T00:00:01.000000Z".into(),
            imported_at: "2026-07-25T00:00:00Z".into(),
            envelope_expires_at: "2026-07-24T00:10:00Z".into(),
            pending_root_ref_json: "{}".into(),
            root_key_id: "root".into(),
            root_fingerprint: "root-public".into(),
            document_version: 1,
            document_hash: "approved-document".into(),
            registry_version: 2,
            now: "2026-07-25T00:00:00Z".into(),
        }
    }

    #[test]
    fn first_import_time_is_sampled_after_preflight_and_frozen_before_provider() {
        let mut value = plan();
        let finished_preflight = OffsetDateTime::parse("2026-07-25T00:00:23Z", &Rfc3339).unwrap();
        start_import_time(&mut value, finished_preflight).unwrap();
        assert_eq!(value.imported_at, "2026-07-25T00:00:23Z");
        let db = rusqlite::Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        freeze_plan_connection(&db, &value).unwrap();
        let resumed = load_plan_connection(&db, "owner", "recipient", "message")
            .unwrap()
            .unwrap();
        assert_eq!(resumed.imported_at, value.imported_at);
        freeze_plan_connection(&db, &resumed).unwrap();
    }

    #[test]
    fn import_plan_survives_reopen_and_refuses_changed_evidence() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.db");
        let original = plan();
        {
            let db = rusqlite::Connection::open(&path).unwrap();
            crate::internal::local_state::schema::ensure_schema(&db).unwrap();
            freeze_plan_connection(&db, &original).unwrap();
            // Models process death after durable plan, before or after provider import.
        }
        let db = rusqlite::Connection::open(&path).unwrap();
        let restored = load_plan_connection(&db, "owner", "recipient", "message")
            .unwrap()
            .unwrap();
        assert!(restored == original);
        freeze_plan_connection(&db, &restored).unwrap();
        for change in 0..4 {
            let mut altered = restored.clone();
            match change {
                0 => altered.imported_at = "2026-07-26T00:00:00Z".into(),
                1 => altered.v2_timing.as_mut().unwrap().input_hash = "other-ciphertext".into(),
                2 => altered.registry_version += 1,
                _ => altered.owner_did = "did:wba:other.test:e1_other".into(),
            }
            assert!(freeze_plan_connection(&db, &altered).is_err());
        }
        assert!(
            load_plan_connection(&db, "other-owner", "recipient", "message")
                .unwrap()
                .is_none()
        );
        let tx = db.unchecked_transaction().unwrap();
        persist_import_sealed_tx(&tx, &restored).unwrap();
        tx.rollback().unwrap();
        assert_eq!(
            db.query_row(
                "SELECT handoff FROM identity_root_import_plan_v2",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        let tx = db.unchecked_transaction().unwrap();
        persist_import_sealed_tx(&tx, &restored).unwrap();
        tx.commit().unwrap();
        assert_eq!(
            db.query_row(
                "SELECT handoff FROM identity_root_import_plan_v2",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
    }

    #[test]
    fn refreshed_completion_keeps_import_delivery_nonce_and_checkpoint_immutable() {
        let value = plan();
        let mut record = CompletionRecord {
            v2_timing: value.v2_timing.clone(),
            did: value.owner_did.clone(),
            local_device_id: value.local_device_id.clone(),
            message_id: value.message_id.clone(),
            sender_device_id: value.sender_device_id.clone(),
            recipient_device_id: value.recipient_device_id.clone(),
            sender_e2ee_key_id: value.sender_e2ee_key_id.clone(),
            recipient_e2ee_key_id: value.recipient_e2ee_key_id.clone(),
            imported_at: value.imported_at.clone(),
            expires_at: value.envelope_expires_at.clone(),
            pending_root_ref: RootImportCustodyRef {
                store_id: "store".into(),
                identity_id: "identity".into(),
                did: value.owner_did.clone(),
            },
            root_key_id: value.root_key_id.clone(),
            root_fingerprint: value.root_fingerprint.clone(),
            document_version: value.document_version,
            document_hash: value.document_hash.clone(),
            registry_version: value.registry_version,
            phase: RootImportCompletionPhase::CompletionPending,
            completion_params_json: None,
            completion_request_hash: None,
            completion_result_json: None,
        };
        let first_time = OffsetDateTime::parse("2026-07-25T00:00:01Z", &Rfc3339).unwrap();
        let (mut first, created) =
            completion_statement(&record, "unchanged-nonce", first_time).unwrap();
        let (mut refreshed, later) =
            completion_statement(&record, "unchanged-nonce", first_time + Duration::days(1))
                .unwrap();
        assert_ne!(created, later);
        assert_eq!(first["imported_at"], value.imported_at);
        assert_eq!(first["delivery_expires_at"], value.envelope_expires_at);
        assert_eq!(first["type"], "awiki.device.root-possession.v2");
        for key in ["proof_created_at", "expires_at"] {
            first.as_object_mut().unwrap().remove(key);
            refreshed.as_object_mut().unwrap().remove(key);
        }
        assert_eq!(first, refreshed);
        record.v2_timing.as_mut().unwrap().completion_contract = ROOT_COMPLETION_EXTENDED.into();
        let (mut extended, _) =
            completion_statement(&record, "unchanged-nonce", first_time).unwrap();
        let (mut refreshed_extended, _) =
            completion_statement(&record, "unchanged-nonce", first_time + Duration::days(1))
                .unwrap();
        assert_eq!(extended["type"], "awiki.device.root-possession.v1");
        assert_eq!(extended["expires_at"], value.envelope_expires_at);
        assert_eq!(extended["completion_contract"], ROOT_COMPLETION_EXTENDED);
        assert_ne!(
            extended["completion_proof_expires_at"],
            refreshed_extended["completion_proof_expires_at"]
        );
        for key in ["proof_created_at", "completion_proof_expires_at"] {
            extended.as_object_mut().unwrap().remove(key);
            refreshed_extended.as_object_mut().unwrap().remove(key);
        }
        assert_eq!(extended, refreshed_extended);
        record.v2_timing = None;
        let (legacy, legacy_created) =
            completion_statement(&record, "unchanged-nonce", first_time + Duration::days(1))
                .unwrap();
        assert_eq!(legacy_created, value.imported_at);
        assert_eq!(legacy["expires_at"], value.envelope_expires_at);
        assert_eq!(legacy["type"], "awiki.device.root-possession.v1");
        assert!(legacy.get("proof_created_at").is_none());
        assert!(legacy.get("delivery_issued_at").is_none());
    }

    #[test]
    fn owner_cleanup_keeps_another_owners_plan_even_for_the_same_did() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.sqlite");
        let db = crate::internal::local_state::open_writable(&path).unwrap();
        let original = plan();
        let mut other = plan();
        other.owner_identity_id = "other-owner".into();
        freeze_plan_connection(&db, &original).unwrap();
        freeze_plan_connection(&db, &other).unwrap();
        drop(db);
        crate::internal::local_state::owner_scope::delete_owner_data(
            &path,
            "owner",
            &original.owner_did,
        )
        .unwrap();
        let db = crate::internal::local_state::open_writable(&path).unwrap();
        assert!(load_plan_connection(&db, "owner", "recipient", "message")
            .unwrap()
            .is_none());
        assert!(
            load_plan_connection(&db, "other-owner", "recipient", "message")
                .unwrap()
                .as_ref()
                == Some(&other)
        );
    }

    #[tokio::test]
    async fn root_v2_lock_child() {
        let Some(directory) = std::env::var_os("AWIKI_ROOT_V2_LOCK_TEST_DIR") else {
            return;
        };
        let directory = std::path::PathBuf::from(directory);
        let path = directory.join("import.lock");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        assert_eq!(
            fs2::FileExt::try_lock_exclusive(&file).unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        std::fs::write(directory.join("denied"), []).unwrap();
        drop(file);
        let local = std::sync::Arc::new(tokio::sync::Mutex::new(()))
            .lock_owned()
            .await;
        let _guard = lock_file(&path, local).await.unwrap();
        std::fs::write(directory.join("acquired"), []).unwrap();
    }

    #[tokio::test]
    async fn receive_lock_serializes_independent_processes_until_owner_releases() {
        struct Child(std::process::Child);
        impl Drop for Child {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let local = std::sync::Arc::new(tokio::sync::Mutex::new(()))
            .lock_owned()
            .await;
        let guard = lock_file(&directory.path().join("import.lock"), local)
            .await
            .unwrap();
        let name = format!(
            "{}::root_v2_lock_child",
            module_path!().split_once("::").unwrap().1
        );
        let mut child = Child(
            std::process::Command::new(std::env::current_exe().unwrap())
                .arg("--exact")
                .arg(name)
                .arg("--nocapture")
                .env("AWIKI_ROOT_V2_LOCK_TEST_DIR", directory.path())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap(),
        );
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while !directory.path().join("denied").exists() {
                assert!(child.0.try_wait().unwrap().is_none());
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(!directory.path().join("acquired").exists());
        drop(guard);
        let status = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                if let Some(status) = child.0.try_wait().unwrap() {
                    break status;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(status.success());
        assert!(directory.path().join("acquired").exists());
    }

    #[cfg(feature = "identity-native-anp")]
    #[tokio::test]
    async fn provider_crash_cuts_reuse_frozen_evidence_without_second_import() {
        use crate::internal::identity_provider::*;

        for imported_before_crash in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let source_path = root.path().join("source");
            let recipient_path = root.path().join("recipient");
            let config = |path: &std::path::Path, key| anp_identity::IdentityManagerConfig {
                state_root: path.to_path_buf(),
                root_key: anp_identity::RootKeySource::Injected(
                    anp_identity::InjectedStoreKey::new("root-v2-test", [key; 32]),
                ),
            };
            let source_custody = DirectAnpIdentityCustody::new(
                anp_identity::IdentityManager::initialize(config(&source_path, 61)).unwrap(),
            );
            let recipient_custody = DirectAnpIdentityCustody::new(
                anp_identity::IdentityManager::initialize(config(&recipient_path, 62)).unwrap(),
            );
            let source = source_custody.create_identity(serde_json::from_value(serde_json::json!({
                "profile": "e1", "domain": "example.test", "pathSegments": ["root-v2-crash"],
                "capabilities": {"didWba": true},
                "managedKeys": [
                    {"fragment": "root", "role": "root_control"},
                    {"fragment": "sender-sign", "role": "device_signing"},
                    {"fragment": "sender-e2ee", "role": "e2ee_agreement"}
                ],
                "extensions": [{"type": "device_manifest", "value": {"devices": [{
                    "deviceId": "sender", "signingKeyId": "#sender-sign", "e2eeKeyId": "#sender-e2ee",
                    "profiles": ["anp.core.binding.v1"]
                }]}}]
            })).unwrap()).await.unwrap();
            let initial = source.public_identity().await.unwrap();
            let initial_remote = ProviderVerifiedRemoteDocument {
                document: initial.document.clone(),
                evidence: ProviderPublicationEvidence {
                    document_version: 1,
                    registry_version: 1,
                    document_digest: crate::internal::identity_wire::document::document_hash(
                        &initial.document,
                    )
                    .unwrap(),
                },
            };
            source
                .adopt_verified_document(initial_remote.clone())
                .await
                .unwrap();
            let enrollment = recipient_custody
                .begin_device_enrollment(ProviderDeviceEnrollmentRequest {
                    remote: initial_remote,
                    device_id: "recipient".into(),
                    device_signing_fragment: "recipient-sign".into(),
                    device_agreement_fragment: "recipient-e2ee".into(),
                    profiles: vec!["anp.core.binding.v1".into()],
                    capabilities: ProviderEnrollmentCapabilities { did_wba: true },
                })
                .await
                .unwrap();
            let proposal = enrollment.proposal().await.unwrap();
            let ProviderEnrollmentProposalKind::Device {
                signing_key,
                agreement_key,
                profiles,
                ..
            } = &proposal.kind
            else {
                panic!("expected exact device enrollment");
            };
            let change = source.prepare_document_change(serde_json::json!({"changes": [{"change": "add_device", "device": {
                "device_id": "recipient", "signing_key": {"kid": signing_key.kid, "public_key_multibase": signing_key.public_key_multibase},
                "agreement_key": {"kid": agreement_key.kid, "public_key_multibase": agreement_key.public_key_multibase}, "profiles": profiles
            }}]})).await.unwrap();
            let candidate = change.candidate().await.unwrap();
            let evidence = ProviderPublicationEvidence {
                document_version: 2,
                registry_version: 2,
                document_digest: candidate.candidate_digest.clone(),
            };
            let attempt = change.begin_publication().await.unwrap();
            change
                .complete(
                    attempt,
                    ProviderPublicationResult::Confirmed {
                        evidence: evidence.clone(),
                    },
                )
                .await
                .unwrap();
            let verified_evidence = ProviderPublicationEvidence {
                document_version: evidence.document_version,
                registry_version: evidence.registry_version,
                document_digest: crate::internal::identity_wire::document::document_hash(
                    &candidate.candidate_document,
                )
                .unwrap(),
            };
            enrollment
                .activate(ProviderVerifiedRemoteDocument {
                    document: candidate.candidate_document,
                    evidence: verified_evidence.clone(),
                })
                .await
                .unwrap();
            source.recover().await.unwrap();
            let root_kid = format!("{}#root", initial.reference.did);
            let exported = source
                .export_root_for_legacy_envelope(ProviderLegacyRootExportRequest {
                    key: ProviderKeySelector::Kid(root_kid.clone()),
                    user_presence_confirmed: true,
                })
                .await
                .unwrap();
            let mut frozen = plan();
            frozen.owner_did = initial.reference.did.clone();
            frozen.root_key_id = root_kid;
            frozen.root_fingerprint = proposal.root_key_fingerprint.clone();
            frozen.sender_e2ee_key_id = format!("{}#sender-e2ee", initial.reference.did);
            frozen.recipient_e2ee_key_id = agreement_key.kid.clone();
            frozen.document_version = 2;
            frozen.registry_version = 2;
            frozen.document_hash = verified_evidence.document_digest;
            frozen.pending_root_ref_json = serde_json::to_string(&RootImportCustodyRef {
                store_id: proposal.identity.store_id.clone(),
                identity_id: proposal.identity.identity_id.clone(),
                did: proposal.identity.did.clone(),
            })
            .unwrap();
            let request = |value: &RootImportSealedPlan| ProviderLegacyRootImportRequest {
                identity: proposal.identity.clone(),
                evidence: ProviderLegacyRootImportEvidence {
                    transfer_id: value.message_id.clone(),
                    source_did: value.owner_did.clone(),
                    target_did: value.owner_did.clone(),
                    sender_device_id: value.sender_device_id.clone(),
                    recipient_device_id: value.recipient_device_id.clone(),
                    recipient_agreement_kid: value.recipient_e2ee_key_id.clone(),
                    root_kid: value.root_key_id.clone(),
                    checkpoint: ProviderDocumentCheckpoint {
                        document_version: value.document_version,
                        registry_version: value.registry_version,
                        document_digest: value.document_hash.clone(),
                    },
                    accepted_at: value.imported_at.clone(),
                },
                encoding: ProviderPrivateKeyEncoding::Pkcs8Der,
                root_key: Zeroizing::new(exported.as_pkcs8_der().to_vec()),
            };
            let db_path = root.path().join("coordinator.db");
            let db = rusqlite::Connection::open(&db_path).unwrap();
            crate::internal::local_state::schema::ensure_schema(&db).unwrap();
            freeze_plan_connection(&db, &frozen).unwrap();
            let before = if imported_before_crash {
                assert_eq!(
                    recipient_custody
                        .import_legacy_root(request(&frozen))
                        .await
                        .unwrap(),
                    ProviderLegacyRootImportOutcome::Pending
                );
                Some(
                    recipient_custody
                        .open_identity(&proposal.identity)
                        .await
                        .unwrap()
                        .public_identity()
                        .await
                        .unwrap()
                        .revision,
                )
            } else {
                None
            };
            // Crash cut: neither a SQLite handoff nor any live provider handle survives.
            drop(db);
            drop(enrollment);
            drop(recipient_custody);
            let restored_custody = DirectAnpIdentityCustody::new(
                anp_identity::IdentityManager::open(config(&recipient_path, 62)).unwrap(),
            );
            let db = rusqlite::Connection::open(&db_path).unwrap();
            let restored = load_plan_connection(&db, "owner", "recipient", "message")
                .unwrap()
                .unwrap();
            assert_eq!(restored.imported_at, frozen.imported_at);
            assert_eq!(
                restored_custody
                    .import_legacy_root(request(&restored))
                    .await
                    .unwrap(),
                ProviderLegacyRootImportOutcome::Pending
            );
            let session = restored_custody
                .open_identity(&proposal.identity)
                .await
                .unwrap();
            let after = session.public_identity().await.unwrap().revision;
            if let Some(before) = before {
                assert_eq!(before, after);
            }
            assert_eq!(
                restored_custody
                    .import_legacy_root(request(&restored))
                    .await
                    .unwrap(),
                ProviderLegacyRootImportOutcome::Pending
            );
            assert_eq!(
                restored_custody
                    .open_identity(&proposal.identity)
                    .await
                    .unwrap()
                    .public_identity()
                    .await
                    .unwrap()
                    .revision,
                after
            );
            let tx = db.unchecked_transaction().unwrap();
            persist_import_sealed_tx(&tx, &restored).unwrap();
            tx.commit().unwrap();
            assert_eq!(
                db.query_row(
                    "SELECT handoff FROM identity_root_import_plan_v2",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
                1
            );
        }
    }
}
