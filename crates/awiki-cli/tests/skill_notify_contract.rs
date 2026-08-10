use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

static TEMP_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn notify_skill_is_routed_and_discoverable() {
    let root = repository_root();
    let entry = fs::read_to_string(root.join("skills/SKILL.md")).expect("entry Skill");
    let notify =
        fs::read_to_string(root.join("skills/references/12-notify.md")).expect("Notify reference");
    let topic = awiki_cli::cli_docs::lookup("skills").expect("skills docs topic");

    assert!(entry.contains("| Notify |"));
    assert!(entry.contains("`references/12-notify.md`"));
    assert!(topic.references.contains(&"skills/references/12-notify.md"));

    for status in ["completed", "blocked", "action_required", "failed"] {
        assert!(notify.contains(&format!("`{status}`")));
    }
    assert!(notify.contains("awiki-cli msg send"));
    assert!(notify.contains("--dry-run"));
    assert!(notify.contains("--format json"));
}

#[test]
fn notify_skill_keeps_authorization_and_product_boundaries() {
    let root = repository_root();
    let notify =
        fs::read_to_string(root.join("skills/references/12-notify.md")).expect("Notify reference");
    let onboarding = fs::read_to_string(root.join("skills/references/01-onboarding.md"))
        .expect("Onboarding reference");

    assert!(notify.contains("current task"));
    assert!(notify.contains("Do not guess"));
    assert!(notify.contains("ordinary progress"));
    assert!(notify.contains("does not prove AWiki Me displayed"));
    assert!(notify.contains("Do not use `runtime host-notify`"));
    assert!(notify.contains("Do not use E2EE"));
    assert!(notify.contains("Do not require the Daemon"));
    assert!(notify.contains("Pass arguments directly as an argv array"));
    assert!(notify.contains("awiki-cli id current"));
    assert!(notify.contains("awiki-cli id resolve"));
    assert!(notify.contains("Do not switch identities"));
    assert!(notify.contains("Select the sender workspace before inspecting the sender"));
    assert!(notify.contains("AWIKI_CLI_WORKSPACE_HOME_DIR"));
    assert!(notify.contains("Do not silently fall back to the default CLI workspace"));
    assert!(notify.contains("Do not scan arbitrary home directories"));
    assert!(notify.contains("trusted task context"));
    assert!(onboarding.contains("Agent full Handle -> exact workspace -> local"));
    assert!(onboarding.contains("Do not store the Token with this mapping"));
    assert!(notify.contains("Dry-run is syntactic planning only"));
    assert!(notify.contains("data.plan.identity"));
    assert!(notify.contains("data.plan.target.did"));
    assert!(notify.contains("plain text may expose"));
    assert!(notify.contains("No durable send ledger"));
    assert!(notify.contains("opaque-notification-key"));
    assert!(notify.contains("--client-message-id"));
    assert!(notify.contains("--idempotency-key"));
    assert!(notify.contains("data.plan.client_message_id"));
    assert!(notify.contains("data.plan.idempotency_key"));
    assert!(notify.contains("data.plan.listener_required"));
    assert!(notify.contains("data.plan.transport_policy"));
    assert!(notify.contains("do not include a Handle, DID"));
    assert!(notify.contains("do not prove deduplication"));
    assert!(notify.contains("authorize an automatic retry"));
    assert!(notify.contains("data.delivery.accepted"));
    assert!(notify.contains("data.delivery.final_acceptance"));
    assert!(notify.contains("data.message.id"));
    assert!(notify.contains("ordinary progress"));
    assert!(notify.contains("do not retry blindly"));
    assert!(notify.contains("at most once"));
    assert!(notify.contains("For a Handle target, require"));
    assert!(notify.contains("For a DID target, require"));
    assert!(notify.contains("`data.lookup` is optional"));
    assert!(!notify.contains("resolved target matches"));
}

#[test]
fn notify_skill_is_listed_in_agent_integration_guides() {
    let root = repository_root();
    for path in [
        "docs/agent-integration.md",
        "docs/agent-integration.zh-CN.md",
    ] {
        let guide = fs::read_to_string(root.join(path)).expect("Agent integration guide");
        assert!(guide.contains("`references/12-notify.md`"), "{path}");
    }
}

#[test]
fn msg_send_dry_run_does_not_resolve_identity_or_full_handle() {
    let workspace = TempWorkspace::new();
    let output = Command::new(env!("CARGO_BIN_EXE_awiki-cli"))
        .args([
            "--identity",
            "missing-notify-sender",
            "msg",
            "send",
            "--to",
            "bob.agent-connect.cn",
            "--text",
            "notification test",
            "--dry-run",
            "--format",
            "json",
        ])
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", &workspace.path)
        .env("HOME", workspace.path.join("home"))
        .env("USERPROFILE", workspace.path.join("home"))
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .output()
        .expect("run awiki-cli dry-run");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("dry-run JSON");
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["data"]["plan"]["action"], "direct.send");
    assert_eq!(
        envelope["data"]["plan"]["identity"],
        "missing-notify-sender"
    );
    assert_eq!(
        envelope["data"]["plan"]["target"]["did"],
        "bob.agent-connect.cn"
    );
}

struct TempWorkspace {
    path: PathBuf,
}

impl TempWorkspace {
    fn new() -> Self {
        let counter = TEMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "awiki-notify-skill-{}-{nanos}-{counter}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create temporary workspace");
        Self { path }
    }
}

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
