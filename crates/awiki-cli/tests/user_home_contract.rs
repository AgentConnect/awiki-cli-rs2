use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(windows)]
#[test]
fn windows_userprofile_drives_init_and_doctor_without_home() {
    let root = TempDir::new("windows-user-home");
    let profile = root.path().join("User Profile");
    let poison_home = root.path().join("wrong-home");
    let current_dir = root.path().join("working-directory");
    std::fs::create_dir_all(&current_dir).expect("create test working directory");

    let mut init = awiki_command(&["init"], &current_dir);
    init.env_remove("HOME")
        .env("USERPROFILE", &profile)
        .env("HOMEDRIVE", "Z:")
        .env("HOMEPATH", "\\wrong-profile");
    assert_success(&init.output().expect("run awiki-cli init"));

    assert_initialized_at(&profile);
    assert!(!poison_home.join(".awiki-cli").exists());
    assert!(!current_dir.join(".awiki-cli").exists());

    let mut doctor = awiki_command(&["doctor"], &current_dir);
    doctor.env_remove("HOME").env("USERPROFILE", &profile);
    assert_success(&doctor.output().expect("run awiki-cli doctor"));

    let mut hermes = awiki_command(
        &["runtime", "host-notify", "hermes", "status"],
        &current_dir,
    );
    hermes.env_remove("HOME").env("USERPROFILE", &profile);
    let hermes = success_json(&hermes.output().expect("run Hermes status"));
    assert_eq!(
        hermes["data"]["local_hermes"]["hermes_home"],
        path_string(&profile.join(".hermes"))
    );
}

#[cfg(windows)]
#[test]
fn windows_userprofile_takes_precedence_over_home() {
    let root = TempDir::new("windows-home-precedence");
    let profile = root.path().join("profile");
    let poison_home = root.path().join("home-must-not-win");
    let current_dir = root.path().join("working-directory");
    std::fs::create_dir_all(&current_dir).expect("create test working directory");

    let mut init = awiki_command(&["init"], &current_dir);
    init.env("HOME", &poison_home).env("USERPROFILE", &profile);
    assert_success(&init.output().expect("run awiki-cli init"));

    assert_initialized_at(&profile);
    assert!(!poison_home.join(".awiki-cli").exists());
}

#[cfg(not(windows))]
#[test]
fn unix_home_keeps_precedence_over_userprofile() {
    let root = TempDir::new("unix-home-precedence");
    let home = root.path().join("home with spaces");
    let poison_profile = root.path().join("profile-must-not-win");
    let current_dir = root.path().join("working-directory");
    std::fs::create_dir_all(&current_dir).expect("create test working directory");

    let mut init = awiki_command(&["init"], &current_dir);
    init.env("HOME", &home).env("USERPROFILE", &poison_profile);
    assert_success(&init.output().expect("run awiki-cli init"));

    assert_initialized_at(&home);
    assert!(!poison_profile.join(".awiki-cli").exists());
    assert!(!current_dir.join(".awiki-cli").exists());

    let mut doctor = awiki_command(&["doctor"], &current_dir);
    doctor
        .env("HOME", &home)
        .env("USERPROFILE", &poison_profile);
    assert_success(&doctor.output().expect("run awiki-cli doctor"));

    let mut hermes = awiki_command(
        &["runtime", "host-notify", "hermes", "status"],
        &current_dir,
    );
    hermes
        .env("HOME", &home)
        .env("USERPROFILE", &poison_profile);
    let hermes = success_json(&hermes.output().expect("run Hermes status"));
    assert_eq!(
        hermes["data"]["local_hermes"]["hermes_home"],
        path_string(&home.join(".hermes"))
    );
}

fn awiki_command(args: &[&str], current_dir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .current_dir(current_dir)
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env_remove("AWIKI_CLI_WORKSPACE_HOME_DIR")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_FORMAT")
        .env_remove("AVIKI_FORMAT");
    command
}

fn assert_initialized_at(profile: &Path) {
    let workspace = profile.join(".awiki-cli");
    assert!(
        workspace.join("tenants/china/config.yaml").is_file(),
        "default config should be under {}",
        workspace.display()
    );
    assert!(
        workspace.join("tenants/china/data/awiki-cli.db").is_file(),
        "default database should be under {}",
        workspace.display()
    );
}

fn assert_success(output: &Output) {
    assert_eq!(
        output.status.code(),
        Some(0),
        "unexpected exit status; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("valid JSON envelope");
    assert_eq!(envelope["ok"], true, "unexpected output: {envelope}");
}

fn success_json(output: &Output) -> Value {
    assert_success(output);
    serde_json::from_slice(&output.stdout).expect("valid JSON envelope")
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-{label}-{}-{nonce}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("create temp directory");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
