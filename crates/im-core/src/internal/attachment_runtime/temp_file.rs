use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

use rand::RngCore;

pub(crate) struct SiblingTempFile {
    path: PathBuf,
    cleanup: bool,
}

pub(crate) struct AsyncSiblingTempFile {
    path: PathBuf,
    cleanup: bool,
}

impl SiblingTempFile {
    pub(crate) fn create(destination: &Path) -> crate::ImResult<(Self, File)> {
        let parent = destination
            .parent()
            .filter(|value| !value.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let filename = destination
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("attachment");

        for attempt in 0..32 {
            let path = parent.join(temp_filename(filename, attempt));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => {
                    return Ok((
                        Self {
                            path,
                            cleanup: true,
                        },
                        file,
                    ));
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(err) => {
                    return Err(crate::ImError::Io {
                        detail: format!("create temp file {}: {err}", path.display()),
                    });
                }
            }
        }

        Err(crate::ImError::Io {
            detail: format!(
                "create temp file for {}: exhausted unique filename attempts",
                destination.display()
            ),
        })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn persist(mut self) {
        self.cleanup = false;
    }
}

impl AsyncSiblingTempFile {
    pub(crate) async fn create(destination: &Path) -> crate::ImResult<(Self, tokio::fs::File)> {
        let parent = destination
            .parent()
            .filter(|value| !value.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let filename = destination
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("attachment");

        for attempt in 0..32 {
            let path = parent.join(temp_filename(filename, attempt));
            match tokio::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .await
            {
                Ok(file) => {
                    return Ok((
                        Self {
                            path,
                            cleanup: true,
                        },
                        file,
                    ));
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(err) => {
                    return Err(crate::ImError::Io {
                        detail: format!("create temp file {}: {err}", path.display()),
                    });
                }
            }
        }

        Err(crate::ImError::Io {
            detail: format!(
                "create temp file for {}: exhausted unique filename attempts",
                destination.display()
            ),
        })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn persist(mut self) {
        self.cleanup = false;
    }
}

impl Drop for SiblingTempFile {
    fn drop(&mut self) {
        if self.cleanup {
            let _ = fs::remove_file(&self.path);
        }
    }
}

impl Drop for AsyncSiblingTempFile {
    fn drop(&mut self) {
        if self.cleanup {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn temp_filename(filename: &str, attempt: u32) -> String {
    let mut random = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut random);
    let random = u64::from_le_bytes(random);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    format!(
        ".{filename}.awiki-attachment-download-{}-{nanos}-{attempt}-{random:016x}.tmp",
        std::process::id()
    )
}
