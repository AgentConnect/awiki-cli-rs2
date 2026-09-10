#![cfg(unix)]
use super::*;
use std::os::unix::process::ExitStatusExt;

fn output(code: i32, stdout: &str, stderr: &str) -> std::process::Output {
    std::process::Output {
        status: std::process::ExitStatus::from_raw(code << 8),
        stdout: stdout.as_bytes().to_vec(),
        stderr: stderr.as_bytes().to_vec(),
    }
}
fn missing() -> std::process::Output {
    output(
        113,
        "",
        &format!("Could not find service \"{LABEL}\" in domain for user gui: 999"),
    )
}
fn running() -> std::process::Output {
    output(
        0,
        "gui/999/test = {\n state = running\n pid = 4321\n}\n",
        "",
    )
}

#[test]
fn systemd_mutations_fail_while_inactive_queries_remain_valid() {
    for code in [1, 3, 4, 5] {
        assert!(check_service_command_result(
            "systemctl --user restart",
            &output(code, "", "failed"),
            false
        )
        .is_err());
    }
    for code in [3, 4] {
        assert!(!check_service_command_result(
            "systemctl --user is-active",
            &output(code, "", ""),
            true
        )
        .unwrap());
    }
    assert!(check_service_command_result(
        "systemctl --user is-active",
        &output(1, "", "bus unavailable"),
        true
    )
    .is_err());
}

fn run_case(
    action: ServiceAction,
    registered: bool,
    replies: Vec<(&str, std::process::Output)>,
) -> Result<ServiceStatus> {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let path = root.path().join("agent.plist");
    if registered {
        std::fs::write(&path, "previous registration").unwrap();
    }
    let mut replies = replies.into_iter();
    let result = manage_with(
        &config,
        Path::new("/isolated/awiki-deamon"),
        action,
        &path,
        "gui/999",
        &mut |args| {
            let (expected, reply) = replies.next().expect("unexpected service command");
            assert_eq!(args[0], expected);
            assert!(!args.contains(&OsStr::new("-k")) || action == ServiceAction::Restart);
            Ok(reply)
        },
    );
    assert!(replies.next().is_none(), "expected commands not executed");
    result
}

#[test]
fn first_install_does_not_bootout_or_kickstart_and_reports_initializing() {
    let status = run_case(
        ServiceAction::Install,
        false,
        vec![
            ("print", missing()),
            ("enable", output(0, "", "")),
            ("bootstrap", output(0, "", "")),
            ("print", running()),
        ],
    )
    .unwrap();
    assert!(status.installed && status.running);
    assert!(status
        .detail
        .unwrap()
        .contains("initialization not yet confirmed"));
}

#[test]
fn reinstall_unloads_only_a_confirmed_loaded_job() {
    let status = run_case(
        ServiceAction::Install,
        true,
        vec![
            ("print", running()),
            ("bootout", output(0, "", "")),
            ("enable", output(0, "", "")),
            ("bootstrap", output(0, "", "")),
            ("print", running()),
        ],
    )
    .unwrap();
    assert!(status.running);
}

#[test]
fn unloaded_or_disabled_registration_is_enabled_before_bootstrap() {
    for action in [
        ServiceAction::Install,
        ServiceAction::Start,
        ServiceAction::Restart,
    ] {
        let status = run_case(
            action,
            true,
            vec![
                ("print", missing()),
                ("enable", output(0, "", "")),
                ("bootstrap", output(0, "", "")),
                ("print", running()),
            ],
        )
        .unwrap();
        assert!(status.running);
    }
}

#[test]
fn unloaded_stop_and_uninstall_are_quiet_and_idempotent() {
    for action in [ServiceAction::Stop, ServiceAction::Uninstall] {
        let status = run_case(
            action,
            true,
            vec![("print", missing()), ("print", missing())],
        )
        .unwrap();
        assert!(!status.running);
        assert_eq!(status.installed, action == ServiceAction::Stop);
    }
}

#[test]
fn real_command_failures_are_not_absence_or_success() {
    for step in ["print", "bootout", "enable", "bootstrap"] {
        let mut replies = Vec::new();
        for name in ["print", "bootout", "enable", "bootstrap"] {
            if name == step {
                replies.push((
                    name,
                    output(
                        5,
                        "",
                        "Input/output error\nTry running as root for richer errors.",
                    ),
                ));
                break;
            }
            replies.push((name, running()));
        }
        let error = run_case(ServiceAction::Install, true, replies)
            .unwrap_err()
            .to_string();
        assert!(error.contains(&format!("launchctl {step}")));
        assert!(!error.contains("Try running as root"));
    }
    for reply in [
        output(113, "", "Could not find domain for gui: 999"),
        output(5, "", &format!("Could not find service \"{LABEL}\"")),
    ] {
        assert!(run_case(ServiceAction::Status, true, vec![("print", reply)]).is_err());
    }
}

#[test]
fn missing_command_is_reported_without_mutating_registration() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let path = root.path().join("agent.plist");
    std::fs::write(&path, "previous").unwrap();
    assert!(manage_with(
        &config,
        Path::new("/isolated/daemon"),
        ServiceAction::Install,
        &path,
        "gui/999",
        &mut |_| { Err(std::io::Error::from(std::io::ErrorKind::NotFound)) }
    )
    .unwrap_err()
    .to_string()
    .contains("launchctl print"));
    assert_eq!(std::fs::read_to_string(path).unwrap(), "previous");
}

#[test]
fn loaded_slow_start_is_success_without_a_readiness_deadline() {
    let status = run_case(
        ServiceAction::Install,
        false,
        vec![
            ("print", missing()),
            ("enable", output(0, "", "")),
            ("bootstrap", output(0, "", "")),
            (
                "print",
                output(0, "job = {\n state = spawn scheduled\n}\n", ""),
            ),
        ],
    )
    .unwrap();
    assert!(status.installed);
    assert!(!status.running);
    assert!(status
        .detail
        .unwrap()
        .contains("Startup is not yet confirmed"));
}

#[test]
fn running_requires_top_level_pid_and_initialization_matches_that_process() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let path = root.path().join("agent.plist");
    for observed in [
        "job = {\n state = running\n}",
        "job = {\n nested = {\n state = running\n pid = 4321\n}\n}",
        "job = {\n state = exited\n pid = 4321\n}",
    ] {
        assert!(!service_status(&config, &path, Some(observed)).running);
    }
    std::fs::create_dir_all(ready_file(&config).parent().unwrap()).unwrap();
    for pid in [1234, 4321] {
        std::fs::write(
            ready_file(&config),
            serde_json::json!({"ready":true,"process_id":pid,"state_root":config.state_root})
                .to_string(),
        )
        .unwrap();
        let raw = String::from_utf8(running().stdout).unwrap();
        let status = service_status(&config, &path, Some(&raw));
        assert!(status.running);
        assert_eq!(
            status.detail.unwrap().contains("initialization complete"),
            pid == 4321
        );
    }
}
