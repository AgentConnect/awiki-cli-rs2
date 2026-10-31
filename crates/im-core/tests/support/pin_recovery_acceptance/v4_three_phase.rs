//! 真实 V4 三阶段验收的薄编排。只调用 public Core；节点、worker 和独立 DB
//! 审计由 coordinator 控制。Q 的 PST/RAT 从不进入这个模块或 marker。
use super::*;
use serde_json::{json, Value};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

const P_CONTEXT: &str = "v4-three-phase-p";
const V_CONTEXT: &str = "v4-three-phase-v";
const P_TEST: &str = "acceptance_child_v4_old_pending";
const V_TEST: &str = "acceptance_child_v4_new_identity";

fn coordinator() -> PathBuf {
    let path = PathBuf::from(
        std::env::var_os("AWIKI_PIN_RECOVERY_V4_COORDINATOR_DIR")
            .expect("exact private V4 coordinator directory required"),
    );
    assert!(path.is_absolute(), "coordinator path must be absolute");
    let metadata = fs::symlink_metadata(&path).expect("existing coordinator directory");
    assert!(metadata.is_dir(), "coordinator must be a real directory");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
    }
    path
}

fn scope(input: &Input) -> PathBuf {
    input
        .storage_b
        .parent()
        .expect("private fixture parent")
        .join("acceptance-v4-three-phase")
}

fn store(input: &Input, name: &str) -> PathBuf {
    assert!(matches!(name, "pending" | "successor"));
    scope(input).join(name)
}

fn create_private_dir(path: &Path) {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path).expect("fresh private V4 scope");
}

fn read_marker(path: &Path) -> Value {
    let metadata = fs::symlink_metadata(path).expect("exact stage marker metadata");
    assert!(metadata.is_file(), "stage marker must be a regular file");
    assert!(metadata.len() <= 128 * 1024, "bounded public stage marker");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    }
    // Q 的既有 create_new writer 可能先公布 inode、随后才写完 JSON。短暂
    // 等待完整编码，不把一个半写文件误当失败，也不接受一个畸形完整 marker。
    let started = Instant::now();
    let marker: Value = loop {
        let bytes = fs::read(path).expect("public stage marker");
        assert!(bytes.len() <= 128 * 1024, "bounded public stage marker");
        match serde_json::from_slice(&bytes) {
            Ok(marker) => break marker,
            Err(_) if started.elapsed() < Duration::from_secs(2) => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => panic!("invalid public V4 marker schema"),
        }
    };
    assert_eq!(marker["schemaVersion"], 1);
    marker
}

fn write_marker(name: &str, marker: Value) {
    assert_eq!(marker["schemaVersion"], 1);
    let path = coordinator().join(name);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let temporary = path.with_extension(format!("pending-{}", uuid::Uuid::new_v4()));
    let mut file = options
        .open(&temporary)
        .expect("new private public-evidence marker");
    file.write_all(&serde_json::to_vec_pretty(&marker).expect("public marker encoding"))
        .expect("public marker write");
    file.sync_all().expect("durable public stage marker");
    // hard_link 以 create-new 语义原子公布完整 marker，绝不覆盖旧运行证据。
    fs::hard_link(&temporary, &path).expect("publish a fresh complete stage marker");
    fs::remove_file(temporary).expect("remove own completed marker staging file");
    fs::File::open(path.parent().expect("marker parent"))
        .and_then(|directory| directory.sync_all())
        .expect("durable stage marker directory");
}

async fn wait_marker(name: &str) -> Value {
    let seconds = std::env::var("AWIKI_PIN_RECOVERY_V4_STAGE_TIMEOUT_SECONDS")
        .ok()
        .map(|value| value.parse::<u64>().expect("stage timeout is seconds"))
        .unwrap_or(600);
    assert!((1..=3600).contains(&seconds), "bounded explicit stage wait");
    let path = coordinator().join(name);
    let started = Instant::now();
    loop {
        match fs::symlink_metadata(&path) {
            Ok(_) => return read_marker(&path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => panic!("stage marker metadata unavailable"),
        }
        assert!(
            started.elapsed() < Duration::from_secs(seconds),
            "V4 coordinator stage timed out: {name}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn field<'a>(marker: &'a Value, name: &str) -> &'a str {
    marker[name]
        .as_str()
        .expect("required public stage identifier")
}

fn target(marker: &Value) {
    let expected = std::env::var("AWIKI_PIN_RECOVERY_SACRIFICIAL_TARGET_ID")
        .expect("exact sacrificial target ID required");
    assert!(uuid::Uuid::parse_str(&expected).is_ok());
    assert!(field(marker, "target_id") == expected);
}

fn applied_gate(marker: &Value, applied: &Value, booleans: &[&str]) {
    target(marker);
    assert!(marker["v4_operation_id"] == applied["v4_operation_id"]);
    for name in booleans {
        assert!(
            marker[*name] == true,
            "missing independent stage evidence: {name}"
        );
    }
}

fn child_command(test: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().expect("native acceptance executable"));
    command
        .args(["--exact", test, "--ignored", "--nocapture"])
        .env_remove("AWIKI_PIN_RECOVERY_TEST_ENABLE")
        .env_remove("AWIKI_PIN_RECOVERY_TEST_CUT")
        .env_remove("AWIKI_PIN_RECOVERY_TEST_MARKER")
        .env_remove("AWIKI_PIN_RECOVERY_TEST_BLOCK_REGISTER2_REALM_ID");
    command
}

fn require_binding_denial<T>(result: sdk::ImResult<T>, step: &str) {
    match result {
        Err(sdk::ImError::Service {
            code: Some(code),
            status_code: Some(409),
            ..
        }) if code == "binding_changed" => {}
        Err(_) => panic!("old pending must reach real HTTP409 binding_changed: {step}"),
        Ok(_) => panic!("old pending unexpectedly succeeded after V4: {step}"),
    }
}

fn pending_marker() -> Value {
    let marker = read_marker(&coordinator().join("p-ready.json"));
    target(&marker);
    assert!(uuid::Uuid::parse_str(field(&marker, "operation_id")).is_ok());
    assert!(marker["phase"] == "awaiting_device_confirmation");
    assert!(marker["pre_v4_status_http200"] == true);
    marker
}

/// E6 的主 test 入口。此函数从不控制节点或 worker，也不读业务数据库。
pub(super) async fn parent() {
    let input = read_input();
    assert!(input.multi_device_audience.is_some());
    assert!(
        !input.register_identity,
        "use the allocated existing disposable root"
    );
    let directory = coordinator();
    for name in [
        "p-ready.json",
        "ready.json",
        "v4-applied.json",
        "pre-denial-state.json",
        "p-rejected.json",
        "result.json",
        "post-denial-state.json",
        "old-authorization-rejected.json",
        "partial-cleanup-ready.json",
        "new-identity-usable.json",
        "new-backup-ready.json",
        "new-backup-completed.json",
        "cleanup-completed.json",
        "final.json",
    ] {
        assert!(
            !directory.join(name).try_exists().expect("stage preflight"),
            "use a fresh coordinator run; preserve previous evidence"
        );
    }
    let path = scope(&input);
    assert!(!path.try_exists().expect("fresh scope preflight"));
    create_private_dir(&path);
    let mut prepare = child_command(P_TEST);
    prepare.env("AWIKI_PIN_RECOVERY_V4_PENDING_PHASE", "prepare");
    super::child_evidence::run(&input, "v4-p-prepare", &mut prepare, 0);
    let p = pending_marker();
    println!(
        "PIN-DRIVER v4_old_pending_ready=true operation={}",
        field(&p, "operation_id")
    );

    // Q starts only after P's explicit PIN submission. A new recover attempt may
    // supersede P's old attempt, but cannot replace its already-prepared keys.
    let q = wait_marker("ready.json").await;
    target(&q);
    assert!(q["phase"] == "old_authorization_proven");
    assert!(q["previous_did"] == p["previous_did"]);
    assert!(q["pre_v4_old_pst_status"] == 200);
    let statuses = q["pre_v4_recover1_statuses"]
        .as_object()
        .expect("three actual Recover1 statuses");
    assert!(statuses.len() == 3 && statuses.values().all(|status| *status == 200));
    let now = chrono::Utc::now().timestamp();
    assert!(q["pst_exp"].as_i64().expect("real PST expiry") > now + 30);
    assert!(q["attempt_exp"].as_i64().expect("real attempt expiry") > now + 30);
    let rat_exp = q["rat_exp"].as_object().expect("three real RAT expiries");
    assert!(
        rat_exp.len() == 3
            && rat_exp
                .values()
                .all(|expiry| expiry.as_i64().is_some_and(|expiry| expiry > now + 30)),
        "Q must still have enough actual unexpired authorization for V4"
    );
    let mut transition = child_command(V_TEST);
    super::child_evidence::run(&input, "v4-v-apply", &mut transition, 0);
    let applied = read_marker(&directory.join("v4-applied.json"));
    target(&applied);
    assert!(applied["previous_did"] == p["previous_did"]);
    assert!(applied["successor_did"] != p["previous_did"]);

    // System 独立记录当前 successor Registry/DID 和 P 的 proposed device
    // 不存在的基线；没有基线就不启动 P 的旧 pending 拒绝请求。
    let baseline = wait_marker("pre-denial-state.json").await;
    applied_gate(
        &baseline,
        &applied,
        &["baseline_captured", "p_device_absent"],
    );

    let mut reject = child_command(P_TEST);
    reject.env("AWIKI_PIN_RECOVERY_V4_PENDING_PHASE", "reject");
    super::child_evidence::run(&input, "v4-p-reject", &mut reject, 0);
    let rejected = read_marker(&directory.join("p-rejected.json"));
    applied_gate(
        &rejected,
        &applied,
        &[
            "confirm_binding_changed_http409",
            "resume_binding_changed_http409",
            "no_active_identity",
        ],
    );
    let q_result = wait_marker("result.json").await;
    assert!(q_result["status"] == "passed");
    assert!(q_result["operation_id"] == q["operation_id"]);
    assert!(q_result["v4"]["v4_operation_id"] == applied["v4_operation_id"]);
    assert!(q_result["all_credentials_still_valid"] == true);
    assert!(q_result["guesses_unchanged"] == true);
    let unchanged = wait_marker("post-denial-state.json").await;
    applied_gate(
        &unchanged,
        &applied,
        &["state_unchanged", "p_device_absent"],
    );
    write_marker(
        "old-authorization-rejected.json",
        json!({
            "schemaVersion":1,"target_id":p["target_id"],
            "v4_operation_id":applied["v4_operation_id"],
            "old_pending_operation_id":p["operation_id"],
            "q_operation_id":q["operation_id"],
            "q_unexpired_old_pst_rat_rejected":true,
            "p_real_confirm_resume_rejected":true,
            "denial_state_unchanged":true,"p_device_absent":true,
        }),
    );

    // Coordinator first stops C and starts the cleanup worker, then independently
    // proves the exact old A/B deletes and retained C record before releasing us.
    let partial = wait_marker("partial-cleanup-ready.json").await;
    applied_gate(
        &partial,
        &applied,
        &[
            "c_offline",
            "ab_old_records_deleted",
            "c_old_record_retained",
            "cleanup_pending",
        ],
    );
    let v = core(&input, &store(&input, "successor"), V_CONTEXT);
    let registry_before = super::owner_registry(
        &v,
        "new identity real authenticated Registry while old cleanup pending",
    )
    .await;
    assert!(registry_before.did.as_str() == field(&applied, "successor_did"));
    assert!(registry_before
        .devices
        .iter()
        .any(|device| device.is_current));
    let unprotected = take(
        v.pin_recovery()
            .backup_status(sdk::IdentitySelector::Default)
            .await,
        "new identity authenticated backup metadata while C is offline",
    );
    assert_eq!(unprotected.state, PinRecoveryBackupState::NotEnabled);
    assert!(unprotected.error.is_none());
    assert!(unprotected.did == field(&applied, "successor_did"));
    write_marker(
        "new-identity-usable.json",
        json!({
            "schemaVersion":1,"target_id":p["target_id"],
            "v4_operation_id":applied["v4_operation_id"],
            "successor_did":applied["successor_did"],
            "registry_authenticated":true,"backup_status_not_enabled":true,
            "old_cleanup_pending_does_not_block_new_identity":true,
        }),
    );

    // Do not attempt a 3/3 backup while C is offline. The independent gate says
    // the worker is held and all nodes are ready before a new slot is created.
    let new_ready = wait_marker("new-backup-ready.json").await;
    applied_gate(
        &new_ready,
        &applied,
        &["cleanup_worker_stopped", "c_ready", "all_realms_ready"],
    );
    let phone = phone_session(&v, &input, None).await;
    require_sacrificial(&input, &phone);
    assert!(phone.target.did == field(&applied, "successor_did"));
    assert!(phone.target.can_enable && phone.target.active_epoch.is_none());
    let begun = take(
        v.pin_recovery()
            .begin_enable(EnablePinRecoveryRequest {
                session: phone.session_handle,
                identity: sdk::IdentitySelector::Default,
                user_presence: UserPresence::confirmed(),
            })
            .await,
        "begin exact new-root backup after all three Realms recover",
    );
    let completed = take(
        v.pin_recovery()
            .submit_pin(
                begun.operation_id,
                take(
                    SecretPin::new(input.pin.as_bytes().to_vec()),
                    "new-root PIN",
                ),
            )
            .await,
        "new root actual three registrations and two self-checks",
    );
    assert_eq!(completed.phase, PinRecoveryPhase::Completed);
    assert_eq!(completed.node_progress.len(), 3);
    assert!(completed
        .node_progress
        .iter()
        .all(|node| matches!(node.phase.as_str(), "registered" | "verified")));
    assert!(
        completed
            .node_progress
            .iter()
            .filter(|node| node.phase == "verified")
            .count()
            >= 2
    );
    let active = take(
        v.pin_recovery()
            .backup_status(sdk::IdentitySelector::Default)
            .await,
        "new backup is independently active",
    );
    assert_eq!(active.state, PinRecoveryBackupState::Active);
    assert!(active.error.is_none() && active.active_epoch.is_some());
    assert!(active.did == field(&applied, "successor_did"));
    assert_eq!(
        registry_before,
        super::owner_registry(&v, "backup does not change the successor Registry").await
    );
    write_marker(
        "new-backup-completed.json",
        json!({
            "schemaVersion":1,"target_id":p["target_id"],
            "v4_operation_id":applied["v4_operation_id"],
            "successor_did":applied["successor_did"],
            "new_backup_operation_id":completed.operation_id.0,
            "three_realms_registered":true,"two_realms_self_checked":true,
            "state_active":true,"active_epoch":active.active_epoch,
            "registry_unchanged":true,
        }),
    );
    let cleaned = wait_marker("cleanup-completed.json").await;
    applied_gate(
        &cleaned,
        &applied,
        &[
            "old_records_absent",
            "old_delete_receipts_verified",
            "new_records_present",
            "new_tombstones_absent",
        ],
    );
    assert!(cleaned["new_backup_operation_id"] == completed.operation_id.0);
    let final_status = take(
        v.pin_recovery()
            .backup_status(sdk::IdentitySelector::Default)
            .await,
        "new backup remains active after exact old cleanup catches up",
    );
    assert_eq!(final_status.state, PinRecoveryBackupState::Active);
    assert!(final_status.error.is_none() && final_status.active_epoch == active.active_epoch);
    assert_eq!(
        registry_before,
        super::owner_registry(
            &v,
            "new identity still authenticates after old deletion catches up",
        )
        .await
    );
    write_marker(
        "final.json",
        json!({
            "schemaVersion":1,"status":"passed","target_id":p["target_id"],
            "v4_operation_id":applied["v4_operation_id"],
            "previous_did":p["previous_did"],"successor_did":applied["successor_did"],
            "old_pending_operation_id":p["operation_id"],
            "q_operation_id":q["operation_id"],
            "new_backup_operation_id":completed.operation_id.0,
            "old_pending_real_server_rejected":true,
            "old_unexpired_pst_rat_rejected_before_physical_delete":true,
            "denial_state_unchanged":true,"p_device_absent":true,
            "new_identity_usable_during_cleanup_pending":true,
            "new_root_three_realm_backup_active":true,
            "old_exact_delete_verified_new_slot_preserved":true,
            "p_local_assets_retained_for_exact_scope_cleanup":true,
        }),
    );
    println!("PIN-DRIVER v4_three_phase=true old_pending_real_server_rejected=true old_unexpired_pst_rat_rejected_before_physical_delete=true new_identity_usable_during_cleanup_pending=true new_root_three_realm_backup_active=true old_exact_delete_verified_new_slot_preserved=true v4_operation={} new_backup_operation={}", field(&applied,"v4_operation_id"),completed.operation_id.0);
}

/// P 的两个短进程共享自己的 store；V 的本地 V4 清理看不到 P 的 journal。
pub(super) async fn pending_child() {
    let input = read_input();
    let path = store(&input, "pending");
    let mode = std::env::var("AWIKI_PIN_RECOVERY_V4_PENDING_PHASE")
        .expect("parent must select the exact P phase");
    assert!(matches!(mode.as_str(), "prepare" | "reject"));
    if mode == "prepare" {
        assert!(!path.try_exists().expect("new P store preflight"));
    } else {
        assert!(path.try_exists().expect("same P store preflight"));
    }
    let p = core(&input, &path, P_CONTEXT);
    assert!(take(p.identities().list(), "P must not project an old identity").is_empty());
    if mode == "prepare" {
        assert!(pending(&p, &input, PinRecoveryKind::Recover)
            .await
            .is_none());
        let phone = phone_session(&p, &input, None).await;
        require_sacrificial(&input, &phone);
        assert!(phone.target.can_recover && phone.target.active_epoch.is_some());
        let old_target = phone.target;
        let begun = take(
            p.pin_recovery()
                .begin_recover(BeginPinRecoveryRequest {
                    session: phone.session_handle,
                    device_label: "V4 held old pending P".into(),
                })
                .await,
            "P begin old restricted recovery",
        );
        let prepared = take(
            p.pin_recovery()
                .submit_pin(
                    begun.operation_id,
                    take(
                        SecretPin::new(input.pin.as_bytes().to_vec()),
                        "old-root PIN",
                    ),
                )
                .await,
            "P explicitly recover old root to restricted pending",
        );
        assert_eq!(prepared.phase, PinRecoveryPhase::AwaitingDeviceConfirmation);
        assert!(prepared.recovered_identity.is_none());
        let fresh = take(
            p.pin_recovery().status(prepared.operation_id.clone()).await,
            "P actual pre-V4 HTTP200 authoritative status",
        );
        assert_eq!(fresh.phase, PinRecoveryPhase::AwaitingDeviceConfirmation);
        assert!(fresh
            .allowed_actions
            .contains(&PinRecoveryAction::ConfirmDevice));
        assert!(take(p.identities().list(), "P remains unprojected").is_empty());
        write_marker(
            "p-ready.json",
            json!({
                "schemaVersion":1,"target_id":old_target.target_id.0,
                "previous_did":old_target.did,
                "previous_binding_generation":old_target.binding_generation,
                "operation_id":fresh.operation_id.0,
                "new_device_id":fresh.new_device_id,
                "phase":"awaiting_device_confirmation",
                "pre_v4_status_http200":true,"no_active_identity":true,
                "session_expires_at":phone.expires_at,
                "store_context":P_CONTEXT,
            }),
        );
        return;
    }
    let prepared = pending_marker();
    let applied = read_marker(&coordinator().join("v4-applied.json"));
    target(&applied);
    assert!(applied["previous_did"] == prepared["previous_did"]);
    let local = pending(&p, &input, PinRecoveryKind::Recover)
        .await
        .expect("P keeps its exact old pending across V's separate store");
    assert!(local.operation_id.0 == field(&prepared, "operation_id"));
    assert_eq!(local.phase, PinRecoveryPhase::AwaitingDeviceConfirmation);
    let deadline = chrono::DateTime::parse_from_rfc3339(field(&prepared, "session_expires_at"))
        .expect("P real PST expiry")
        .timestamp();
    assert!(deadline > chrono::Utc::now().timestamp() + 5);
    require_binding_denial(
        p.pin_recovery()
            .confirm_device(ConfirmRecoveredDeviceRequest {
                operation_id: local.operation_id.clone(),
                user_presence: UserPresence::confirmed(),
            })
            .await,
        "confirm",
    );
    require_binding_denial(
        p.pin_recovery()
            .resume(ResumePinRecoveryRequest {
                operation_id: local.operation_id,
                session: None,
            })
            .await,
        "resume without replacing the original session/key",
    );
    assert!(deadline > chrono::Utc::now().timestamp() + 5);
    assert!(take(p.identities().list(), "P still has no active identity").is_empty());
    write_marker(
        "p-rejected.json",
        json!({
            "schemaVersion":1,"target_id":prepared["target_id"],
            "v4_operation_id":applied["v4_operation_id"],
            "operation_id":prepared["operation_id"],
            "confirm_binding_changed_http409":true,
            "resume_binding_changed_http409":true,
            "original_pst_still_valid":true,"no_active_identity":true,
        }),
    );
}

/// V 只执行既有 V4；在 Q 的旧凭证拒绝窗口结束前不创建 successor 备份。
pub(super) async fn v4_child() {
    let input = read_input();
    assert!(input.multi_device_audience.is_some());
    let old = pending_marker();
    let path = store(&input, "successor");
    assert!(!path.try_exists().expect("fresh V store preflight"));
    let v = core(&input, &path, V_CONTEXT);
    assert!(take(v.identities().list(), "V starts without any local identity").is_empty());
    let otp = take(
        v.handle_recovery()
            .request_handle_recovery_otp(sdk::identity::HandleRecoveryOtpRequest {
                identity: None,
                full_handle: input.full_handle.clone(),
                phone: input.phone.clone(),
            })
            .await,
        "V exact Handle V4 phone challenge",
    );
    let prepared = take(
        v.handle_recovery()
            .prepare_handle_recovery(sdk::identity::HandleRecoveryPrepareRequest {
                operation_id: otp.operation_id.clone(),
                phone: input.phone.clone(),
                code: input.otp.clone(),
            })
            .await,
        "V factor verification and immutable successor root",
    );
    assert_eq!(
        prepared.phase,
        sdk::identity::HandleRecoveryPhase::ReadyToCommit
    );
    let applied = take(
        v.handle_recovery()
            .activate_handle_recovery(sdk::identity::HandleRecoveryActivateRequest {
                operation_id: otp.operation_id,
                user_presence_confirmed: true,
            })
            .await,
        "V actual V4 remote commit and local activation",
    );
    assert_eq!(applied.phase, sdk::identity::HandleRecoveryPhase::Applied);
    assert!(applied.full_handle == input.full_handle);
    assert!(applied.current_did.as_str() != field(&old, "previous_did"));
    assert!(applied.binding_generation.is_some());
    write_marker(
        "v4-applied.json",
        json!({
            "schemaVersion":1,"target_id":old["target_id"],
            "previous_did":old["previous_did"],
            "successor_did":applied.current_did.as_str(),
            "v4_operation_id":applied.operation_id,
            "successor_binding_generation":applied.binding_generation,
            "store_context":V_CONTEXT,
        }),
    );
}
