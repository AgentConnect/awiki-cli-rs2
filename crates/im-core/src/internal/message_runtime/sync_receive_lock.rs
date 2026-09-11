//! Cross-process reception ownership. Business input processing has separate leases.
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

pub(super) struct ReceiveLock {
    // Closing the file also releases the OS lock on cancellation or process exit.
    _file: fs::File,
}

impl ReceiveLock {
    pub(super) async fn acquire(sqlite_path: &Path, owner: &str) -> crate::ImResult<Self> {
        // Alias paths must coordinate on the same sidecar for the same database.
        let database = sqlite_path.canonicalize()?;
        let mut path = database.into_os_string();
        path.push(format!(
            ".receive-{:x}.lock",
            Sha256::digest(owner.as_bytes())
        ));
        let mut options = fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let file = options.open(path)?;
        loop {
            match fs2::FileExt::try_lock_exclusive(&file) {
                Ok(()) => return Ok(Self { _file: file }),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
}
