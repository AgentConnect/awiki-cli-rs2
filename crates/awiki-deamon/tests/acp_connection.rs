use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use awiki_deamon::plugins::acp::connection::{
    AcpConnectionTimeouts, AcpProcessPool, AcpProcessSpec,
};
use awiki_deamon::plugins::acp::protocol::AcpPermissionPolicy;
use tempfile::TempDir;

const STUB_ACP_SERVER: &str = r#"
import json
import os
import sys
import time

mode = sys.argv[1]
expected_permission = sys.argv[2]

def read_frame():
    line = sys.stdin.readline()
    if not line:
        raise SystemExit(2)
    return json.loads(line)

def send_frame(frame):
    sys.stdout.write(json.dumps(frame, separators=(",", ":")) + "\n")
    sys.stdout.flush()

initialize = read_frame()
if mode == "initialize_timeout":
    time.sleep(5)
    raise SystemExit(0)
if mode == "exit_with_secret":
    sys.stderr.write("authentication failed for sk-1234567890abcdefghijklmnop\n")
    sys.stderr.flush()
    raise SystemExit(17)
if mode == "env_isolated" and (os.getenv("HOME") is not None or os.getenv("ACP_ALLOWED") != "yes"):
    sys.stderr.write("ACP child environment was not isolated\n")
    sys.stderr.flush()
    raise SystemExit(19)
send_frame({
    "jsonrpc": "2.0",
    "id": initialize["id"],
    "result": {
        "protocolVersion": 1,
        "agentInfo": {"name": "stub-acp", "version": "1"},
        "agentCapabilities": {
            "promptCapabilities": {"image": False, "audio": False, "embeddedContext": False}
        },
        "authMethods": [],
    },
})

new_session = read_frame()
if mode == "session_new_timeout":
    time.sleep(5)
    raise SystemExit(0)
send_frame({
    "jsonrpc": "2.0",
    "id": new_session["id"],
    "result": {"sessionId": "stub-session"},
})

while True:
    prompt = read_frame()
    if prompt["method"] == "session/cancel":
        continue
    if mode == "cancel_inflight":
        send_frame({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {
                "sessionId": "stub-session",
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "content": {"type": "text", "text": "waiting"},
                },
            },
        })
        with open(os.environ["ACP_READY_FILE"], "w", encoding="utf-8") as ready:
            ready.write("ready")
        cancel = read_frame()
        if cancel.get("method") != "session/cancel":
            sys.stderr.write("expected session/cancel while prompt was in flight\n")
            sys.stderr.flush()
            raise SystemExit(20)
        send_frame({
            "jsonrpc": "2.0",
            "id": prompt["id"],
            "result": {"stopReason": "cancelled"},
        })
        continue
    if mode == "prompt_first_update_timeout":
        time.sleep(5)
        raise SystemExit(0)
    send_frame({
        "jsonrpc": "2.0",
        "id": "permission-1",
        "method": "session/request_permission",
        "params": {
            "sessionId": "stub-session",
            "toolCall": {"toolCallId": "call-1"},
            "options": [
                {"optionId": "allow-once", "name": "Allow once", "kind": "allow_once"},
                {"optionId": "reject-once", "name": "Reject", "kind": "reject_once"},
            ],
        },
    })
    permission = read_frame()
    selected = permission["result"]["outcome"]["optionId"]
    if selected != expected_permission:
        sys.stderr.write("unexpected permission response\n")
        sys.stderr.flush()
        raise SystemExit(18)
    send_frame({
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": {
            "sessionId": "stub-session",
            "update": {
                "sessionUpdate": "agent_message_chunk",
                "content": {"type": "text", "text": "hello "},
            },
        },
    })
    if mode == "prompt_total_timeout":
        time.sleep(5)
        raise SystemExit(0)
    send_frame({
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": {
            "sessionId": "stub-session",
            "update": {
                "sessionUpdate": "agent_message_chunk",
                "content": {"type": "text", "text": "world"},
            },
        },
    })
    send_frame({
        "jsonrpc": "2.0",
        "id": prompt["id"],
        "result": {"stopReason": "end_turn"},
    })
"#;

fn write_stub(root: &Path) -> Result<PathBuf> {
    let path = root.join("stub_acp.py");
    std::fs::write(&path, STUB_ACP_SERVER).with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}

fn spec(root: &TempDir, mode: &str, policy: AcpPermissionPolicy) -> Result<AcpProcessSpec> {
    let script = write_stub(root.path())?;
    Ok(AcpProcessSpec {
        runner_id: format!("runner-{mode}-{}", policy.option_id()),
        program: PathBuf::from("python3"),
        args: vec![
            script.display().to_string(),
            mode.to_string(),
            policy.option_id().to_string(),
        ],
        cwd: root.path().to_path_buf(),
        env: BTreeMap::new(),
        permission_policy: policy,
    })
}

fn short_timeouts() -> AcpConnectionTimeouts {
    AcpConnectionTimeouts {
        initialize: Duration::from_millis(120),
        session_new: Duration::from_millis(120),
        prompt_first_update: Duration::from_millis(120),
        prompt_total: Duration::from_millis(240),
    }
}

#[test]
fn acp_process_completes_initialize_session_prompt_and_permission() -> Result<()> {
    for policy in [
        AcpPermissionPolicy::AllowOnce,
        AcpPermissionPolicy::RejectOnce,
    ] {
        let root = tempfile::tempdir()?;
        let pool = AcpProcessPool::new(AcpConnectionTimeouts::default());
        let runner = pool.ensure_process(&spec(&root, "success", policy)?)?;
        assert_eq!(runner.connection_epoch, 1);
        assert_eq!(runner.protocol_version, 1);

        let session = pool.create_session(&runner, root.path())?;
        assert_eq!(session.session_id, "stub-session");
        let outcome = pool.submit_prompt(&runner, &session, "say hello")?;

        assert_eq!(outcome.stop_reason, "end_turn");
        assert_eq!(outcome.final_text.as_deref(), Some("hello world"));
        assert_eq!(outcome.permission_requests, 1);
    }
    Ok(())
}

#[test]
fn acp_process_uses_distinct_timeout_stages() -> Result<()> {
    for (mode, expected) in [
        ("initialize_timeout", "initialize timed out"),
        ("session_new_timeout", "session/new timed out"),
        (
            "prompt_first_update_timeout",
            "prompt first update timed out",
        ),
        ("prompt_total_timeout", "prompt total timed out"),
    ] {
        let root = tempfile::tempdir()?;
        let pool = AcpProcessPool::new(short_timeouts());
        let process_spec = spec(&root, mode, AcpPermissionPolicy::AllowOnce)?;
        let result = (|| -> Result<()> {
            let runner = pool.ensure_process(&process_spec)?;
            let session = pool.create_session(&runner, root.path())?;
            pool.submit_prompt(&runner, &session, "wait")?;
            Ok(())
        })();
        let error = result.expect_err(mode).to_string();
        assert!(
            error.contains(expected),
            "mode {mode} returned unexpected error: {error}"
        );
    }
    Ok(())
}

#[test]
fn acp_request_timeout_discards_poisoned_connection() -> Result<()> {
    for mode in ["session_new_timeout", "prompt_first_update_timeout"] {
        let root = tempfile::tempdir()?;
        let pool = AcpProcessPool::new(short_timeouts());
        let process_spec = spec(&root, mode, AcpPermissionPolicy::AllowOnce)?;
        let first = pool.ensure_process(&process_spec)?;
        if mode == "session_new_timeout" {
            pool.create_session(&first, root.path())
                .expect_err("session/new must time out");
        } else {
            let session = pool.create_session(&first, root.path())?;
            pool.submit_prompt(&first, &session, "wait")
                .expect_err("prompt must time out");
        }

        let second = pool.ensure_process(&process_spec)?;
        assert_eq!(second.connection_epoch, first.connection_epoch + 1);
    }
    Ok(())
}

#[test]
fn acp_process_exit_redacts_stderr_secrets() -> Result<()> {
    let root = tempfile::tempdir()?;
    let pool = AcpProcessPool::new(short_timeouts());
    let error = pool
        .ensure_process(&spec(
            &root,
            "exit_with_secret",
            AcpPermissionPolicy::AllowOnce,
        )?)
        .expect_err("stub must exit")
        .to_string();

    assert!(error.contains("exited"));
    assert!(!error.contains("sk-1234567890abcdefghijklmnop"));
    assert!(error.contains("<redacted>"));
    Ok(())
}

#[test]
fn acp_process_restart_increments_epoch_and_fences_old_sessions() -> Result<()> {
    let root = tempfile::tempdir()?;
    let pool = AcpProcessPool::new(AcpConnectionTimeouts::default());
    let process_spec = spec(&root, "success", AcpPermissionPolicy::AllowOnce)?;

    let first = pool.ensure_process(&process_spec)?;
    let stale_session = pool.create_session(&first, root.path())?;
    pool.terminate(&first)?;

    let second = pool.ensure_process(&process_spec)?;
    assert_eq!(second.connection_epoch, first.connection_epoch + 1);
    let error = pool
        .submit_prompt(&second, &stale_session, "must fail")
        .expect_err("old session must not cross connection epochs")
        .to_string();
    assert!(error.contains("connection epoch"));
    Ok(())
}

#[test]
fn acp_process_inherits_only_explicit_environment() -> Result<()> {
    let root = tempfile::tempdir()?;
    let pool = AcpProcessPool::new(AcpConnectionTimeouts::default());
    let mut process_spec = spec(&root, "env_isolated", AcpPermissionPolicy::AllowOnce)?;
    process_spec
        .env
        .insert("ACP_ALLOWED".to_string(), "yes".to_string());
    let runner = pool.ensure_process(&process_spec)?;
    assert_eq!(runner.protocol_version, 1);
    Ok(())
}

#[test]
fn acp_inflight_prompt_rejects_a_second_prompt_and_accepts_cancel() -> Result<()> {
    let root = tempfile::tempdir()?;
    let pool = AcpProcessPool::new(short_timeouts());
    let mut process_spec = spec(&root, "cancel_inflight", AcpPermissionPolicy::AllowOnce)?;
    let ready_file = root.path().join("prompt-ready");
    process_spec.env.insert(
        "ACP_READY_FILE".to_string(),
        ready_file.display().to_string(),
    );
    let runner = pool.ensure_process(&process_spec)?;
    let session = pool.create_session(&runner, root.path())?;

    let prompt_pool = pool.clone();
    let prompt_runner = runner.clone();
    let prompt_session = session.clone();
    let prompt_thread = thread::spawn(move || {
        prompt_pool.submit_prompt(&prompt_runner, &prompt_session, "wait until cancelled")
    });

    for _ in 0..100 {
        if ready_file.is_file() {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(ready_file.is_file(), "stub prompt did not become ready");

    let error = pool
        .submit_prompt(&runner, &session, "must not overlap")
        .expect_err("a second prompt on the same runner must be rejected")
        .to_string();
    assert!(error.contains("already has an in-flight prompt"));

    pool.cancel(&runner, &session)?;
    let outcome = prompt_thread
        .join()
        .expect("prompt thread must not panic")?;
    assert_eq!(outcome.stop_reason, "cancelled");
    assert_eq!(outcome.final_text.as_deref(), Some("waiting"));
    Ok(())
}
