use std::path::Path;

#[cfg(windows)]
pub fn sync_directory(_path: impl AsRef<Path>) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(not(windows))]
pub fn sync_directory(path: impl AsRef<Path>) -> anyhow::Result<()> {
    let path = path.as_ref();
    std::fs::File::open(path)
        .map_err(|err| anyhow::anyhow!("open dir: {err}"))?
        .sync_all()
        .map_err(|err| anyhow::anyhow!("sync dir: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn sync_directory_existing_dir_matches_go_contract() {
        let temp = TempDir::new("durablefs-existing").expect("temp dir");
        sync_directory(temp.path()).expect("sync existing dir");
    }

    #[test]
    fn sync_directory_missing_dir_behavior_matches_platform_contract() {
        let temp = TempDir::new("durablefs-missing").expect("temp dir");
        let missing = temp.path().join("missing");
        let err = sync_directory(&missing).err();

        if cfg!(windows) {
            assert!(
                err.is_none(),
                "windows sync_directory is a no-op and should ignore missing dirs"
            );
        } else {
            assert!(
                err.is_some(),
                "non-windows sync_directory should fail for missing dirs"
            );
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
            std::fs::create_dir_all(&path)?;
            Ok(Self { path })
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
}
