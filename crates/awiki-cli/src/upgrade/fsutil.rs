#![allow(dead_code)]

use crate::durablefs;
use serde::Serialize;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub(super) fn path_exists(path: &str) -> bool {
    !path.is_empty() && Path::new(path).exists()
}

pub(super) fn file_exists(path: &str) -> bool {
    !path.is_empty() && Path::new(path).is_file()
}

pub(super) fn dir_exists(path: &str) -> bool {
    !path.is_empty() && Path::new(path).is_dir()
}

pub(super) fn write_atomic_file(path: &str, content: &[u8], mode: u32) -> std::io::Result<()> {
    let path = Path::new(path);
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    create_dir_all_private(parent).map_err(|err| prefix_io("create parent dir", err))?;
    let temp_path = unique_temp_path(parent);
    let mut cleanup = true;
    let result = (|| {
        let mut temp_file =
            open_temp_file(&temp_path).map_err(|err| prefix_io("create temp file", err))?;
        temp_file
            .write_all(content)
            .map_err(|err| prefix_io("write temp file", err))?;
        temp_file
            .sync_all()
            .map_err(|err| prefix_io("sync temp file", err))?;
        close_file(temp_file).map_err(|err| prefix_io("close temp file", err))?;
        set_file_mode(&temp_path, mode).map_err(|err| prefix_io("chmod temp file", err))?;
        fs::rename(&temp_path, path).map_err(|err| prefix_io("rename temp file", err))?;
        cleanup = false;
        sync_directory(parent)
    })();
    if cleanup {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

pub(super) fn sync_directory(path: impl AsRef<Path>) -> std::io::Result<()> {
    durablefs::sync_directory(path)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err.to_string()))
}

pub(super) fn copy_file(src: &str, dst: &str, mode: u32) -> std::io::Result<()> {
    let dst_path = Path::new(dst);
    let parent = dst_path.parent().unwrap_or_else(|| Path::new("."));
    create_dir_all_private(parent).map_err(|err| prefix_io("create dst dir", err))?;
    let mut from = File::open(src).map_err(|err| prefix_io("open src file", err))?;
    let mut to =
        open_destination_file(dst_path, mode).map_err(|err| prefix_io("open dst file", err))?;
    std::io::copy(&mut from, &mut to).map_err(|err| prefix_io("copy file", err))?;
    to.sync_all()
        .map_err(|err| prefix_io("sync dst file", err))?;
    Ok(())
}

pub(super) fn copy_tree(src: &str, dst: &str) -> std::io::Result<()> {
    if !dir_exists(src) {
        return Ok(());
    }
    create_dir_all_private(Path::new(dst)).map_err(|err| prefix_io("create dst tree", err))?;
    copy_tree_inner(Path::new(src), Path::new(src), Path::new(dst))
        .map_err(|err| prefix_io("copy tree", err))?;
    let parent = Path::new(dst).parent().unwrap_or_else(|| Path::new("."));
    sync_directory(parent)
}

pub(super) fn write_json_file<T: Serialize>(path: &str, payload: &T) -> std::io::Result<()> {
    let raw = serde_json::to_vec_pretty(payload).map_err(|err| prefix_io("marshal json", err))?;
    write_atomic_file(path, &raw, 0o600)
}

fn copy_tree_inner(src_root: &Path, current: &Path, dst_root: &Path) -> std::io::Result<()> {
    let mut entries = fs::read_dir(current)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let source = entry.path();
        let metadata = fs::symlink_metadata(&source)?;
        let relative = source.strip_prefix(src_root).map_err(|err| {
            std::io::Error::new(std::io::ErrorKind::Other, format!("rel path: {err}"))
        })?;
        let target = dst_root.join(relative);
        if metadata.is_dir() {
            create_dir_all_private(&target)?;
            copy_tree_inner(src_root, &source, dst_root)?;
        } else {
            let mode = file_mode(&metadata).unwrap_or(0o600);
            copy_file(
                source.to_string_lossy().as_ref(),
                target.to_string_lossy().as_ref(),
                mode,
            )?;
        }
    }
    Ok(())
}

fn create_dir_all_private(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .mode(0o700)
            .recursive(true)
            .create(path)
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(path)
    }
}

fn open_temp_file(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.create_new(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

fn open_destination_file(path: &Path, mode: u32) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    #[cfg(not(unix))]
    {
        let _ = mode;
    }
    options.open(path)
}

fn set_file_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        let _ = mode;
        Ok(())
    }
}

fn file_mode(metadata: &fs::Metadata) -> Option<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Some(metadata.permissions().mode())
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        None
    }
}

fn unique_temp_path(parent: &Path) -> PathBuf {
    for attempt in 0..1000u32 {
        let name = format!(
            ".upgrade-{}-{}-{attempt}.tmp",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let path = parent.join(name);
        if !path.exists() {
            return path;
        }
    }
    parent.join(format!(".upgrade-{}.tmp", std::process::id()))
}

fn prefix_io(label: &str, err: impl std::fmt::Display) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Other, format!("{label}: {err}"))
}

#[cfg(not(windows))]
fn close_file(file: File) -> std::io::Result<()> {
    use std::os::fd::IntoRawFd;
    let fd = file.into_raw_fd();
    let result = unsafe { close(fd) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(windows))]
extern "C" {
    fn close(fd: std::os::raw::c_int) -> std::os::raw::c_int;
}

#[cfg(windows)]
fn close_file(file: File) -> std::io::Result<()> {
    use std::os::windows::io::IntoRawHandle;
    let handle = file.into_raw_handle();
    let result = unsafe { CloseHandle(handle as *mut std::ffi::c_void) };
    if result != 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(windows)]
extern "system" {
    fn CloseHandle(hObject: *mut std::ffi::c_void) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::ser::{Error, SerializeStruct};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn path_file_and_dir_exists_match_go_contract() {
        let temp = TempDir::new("upgrade-fsutil-exists").expect("temp dir");
        let file = temp.path().join("file.txt");
        let dir = temp.path().join("dir");
        fs::write(&file, "content").expect("write file");
        fs::create_dir_all(&dir).expect("create dir");

        assert!(!path_exists(""));
        assert!(!file_exists(""));
        assert!(!dir_exists(""));
        assert!(path_exists(file.to_string_lossy().as_ref()));
        assert!(path_exists(dir.to_string_lossy().as_ref()));
        assert!(file_exists(file.to_string_lossy().as_ref()));
        assert!(!file_exists(dir.to_string_lossy().as_ref()));
        assert!(dir_exists(dir.to_string_lossy().as_ref()));
        assert!(!dir_exists(file.to_string_lossy().as_ref()));
    }

    #[test]
    fn write_atomic_file_matches_go_temp_cleanup_and_permissions() {
        let temp = TempDir::new("upgrade-fsutil-atomic").expect("temp dir");
        let target = temp.path().join("nested").join("state.json");
        write_atomic_file(target.to_string_lossy().as_ref(), b"{\"ok\":true}", 0o600)
            .expect("write atomic file");
        assert_eq!(
            fs::read_to_string(&target).expect("read target"),
            "{\"ok\":true}"
        );
        let leftovers = fs::read_dir(target.parent().expect("target parent"))
            .expect("read target parent")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(".upgrade-"))
            .count();
        assert_eq!(leftovers, 0, "temporary files should be removed");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&target).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn copy_file_and_copy_tree_match_go_contracts() {
        let temp = TempDir::new("upgrade-fsutil-copy").expect("temp dir");
        let source_file = temp.path().join("source.txt");
        let copied_file = temp.path().join("out").join("copied.txt");
        fs::write(&source_file, "hello").expect("write source file");
        copy_file(
            source_file.to_string_lossy().as_ref(),
            copied_file.to_string_lossy().as_ref(),
            0o640,
        )
        .expect("copy file");
        assert_eq!(fs::read_to_string(&copied_file).unwrap(), "hello");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&copied_file).unwrap().permissions().mode() & 0o777,
                0o640
            );
        }

        let missing_source = temp.path().join("missing");
        let missing_target = temp.path().join("missing-target");
        copy_tree(
            missing_source.to_string_lossy().as_ref(),
            missing_target.to_string_lossy().as_ref(),
        )
        .expect("missing tree is a no-op");
        assert!(!missing_target.exists());

        let source_tree = temp.path().join("tree");
        fs::create_dir_all(source_tree.join("a").join("b")).expect("create source tree");
        fs::write(source_tree.join("root.txt"), "root").expect("write root");
        fs::write(source_tree.join("a").join("b").join("leaf.txt"), "leaf").expect("write leaf");
        let target_tree = temp.path().join("tree-copy");
        copy_tree(
            source_tree.to_string_lossy().as_ref(),
            target_tree.to_string_lossy().as_ref(),
        )
        .expect("copy tree");
        assert_eq!(
            fs::read_to_string(target_tree.join("root.txt")).unwrap(),
            "root"
        );
        assert_eq!(
            fs::read_to_string(target_tree.join("a").join("b").join("leaf.txt")).unwrap(),
            "leaf"
        );
    }

    #[test]
    fn write_json_file_matches_go_pretty_json_and_marshal_error_prefix() {
        let temp = TempDir::new("upgrade-fsutil-json").expect("temp dir");
        let target = temp.path().join("payload.json");
        write_json_file(
            target.to_string_lossy().as_ref(),
            &serde_json::json!({"b": 2, "a": 1}),
        )
        .expect("write json file");
        let raw = fs::read_to_string(&target).expect("read json file");
        assert!(raw.contains("\n  \"a\": 1"));
        assert!(!raw.ends_with('\n'));

        let err = write_json_file(target.to_string_lossy().as_ref(), &BrokenSerialize)
            .expect_err("marshal should fail");
        assert!(
            err.to_string().starts_with("marshal json:"),
            "unexpected error: {err}"
        );
    }

    struct BrokenSerialize;

    impl Serialize for BrokenSerialize {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            let mut state = serializer.serialize_struct("BrokenSerialize", 1)?;
            state.serialize_field("broken", &AlwaysFails)?;
            state.end()
        }
    }

    struct AlwaysFails;

    impl Serialize for AlwaysFails {
        fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            Err(S::Error::custom("forced failure"))
        }
    }

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(prefix: &str) -> std::io::Result<Self> {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "awiki-cli-rs2-{prefix}-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir_all(&path)?;
            Ok(Self { path })
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
