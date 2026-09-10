#![cfg(unix)]
use super::linux::{uninstall_with, UNIT_NAME};
use std::os::unix::process::ExitStatusExt;

fn output(code: i32, stdout: &str, stderr: &str) -> std::process::Output {
    std::process::Output {
        status: std::process::ExitStatus::from_raw(code << 8),
        stdout: stdout.as_bytes().to_vec(),
        stderr: stderr.as_bytes().to_vec(),
    }
}

#[test]
fn uninstall_absent_unit_is_idempotent_without_disabling() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(UNIT_NAME);
    for _ in 0..2 {
        let mut calls = Vec::new();
        uninstall_with(&path, &mut |args| {
            calls.push(args.join(" "));
            Ok(match args[1] {
                "show" => output(0, "LoadState=not-found\nActiveState=inactive\n", ""),
                "daemon-reload" => output(0, "", ""),
                _ => output(
                    1,
                    "",
                    "Failed to disable unit: Unit awiki-deamon.service does not exist",
                ),
            })
        })
        .unwrap();
        assert_eq!(
            calls,
            [
                format!("--user show --property=LoadState --property=ActiveState {UNIT_NAME}"),
                "--user daemon-reload".to_string(),
            ]
        );
        assert!(!path.exists());
    }
}

#[test]
fn uninstall_existing_unit_disables_and_removes_registration() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(UNIT_NAME);
    std::fs::write(&path, "existing unit").unwrap();
    let mut calls = Vec::new();
    uninstall_with(&path, &mut |args| {
        calls.push(args[1].to_string());
        if args[1] == "disable" {
            assert_eq!(args, ["--user", "disable", "--now", UNIT_NAME]);
            assert!(path.exists());
        }
        Ok(output(0, "LoadState=loaded\nActiveState=inactive\n", ""))
    })
    .unwrap();
    assert_eq!(calls, ["show", "disable", "daemon-reload"]);
    assert!(!path.exists());
}

#[test]
fn uninstall_does_not_ignore_live_unit_after_unit_file_removal() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(UNIT_NAME);
    let mut calls = Vec::new();
    uninstall_with(&path, &mut |args| {
        calls.push(args[1].to_string());
        Ok(output(0, "LoadState=not-found\nActiveState=active\n", ""))
    })
    .unwrap();
    assert_eq!(calls, ["show", "stop", "daemon-reload"]);
}

#[test]
fn uninstall_preserves_registration_on_query_or_disable_errors() {
    for step in ["show", "disable"] {
        for (code, diagnostic) in [(1, "Access denied"), (4, "Failed to connect to bus")] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join(UNIT_NAME);
            std::fs::write(&path, "existing unit").unwrap();
            let error = uninstall_with(&path, &mut |args| {
                assert_ne!(args[1], "daemon-reload");
                Ok(if args[1] == step {
                    output(
                        code,
                        "LoadState=not-found\nActiveState=inactive\n",
                        diagnostic,
                    )
                } else {
                    output(0, "LoadState=loaded\nActiveState=active\n", "")
                })
            })
            .unwrap_err()
            .to_string();
            assert!(
                error.contains(step) && error.contains(diagnostic),
                "{error}"
            );
            assert_eq!(std::fs::read_to_string(path).unwrap(), "existing unit");
        }
    }
}

#[test]
fn uninstall_rejects_unconfirmed_absence_and_reports_reload_failures() {
    for stdout in [
        "",
        "LoadState=not-found\n",
        "ActiveState=inactive\n",
        "LoadState=not-found\nLoadState=loaded\nActiveState=inactive\n",
    ] {
        let root = tempfile::tempdir().unwrap();
        let error = uninstall_with(&root.path().join(UNIT_NAME), &mut |args| {
            assert_eq!(args[1], "show");
            Ok(output(0, stdout, ""))
        })
        .unwrap_err()
        .to_string();
        assert!(error.contains("properties"), "{error}");
    }
    let root = tempfile::tempdir().unwrap();
    let error = uninstall_with(&root.path().join(UNIT_NAME), &mut |args| {
        Ok(if args[1] == "show" {
            output(0, "LoadState=not-found\nActiveState=inactive\n", "")
        } else {
            assert_eq!(args[1], "daemon-reload");
            output(1, "", "Failed to connect to bus")
        })
    })
    .unwrap_err()
    .to_string();
    assert!(error.contains("daemon-reload") && error.contains("Failed to connect to bus"));
}
