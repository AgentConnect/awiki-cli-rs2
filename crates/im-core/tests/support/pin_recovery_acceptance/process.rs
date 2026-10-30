use super::*;
use serde::{Deserialize, Serialize};
use std::{io::Write, path::PathBuf, process::Command};

#[derive(Deserialize)]
struct CutMarker {
    schema_version: u32,
    cutpoint: String,
    operation_id: String,
    device_id: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct CompletedMarker {
    operation_id: String,
    device_id: String,
    original_did_preserved: bool,
    authenticated: bool,
}
fn scope(input: &Input, cut: &str) -> PathBuf {
    assert!(matches!(
        cut,
        "device_commit_response_received" | "local_activation_pending_saved"
    ));
    input
        .storage_b
        .parent()
        .expect("scope parent")
        .join(format!(
            "acceptance-process-{cut}{}",
            if std::env::var("AWIKI_PIN_RECOVERY_ACCEPTANCE_EXPIRE_OPERATION").as_deref() == Ok("1")
            {
                "-expired-pst-and-operation"
            } else if std::env::var("AWIKI_PIN_RECOVERY_ACCEPTANCE_WAIT_EXPIRY").as_deref()
                == Ok("1")
            {
                "-expired-pst"
            } else {
                ""
            }
        ))
}
fn child_cut() -> String {
    let cut = std::env::var("AWIKI_PIN_RECOVERY_ACCEPTANCE_CHILD")
        .expect("only an acceptance parent may invoke this child");
    assert!(matches!(
        cut.as_str(),
        "device_commit_response_received" | "local_activation_pending_saved"
    ));
    cut
}
fn child_command(name: &str, cut: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().expect("native acceptance executable"));
    command
        .args(["--exact", name, "--ignored", "--nocapture"])
        .env("AWIKI_PIN_RECOVERY_ACCEPTANCE_CHILD", cut)
        .env_remove("AWIKI_PIN_RECOVERY_TEST_ENABLE")
        .env_remove("AWIKI_PIN_RECOVERY_TEST_CUT")
        .env_remove("AWIKI_PIN_RECOVERY_TEST_MARKER");
    command
}
fn load_cut(input: &Input, cut: &str) -> CutMarker {
    let bytes =
        std::fs::read(scope(input, cut).join("cut.json")).expect("public native cut marker");
    let marker: CutMarker = serde_json::from_slice(&bytes).expect("cut marker schema");
    assert_eq!(marker.schema_version, 1);
    assert!(marker.cutpoint == cut && uuid::Uuid::parse_str(&marker.operation_id).is_ok());
    assert!(marker.device_id.as_ref().is_some_and(|id| !id.is_empty()));
    marker
}
pub(super) async fn parent(cut: &str, wait_for_expiry: bool) {
    assert_eq!(
        wait_for_expiry,
        std::env::var("AWIKI_PIN_RECOVERY_ACCEPTANCE_WAIT_EXPIRY").as_deref() == Ok("1"),
        "expiry scenario must use its explicit distinct scope"
    );
    let input = read_input();
    let path = scope(&input, cut);
    assert!(
        !path.exists(),
        "use a fresh process-cut scenario directory and preserve prior evidence"
    );
    std::fs::create_dir_all(&path).expect("private cut scope");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .expect("private cut scope permissions");
    }
    let owner = core(&input, &input.storage_b, "b");
    let before = take(
        owner
            .device_join()
            .registry(sdk::IdentitySelector::Default)
            .await,
        "Registry before child commit",
    );
    let committed = child_command("acceptance_child_commit_with_cut", cut)
        .env("AWIKI_PIN_RECOVERY_TEST_ENABLE", "1")
        .env("AWIKI_PIN_RECOVERY_TEST_CUT", cut)
        .env("AWIKI_PIN_RECOVERY_TEST_MARKER", path.join("cut.json"))
        .output()
        .expect("spawn actual commit child");
    assert_eq!(
        committed.status.code(),
        Some(86),
        "child must terminate at the exact real successful boundary"
    );
    let marker = load_cut(&input, cut);
    let after_cut = take(
        owner
            .device_join()
            .registry(sdk::IdentitySelector::Default)
            .await,
        "Registry after child exited",
    );
    assert_eq!(after_cut.devices.len(), before.devices.len() + 1);
    assert!(
        before
            .devices
            .iter()
            .all(|old| after_cut.devices.contains(old)),
        "existing devices must remain unchanged"
    );
    let device_id = marker.device_id.as_deref().expect("cut device ID");
    assert_eq!(
        after_cut
            .devices
            .iter()
            .filter(|d| d.protocol_device_id.as_str() == device_id)
            .count(),
        1
    );
    let advance_operation_deadline =
        std::env::var("AWIKI_PIN_RECOVERY_ACCEPTANCE_EXPIRE_OPERATION").as_deref() == Ok("1");
    if advance_operation_deadline {
        assert!(
            wait_for_expiry,
            "the deadline advancement case also waits for real PST expiry"
        );
        let python = std::env::var_os("AWIKI_REALM_SYSTEM_AUDIT_PYTHON")
            .expect("System expiry fixture Python required");
        let root = std::env::var_os("AWIKI_REALM_SYSTEM_AUDIT_ROOT")
            .expect("System expiry fixture owner root required");
        let expiry = Command::new(python)
            .current_dir(root)
            .args(["-m", "tests_v2.key_recovery.expiry_fixture", "--advance"])
            .arg(path.join("cut.json"))
            .output()
            .expect("exact committed operation deadline fixture");
        assert!(
            expiry.status.success(),
            "guarded exact-record deadline advancement must succeed before resuming"
        );
    }
    if wait_for_expiry {
        let expiry: serde_json::Value = serde_json::from_slice(
            &std::fs::read(path.join("session-expiry.json")).expect("real PST deadline marker"),
        )
        .expect("deadline marker schema");
        let deadline = chrono::DateTime::parse_from_rfc3339(
            expiry["expires_at"]
                .as_str()
                .expect("public session expiry"),
        )
        .expect("real session timestamp")
        .timestamp();
        let remaining = deadline
            .saturating_add(2)
            .saturating_sub(chrono::Utc::now().timestamp());
        assert!(
            (0..=610).contains(&remaining),
            "bounded actual PST expiration wait"
        );
        if remaining > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(remaining as u64)).await;
        }
        assert!(chrono::Utc::now().timestamp() > deadline);
    }
    let resumed = child_command("acceptance_child_resume_same_operation", cut)
        .output()
        .expect("spawn independent resume child");
    assert!(
        resumed.status.success(),
        "new process must reconcile and activate the exact committed device"
    );
    let completed: CompletedMarker = serde_json::from_slice(
        &std::fs::read(path.join("completed.json")).expect("completed public marker"),
    )
    .expect("completed marker schema");
    assert!(completed.operation_id == marker.operation_id && completed.device_id == device_id);
    assert!(completed.original_did_preserved && completed.authenticated);
    let final_registry = take(
        owner
            .device_join()
            .registry(sdk::IdentitySelector::Default)
            .await,
        "Registry after separate process resume",
    );
    assert_eq!(
        after_cut, final_registry,
        "resume must not commit a second device or overwrite concurrent authority"
    );
    if wait_for_expiry {
        println!("PIN-DRIVER real_pst_expired=true same_key_reauthentication=true same_committed_device=true operation={}", completed.operation_id);
    }
    println!("PIN-DRIVER cutpoint={cut} actual_child_exit=true remote_device_added_once=true cross_process_resume=true same_operation=true same_device=true original_did_preserved=true authenticated=true operation={}", completed.operation_id);
}
pub(super) async fn commit_child() {
    let cut = child_cut();
    let input = read_input();
    let candidate = core(
        &input,
        &scope(&input, &cut).join("store"),
        &format!("process-{cut}"),
    );
    assert!(take(
        candidate.identities().list(),
        "child starts without active identity"
    )
    .is_empty());
    assert!(pending(&candidate, &input, PinRecoveryKind::Recover)
        .await
        .is_none());
    let session = phone_session(&candidate, &input, None).await;
    assert!(session.target.can_recover);
    let mut expiry_options = std::fs::OpenOptions::new();
    expiry_options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        expiry_options.mode(0o600);
    }
    let mut expiry_file = expiry_options
        .open(scope(&input, &cut).join("session-expiry.json"))
        .expect("new public session deadline marker");
    expiry_file
        .write_all(
            &serde_json::to_vec(&serde_json::json!({"expires_at":session.expires_at}))
                .expect("session deadline JSON"),
        )
        .expect("deadline write");
    expiry_file.sync_all().expect("deadline sync");
    let facade = candidate.pin_recovery();
    let begun = take(
        facade
            .begin_recover(BeginPinRecoveryRequest {
                session: session.session_handle,
                device_label: format!("process cut {cut}"),
            })
            .await,
        "begin child recovery",
    );
    let recovered = take(
        facade
            .submit_pin(
                begun.operation_id.clone(),
                take(
                    SecretPin::new(input.pin.as_bytes().to_vec()),
                    "child PIN format",
                ),
            )
            .await,
        "real root recovery before child commit",
    );
    assert_eq!(
        recovered.phase,
        PinRecoveryPhase::AwaitingDeviceConfirmation
    );
    assert!(take(candidate.identities().list(), "restricted child pending").is_empty());
    let _ = take(
        facade
            .confirm_device(ConfirmRecoveredDeviceRequest {
                operation_id: begun.operation_id,
                user_presence: UserPresence::confirmed(),
            })
            .await,
        "actual device commit before cut",
    );
    panic!("required task-only process cutpoint did not terminate the child");
}
pub(super) async fn resume_child() {
    let cut = child_cut();
    let input = read_input();
    let marker = load_cut(&input, &cut);
    let candidate = core(
        &input,
        &scope(&input, &cut).join("store"),
        &format!("process-{cut}"),
    );
    assert!(take(
        candidate.identities().list(),
        "no preexisting local activation before resume"
    )
    .is_empty());
    let prior = pending(&candidate, &input, PinRecoveryKind::Recover)
        .await
        .expect("same durable pending operation survives process exit");
    assert!(prior.operation_id.0 == marker.operation_id);
    let facade = candidate.pin_recovery();
    let new_session =
        if std::env::var("AWIKI_PIN_RECOVERY_ACCEPTANCE_WAIT_EXPIRY").as_deref() == Ok("1") {
            let expired = take(
                facade.status(prior.operation_id.clone()).await,
                "expired real PST status",
            );
            assert!(expired.needs_phone_verification);
            Some(
                phone_session(&candidate, &input, Some(prior.operation_id.clone()))
                    .await
                    .session_handle,
            )
        } else {
            None
        };
    let resumed = take(
        facade
            .resume(ResumePinRecoveryRequest {
                operation_id: prior.operation_id,
                session: new_session,
            })
            .await,
        "reconcile actual committed operation",
    );
    assert_eq!(resumed.phase, PinRecoveryPhase::Completed);
    assert!(resumed.operation_id.0 == marker.operation_id);
    assert!(resumed.new_device_id == marker.device_id);
    let selector = resumed
        .recovered_identity
        .expect("same committed device activated");
    let identity = take(
        candidate.identities().resolve(selector.clone()),
        "resumed identity",
    );
    let owner = core(&input, &input.storage_b, "b");
    let original = take(
        owner.identities().resolve(sdk::IdentitySelector::Default),
        "original current identity",
    );
    assert!(identity.did == original.did);
    let _client = take(
        candidate.client_async(selector).await,
        "resumed device JWT/P5 activation",
    );
    let completed = CompletedMarker {
        operation_id: resumed.operation_id.0,
        device_id: resumed.new_device_id.expect("resumed device ID"),
        original_did_preserved: true,
        authenticated: true,
    };
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(scope(&input, &cut).join("completed.json"))
        .expect("new public completion marker");
    file.write_all(&serde_json::to_vec(&completed).expect("public completion JSON"))
        .expect("completion marker write");
    file.sync_all().expect("completion marker sync");
}
