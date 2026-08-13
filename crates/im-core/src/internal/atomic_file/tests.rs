use super::*;

#[test]
fn publish_if_absent_creates_missing_target_and_leaves_existing_target() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("record.json");
    let first = root.path().join("first.tmp");
    let second = root.path().join("second.tmp");
    std::fs::write(&first, b"first-secret").unwrap();
    std::fs::write(&second, b"second-secret").unwrap();

    assert!(publish_if_absent(&first, &target).unwrap());
    assert!(!publish_if_absent(&second, &target).unwrap());

    assert_eq!(std::fs::read(&target).unwrap(), b"first-secret");
}

#[test]
fn exclusive_publish_fallback_does_not_replace_existing_target() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("record.json");
    let temporary = root.path().join("record.tmp");
    std::fs::write(&target, b"existing").unwrap();
    std::fs::write(&temporary, b"replacement").unwrap();

    assert!(!publish_if_absent_exclusive(&temporary, &target).unwrap());
    assert_eq!(std::fs::read(&target).unwrap(), b"existing");

    std::fs::remove_file(&target).unwrap();
    assert!(publish_if_absent_exclusive(&temporary, &target).unwrap());
    assert_eq!(std::fs::read(&target).unwrap(), b"replacement");
}

#[test]
fn consecutive_replacements_keep_only_complete_images() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("state.json");

    for (index, expected) in [b"first".as_slice(), b"second", b"third"]
        .into_iter()
        .enumerate()
    {
        let temporary = root.path().join(format!("state-{index}.tmp"));
        std::fs::write(&temporary, expected).unwrap();
        std::fs::File::open(&temporary).unwrap().sync_all().unwrap();

        replace(&temporary, &target).unwrap();

        assert!(std::fs::read(&target).unwrap() == expected);
        assert!(!temporary.exists());
    }
}

#[test]
fn windows_extended_path_encoding_covers_drive_and_unc_paths() {
    let drive = format!(r"C:\AWiki\{}\attachment.awiki-part", "segment\\".repeat(32));
    let encoded = windows_extended_path_wide(drive.encode_utf16().collect()).unwrap();
    assert_eq!(encoded.last(), Some(&0));
    assert_eq!(
        String::from_utf16(&encoded[..encoded.len() - 1]).unwrap(),
        format!(r"\\?\{drive}")
    );

    let unc = r"\\server\share\AWiki\attachment.awiki-part";
    let encoded = windows_extended_path_wide(unc.encode_utf16().collect()).unwrap();
    assert_eq!(
        String::from_utf16(&encoded[..encoded.len() - 1]).unwrap(),
        r"\\?\UNC\server\share\AWiki\attachment.awiki-part"
    );
}

#[cfg(windows)]
#[test]
fn replacement_supports_a_path_longer_than_windows_max_path() {
    use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};

    let root = tempfile::tempdir().unwrap();
    let mut parent = root.path().to_path_buf();
    while parent.as_os_str().encode_wide().count() < 270 {
        parent.push("0123456789abcdef");
    }
    let encoded = windows_extended_path_wide(parent.as_os_str().encode_wide().collect()).unwrap();
    let parent =
        std::path::PathBuf::from(std::ffi::OsString::from_wide(&encoded[..encoded.len() - 1]));
    std::fs::create_dir_all(&parent).unwrap();
    let temporary = parent.join("attachment.awiki-part");
    let target = parent.join("attachment");
    std::fs::write(&temporary, b"complete").unwrap();

    replace(&temporary, &target).unwrap();

    assert_eq!(std::fs::read(&target).unwrap(), b"complete");
    assert!(!temporary.exists());
}
