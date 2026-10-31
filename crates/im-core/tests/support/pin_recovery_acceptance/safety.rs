use super::*;
use sdk::identity::{DeviceJoinAuthorizationStatus, DeviceRevokeRequest};
use std::io::Write;

fn owner(input: &Input) -> sdk::ImCore {
    let path = &input.storage_a;
    let label = input.storage_a_context.as_deref().unwrap_or("a");
    let options = sdk::ImCoreOpenOptions::default()
        .with_pin_recovery_tenants(vec![input.tenant.clone()])
        .with_multi_device_direct_e2ee_enabled(true)
        .with_multi_device_device_revoke_enabled(true)
        .with_multi_device_audience(
            input
                .multi_device_audience
                .clone()
                .expect("installed device audience"),
        )
        .with_identity_secret_vault(
            sdk::IdentitySecretStoragePolicy::VaultRequired,
            sdk::ImCoreSecretVaultOptions::new(
                sdk::vault::DeviceVaultRootKey::from_bytes(key(path)),
                path.join("vault"),
                format!("pin-driver-{label}"),
                format!("pin-driver-{label}"),
            ),
        );
    take(
        sdk::ImCore::new_with_options(
            sdk::ImCoreConfig {
                service_base_url: take(
                    sdk::ServiceEndpoint::parse(&input.service_base_url),
                    "safety service origin",
                ),
                did_domain: input.did_domain.clone(),
                client_version_info: Some(take(
                    sdk::ClientVersionInfo::new("awiki-me", "0815", "0.1.41", Some(53)),
                    "current host version",
                )),
                user_service_endpoint: None,
                message_service_endpoint: None,
                mail_service_endpoint: None,
                anp_service_endpoint: None,
                anp_service_did: None,
                ca_bundle: None,
                transport_policy: sdk::MessageTransportPolicy::HttpOnly,
            },
            sdk::ImCorePaths {
                identities: sdk::IdentityRegistryPaths {
                    identity_root_dir: path.join("identities"),
                    registry_path: path.join("identities/index.json"),
                    default_identity_path: Some(path.join("identities/default")),
                },
                local_state: sdk::LocalStatePaths {
                    sqlite_path: path.join("local/im.sqlite"),
                },
                runtime: sdk::RuntimePaths {
                    cache_dir: path.join("cache"),
                    temp_dir: path.join("tmp"),
                },
            },
            options,
        ),
        "safety owner with existing public revoke capability",
    )
}
async fn recover(input: &Input, name: &str) -> (sdk::ImCore, PinRecoveryProgress) {
    let candidate = fresh_recovery(input, name).await;
    let session = phone_session(&candidate, input, None).await;
    require_sacrificial(input, &session);
    let facade = candidate.pin_recovery();
    let progress = take(
        facade
            .begin_recover(BeginPinRecoveryRequest {
                session: session.session_handle,
                device_label: name.into(),
            })
            .await,
        "safety begin recovery",
    );
    let progress = take(
        facade
            .submit_pin(
                progress.operation_id,
                take(
                    SecretPin::new(input.pin.as_bytes().to_vec()),
                    "safety explicit PIN",
                ),
            )
            .await,
        "safety original root to restricted pending",
    );
    assert_eq!(progress.phase, PinRecoveryPhase::AwaitingDeviceConfirmation);
    assert!(progress.recovered_identity.is_none());
    (candidate, progress)
}
fn revoked(registry: &sdk::identity::DeviceJoinRegistrySnapshot, id: &str) -> bool {
    registry.devices.iter().any(|d| {
        d.protocol_device_id.as_str() == id
            && d.status == DeviceJoinAuthorizationStatus::Revoked
            && !d.management_ready
    })
}
pub(super) async fn concurrent_document() {
    let input = read_input();
    let owner = owner(&input);
    let original = take(
        owner.identities().resolve(sdk::IdentitySelector::Default),
        "concurrent owner identity",
    );
    let original_doc = take(
        owner
            .identities()
            .identity_document_async(sdk::IdentitySelector::Default)
            .await,
        "original public root document",
    );
    let root_id = original_doc["proof"]["verificationMethod"]
        .as_str()
        .expect("public root method");
    let root_method = original_doc["verificationMethod"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"].as_str() == Some(root_id))
        .unwrap()
        .clone();
    let (d, d_progress) = recover(&input, "acceptance-concurrent-device-d").await;
    let d_done = take(
        d.pin_recovery()
            .confirm_device(ConfirmRecoveredDeviceRequest {
                operation_id: d_progress.operation_id,
                user_presence: UserPresence::confirmed(),
            })
            .await,
        "create controlled concurrent device D",
    );
    let d_id = d_done.new_device_id.expect("actual device D");
    assert_eq!(d_done.phase, PinRecoveryPhase::Completed);
    drop(d);
    let (p, p_progress) = recover(&input, "acceptance-concurrent-pending-p").await;
    let p_id = p_progress
        .new_device_id
        .clone()
        .expect("fixed proposed P device");
    let p_operation = p_progress.operation_id.clone();
    take(
        owner
            .device_revoke()
            .revoke(DeviceRevokeRequest {
                identity: sdk::IdentitySelector::Default,
                target_device_id: take(
                    sdk::ids::ProtocolDeviceId::parse(&d_id),
                    "exact controlled D",
                ),
                user_presence_confirmed: true,
            })
            .await,
        "legitimate concurrent document and Registry revoke",
    );
    let after_revoke = owner_registry(&owner, "authoritative Registry after D revoke").await;
    assert!(revoked(&after_revoke, &d_id));
    let result = p
        .pin_recovery()
        .confirm_device(ConfirmRecoveredDeviceRequest {
            operation_id: p_operation.clone(),
            user_presence: UserPresence::confirmed(),
        })
        .await;
    let after = owner_registry(&owner, "Registry after old P proposal").await;
    assert!(
        revoked(&after, &d_id)
            && after_revoke
                .devices
                .iter()
                .all(|old| after.devices.contains(old))
    );
    match result {
        Ok(done) => {
            assert_eq!(done.phase, PinRecoveryPhase::Completed);
            assert!(done.new_device_id.as_deref() == Some(&p_id));
            assert_eq!(after.devices.len(), after_revoke.devices.len() + 1);
            let selector = done.recovered_identity.expect("same P activated");
            assert_eq!(
                take(p.identities().resolve(selector.clone()), "concurrent P DID").did,
                original.did
            );
            let document = take(
                p.identities().identity_document_async(selector).await,
                "actual committed P public document",
            );
            assert!(document["verificationMethod"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m == &root_method));
            println!("PIN-DRIVER concurrent_document_updated=true revoked_device_preserved=true same_proposed_device=true old_proposal_did_not_overwrite=true outcome=committed operation={}",p_operation.0);
        }
        Err(sdk::ImError::Service {
            code: Some(code), ..
        }) if code == "document_changed" => {
            assert_eq!(after, after_revoke);
            assert!(!after
                .devices
                .iter()
                .any(|d| d.protocol_device_id.as_str() == p_id));
            assert!(take(p.identities().list(), "rejected P inventory").is_empty());
            println!("PIN-DRIVER concurrent_document_updated=true revoked_device_preserved=true same_proposed_device=true old_proposal_did_not_overwrite=true outcome=document_changed pending_scope_retained=true operation={}",p_operation.0);
        }
        other => expect_error(
            other,
            &["document_changed"],
            "only explicit stale-document refusal is acceptable",
        ),
    }
}
pub(super) async fn revoked_committed_parent() {
    let input = read_input();
    let cut = "device_commit_response_received";
    let path = process::scope(&input, cut);
    assert!(!path.exists(), "use a fresh revoked-commit process scope");
    std::fs::create_dir_all(&path).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let owner = owner(&input);
    let before = owner_registry(&owner, "Registry before commit then revoke").await;
    child_evidence::run(
        &input,
        "commit-before-revoke",
        process::child_command("acceptance_child_commit_with_cut", cut)
            .env("AWIKI_PIN_RECOVERY_TEST_ENABLE", "1")
            .env("AWIKI_PIN_RECOVERY_TEST_CUT", cut)
            .env("AWIKI_PIN_RECOVERY_TEST_MARKER", path.join("cut.json")),
        86,
    );
    let marker = process::load_cut(&input, cut);
    let id = marker.device_id.as_deref().unwrap();
    let committed = owner_registry(&owner, "actual committed device before revoke").await;
    assert_eq!(committed.devices.len(), before.devices.len() + 1);
    take(
        owner
            .device_revoke()
            .revoke(DeviceRevokeRequest {
                identity: sdk::IdentitySelector::Default,
                target_device_id: take(
                    sdk::ids::ProtocolDeviceId::parse(id),
                    "exact committed test device",
                ),
                user_presence_confirmed: true,
            })
            .await,
        "revoke committed device before local activation",
    );
    let after = owner_registry(&owner, "Registry after exact revoke").await;
    assert!(revoked(&after, id));
    child_evidence::run(
        &input,
        "fresh-revoked-resume",
        &mut process::child_command("acceptance_child_revoked_device_resume", cut),
        0,
    );
    assert_eq!(
        after,
        owner_registry(&owner, "revoked resume has no Registry side effect").await
    );
    assert!(path.join("revoked-rejected.json").is_file());
    println!("PIN-DRIVER committed_device_revoked=true fresh_process_activation_rejected=true registry_unchanged_after_revoke=true local_identity_count=0 operation={}",marker.operation_id);
}
pub(super) async fn revoked_resume_child() {
    let input = read_input();
    let cut = "device_commit_response_received";
    let path = process::scope(&input, cut);
    let marker = process::load_cut(&input, cut);
    let candidate = core(&input, &path.join("store"), &format!("process-{cut}"));
    assert!(take(
        candidate.identities().list(),
        "revoked fresh child inventory"
    )
    .is_empty());
    let id = PinRecoveryOperationId(marker.operation_id.clone());
    expect_error(
        candidate
            .pin_recovery()
            .resume(ResumePinRecoveryRequest {
                operation_id: id,
                session: None,
            })
            .await,
        &["device_revoked"],
        "actual revoked committed device cannot locally activate",
    );
    assert!(take(
        candidate.identities().list(),
        "revoked device remains locally inactive"
    )
    .is_empty());
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path.join("revoked-rejected.json")).unwrap();
    file.write_all(&serde_json::to_vec(&serde_json::json!({"operation_id":marker.operation_id,"device_id":marker.device_id,"local_identity_count":0,"code":"device_revoked","pending_assets_retained_for_exact_cleanup":true})).unwrap()).unwrap();
    file.sync_all().unwrap();
}
