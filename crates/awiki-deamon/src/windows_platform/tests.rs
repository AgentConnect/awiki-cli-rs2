use super::*;
#[test]
fn private_directory_and_file_have_only_owner_and_system_access() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("private");
    create_private_directory(&directory).unwrap();
    verify_private_path(&directory).unwrap();
    let file = create_private_file(&directory.join("secret")).unwrap();
    verify_private_handle(file.as_raw_handle() as _, SE_FILE_OBJECT).unwrap();
}
#[test]
fn dpapi_roundtrip_is_not_plaintext_and_tampering_is_rejected() {
    let original = [0x63u8; 32];
    let mut protected = protect_secret(&original).unwrap();
    assert_ne!(protected, original);
    assert_eq!(unprotect_secret(&protected).unwrap().as_slice(), &original);
    let end = protected.len() - 1;
    protected[end] ^= 0x40;
    assert!(unprotect_secret(&protected).is_err());
}
#[test]
fn state_lock_rejects_second_handle_and_releases_on_close() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("run/owner.lock");
    let first = lock_private_file(&path).unwrap();
    assert!(lock_private_file(&path)
        .unwrap_err()
        .to_string()
        .contains("daemon_state_root_busy"));
    drop(first);
    lock_private_file(&path).unwrap();
}
#[test]
fn reparse_point_is_rejected_without_changing_target() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target");
    create_private_directory(&target).unwrap();
    let link = root.path().join("link");
    std::os::windows::fs::symlink_dir(&target, &link).unwrap();
    assert!(create_private_directory(&link).is_err());
    assert!(create_private_file(&link.join("secret")).is_err());
    assert!(!target.join("secret").exists());
}

#[test]
fn state_lock_child_process_fixture() {
    let Some(root) = std::env::var_os("AWIKI_NATIVE_LOCK_CHILD_SCOPE") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let _lock = lock_private_file(&root.join("owner.lock")).unwrap();
    std::fs::write(root.join("child-ready"), b"locked").unwrap();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).unwrap();
}

#[test]
fn state_lock_is_cross_process_and_kernel_releases_it_after_termination() {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    let root = tempfile::tempdir().unwrap();
    create_private_directory(root.path()).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "windows_platform::tests::state_lock_child_process_fixture",
            "--nocapture",
        ])
        .env("AWIKI_NATIVE_LOCK_CHILD_SCOPE", root.path())
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !root.path().join("child-ready").exists() {
        if std::time::Instant::now() >= deadline || child.try_wait().unwrap().is_some() {
            let _ = child.kill();
            let _ = child.wait();
            panic!("native owner child did not become ready");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let result = lock_private_file(&root.path().join("owner.lock"));
    let busy = result
        .as_ref()
        .err()
        .is_some_and(|e| e.to_string().contains("daemon_state_root_busy"));
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(busy);
    lock_private_file(&root.path().join("owner.lock")).unwrap();
}

#[test]
fn additional_world_read_ace_is_rejected_even_under_a_private_parent() {
    let root = tempfile::tempdir().unwrap();
    create_private_directory(root.path()).unwrap();
    let path = root.path().join("test-data");
    drop(create_private_file(&path).unwrap());
    let file = open_security_handle(&path, true).unwrap();
    let sid = current_user_sid().unwrap();
    let input = wide(std::ffi::OsStr::new(&format!(
        "O:{sid}D:P(A;;FA;;;{sid})(A;;FA;;;SY)(A;;FR;;;WD)"
    )))
    .unwrap();
    let mut raw = null_mut();
    checked(
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                input.as_ptr(),
                SDDL_REVISION_1,
                &mut raw,
                null_mut(),
            )
        },
        "test descriptor",
    )
    .unwrap();
    let descriptor = PrivateDescriptor {
        allocation: LocalAllocation(raw),
    };
    let (owner, acl) = descriptor.owner_and_dacl().unwrap();
    let code = unsafe {
        SetSecurityInfo(
            file.0,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION
                | DACL_SECURITY_INFORMATION
                | PROTECTED_DACL_SECURITY_INFORMATION,
            owner,
            null_mut(),
            acl,
            null_mut(),
        )
    };
    assert_eq!(code, ERROR_SUCCESS);
    assert!(verify_private_handle(file.0, SE_FILE_OBJECT).is_err());
}
