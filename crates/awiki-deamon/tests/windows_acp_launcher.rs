#![cfg(windows)]
//! Native offline launch, cwd, UTF-8/argv and process-tree lifetime regressions.
use serde_json::{json, Value};
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use windows_sys::Win32::{Foundation::*, System::Threading::*};

fn python() -> PathBuf {
    PathBuf::from(
        std::env::var_os("AWIKI_E2E_PYTHON_BIN")
            .expect("Select native Python with AWIKI_E2E_PYTHON_BIN"),
    )
}
fn helper(cwd: &Path, parent: u32) -> Command {
    let daemon = std::env::var_os("AWIKI_WINDOWS_ACP_TEST_DAEMON")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_awiki-deamon")));
    let mut c = Command::new(daemon);
    c.arg("__acp-windows-child")
        .arg(parent.to_string())
        .arg(cwd)
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8");
    c
}
#[test]
fn native_child_preserves_cwd_argv_stdio_and_exit_code() {
    let t = tempfile::tempdir().unwrap();
    let cwd = t.path().join("中文 workspace & literal");
    std::fs::create_dir(&cwd).unwrap();
    let mut c = helper(&cwd, std::process::id());
    let mut p = c.arg(python()).args(["-c", "import sys,os,json; print(json.dumps({'cwd':os.path.samefile(os.getcwd(),sys.argv[1]),'args':sys.argv[2:],'input':sys.stdin.read()},ensure_ascii=False))"])
        .arg(&cwd).args(["space value", "x&y", "引号\"literal", "百分号%literal"])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    p.stdin
        .take()
        .unwrap()
        .write_all("中文 JSON input".as_bytes())
        .unwrap();
    let r = p.wait_with_output().unwrap();
    assert!(
        r.status.success(),
        "private launch failed; no raw diagnostics rendered"
    );
    let value: Value = serde_json::from_slice(&r.stdout).unwrap();
    assert_eq!(
        value,
        json!({"cwd":true,"args":["space value","x&y","引号\"literal","百分号%literal"],"input":"中文 JSON input"})
    );
    let r = helper(&cwd, std::process::id())
        .arg(python())
        .args(["-c", "raise SystemExit(7)"])
        .output()
        .unwrap();
    assert_eq!(r.status.code(), Some(7));
}
#[test]
fn absent_working_directory_is_rejected_before_client_start() {
    let t = tempfile::tempdir().unwrap();
    let marker = t.path().join("must-not-start");
    let r = helper(&t.path().join("absent"), std::process::id())
        .arg(python())
        .args(["-c", "import pathlib,sys;pathlib.Path(sys.argv[1]).touch()"])
        .arg(&marker)
        .output()
        .unwrap();
    assert!(!r.status.success());
    assert!(!marker.exists());
    assert_eq!(
        String::from_utf8(r.stderr).unwrap().trim(),
        "acp_windows_child_failed"
    );
}
#[test]
fn offline_cmd_peer_accepts_extended_cwd_without_sending_a_prompt() {
    let t = tempfile::tempdir().unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let home = t.path().join("clients");
    let prepared = Command::new(python())
        .arg(root.join("scripts/testing/prepare_acp_fixture.py"))
        .arg("--root")
        .arg(&home)
        .env("PYTHONUTF8", "1")
        .output()
        .unwrap();
    assert!(prepared.status.success());
    let cwd = t.path().join("工作目录");
    std::fs::create_dir(&cwd).unwrap();
    let canonical = std::fs::canonicalize(&cwd).unwrap();
    let mut c = helper(&canonical, std::process::id());
    let mut child = c
        .arg(home.join(".local/bin/node.cmd"))
        .arg("unused-adapter-entry")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let requests = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        json!({"jsonrpc":"2.0","id":2,"method":"session/new","params":{"cwd":canonical,"mcpServers":[]}}),
        json!({"jsonrpc":"2.0","id":3,"method":"session/close","params":{"sessionId":"native-exact-session"}}),
    ];
    let mut input = child.stdin.take().unwrap();
    for request in requests {
        writeln!(input, "{request}").unwrap();
    }
    drop(input);
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "offline peer did not complete; no raw protocol rendered"
    );
    let responses = String::from_utf8(result.stdout)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str::<Value>(s).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), 3);
    assert_eq!(responses[1]["result"]["sessionId"], "native-exact-session");
    assert!(cwd.join("client-capabilities.json").is_file());
    assert!(!cwd.join("prompts.jsonl").exists());
}
struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn terminated(pid: u32) -> bool {
    let h = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if h == 0 {
        return true;
    }
    let done = unsafe { WaitForSingleObject(h, 0) } == WAIT_OBJECT_0;
    unsafe {
        CloseHandle(h);
    }
    done
}
fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "owned process boundary did not settle"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn lifetime(parent_exit: bool) {
    let t = tempfile::tempdir().unwrap();
    let marker = t.path().join("owned-pids.json");
    let mut parent = OwnedChild(
        Command::new(python())
            .args(["-c", "import time;time.sleep(120)"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let pid = if parent_exit {
        parent.0.id()
    } else {
        std::process::id()
    };
    let mut child=OwnedChild(helper(t.path(),pid).arg(python())
        .args(["-c","import os,sys,subprocess,pathlib,json,time;p=subprocess.Popen([sys.executable,'-c','import time;time.sleep(120)']);pathlib.Path(sys.argv[1]).write_text(json.dumps([os.getpid(),p.pid]));time.sleep(120)"])
        .arg(&marker).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap());
    wait_until(|| marker.is_file() || child.0.try_wait().unwrap().is_some());
    assert!(
        marker.is_file(),
        "native helper did not start offline client"
    );
    let pids: Vec<u32> = serde_json::from_slice(&std::fs::read(&marker).unwrap()).unwrap();
    if parent_exit {
        parent.0.kill().unwrap();
        parent.0.wait().unwrap();
    } else {
        child.0.kill().unwrap();
    }
    wait_until(|| child.0.try_wait().unwrap().is_some());
    wait_until(|| pids.iter().copied().all(terminated));
}
#[test]
fn helper_death_terminates_client_and_grandchild() {
    lifetime(false);
}
#[test]
fn daemon_parent_death_terminates_helper_and_client_tree() {
    lifetime(true);
}

#[test]
fn isolated_environment_retains_only_named_values_and_inherits_workspace_cwd() {
    let t = tempfile::tempdir().unwrap();
    let cwd = t.path().join("native workspace");
    std::fs::create_dir(&cwd).unwrap();
    for nested in [false, true] {
        let mut c = if nested {
            let daemon = std::env::var_os("AWIKI_WINDOWS_ACP_TEST_DAEMON")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_awiki-deamon")));
            let mut outer = helper(&std::fs::canonicalize(&cwd).unwrap(), std::process::id());
            outer
                .arg(daemon)
                .arg("__acp-windows-child")
                .arg(std::process::id().to_string())
                .arg("--inherit-cwd");
            outer
        } else {
            let mut direct = helper(Path::new("--inherit-cwd"), std::process::id());
            direct.current_dir(&cwd);
            direct
        };
        let keep = serde_json::to_string(&["SystemRoot", "PYTHONUTF8", "HERMES_HOME"]).unwrap();
        let result = c.args(["--isolated-env", &keep, "--"])
            .arg(python())
            .args(["-c", "import os,json,sys;print(json.dumps({'cwd':os.path.samefile(os.getcwd(),sys.argv[1]),'home':os.environ.get('HERMES_HOME')=='isolated-fixture','systemRoot':bool(os.environ.get('SystemRoot')),'secretAbsent':'OPENAI_API_KEY' not in os.environ,'behaviorAbsent':'HERMES_PROFILE' not in os.environ,'input':sys.stdin.read(),'args':sys.argv[2:]}))"])
            .arg(&cwd).args(["literal & value", "quote\"value"])
            .env("HERMES_HOME", "isolated-fixture")
            .env("OPENAI_API_KEY", "synthetic-unrelated-secret")
            .env("HERMES_PROFILE", "synthetic-unrelated-profile")
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let mut child = result;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"protocol-only")
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "isolated native helper failed; raw output suppressed"
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            value,
            json!({"cwd":true,"home":true,"systemRoot":true,"secretAbsent":true,"behaviorAbsent":true,"input":"protocol-only","args":["literal & value","quote\"value"]})
        );
    }
}

#[test]
fn invalid_environment_name_is_rejected_before_child_start() {
    let t = tempfile::tempdir().unwrap();
    let marker = t.path().join("must-not-start");
    let result = helper(t.path(), std::process::id())
        .args(["--isolated-env", "[\"OPENAI_API_KEY=invalid\"]", "--"])
        .arg(python())
        .args(["-c", "import pathlib,sys;pathlib.Path(sys.argv[1]).touch()"])
        .arg(&marker)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!marker.exists());
    assert_eq!(
        String::from_utf8(result.stderr).unwrap().trim(),
        "acp_windows_child_failed"
    );
}
