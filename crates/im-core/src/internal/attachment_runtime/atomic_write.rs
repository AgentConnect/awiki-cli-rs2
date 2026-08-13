use std::io::Write;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

pub(crate) const RESUMABLE_PARTIAL_SUFFIX: &str = ".awiki-part";

pub(crate) fn validate_destination(destination: &Path, overwrite: bool) -> crate::ImResult<()> {
    if destination.as_os_str().is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("destination".to_string()),
            "destination path must not be empty",
        ));
    }

    match std::fs::metadata(destination) {
        Ok(metadata) if metadata.is_dir() => {
            return Err(crate::ImError::invalid_input(
                Some("destination".to_string()),
                format!("destination path is a directory: {}", destination.display()),
            ));
        }
        Ok(_) if !overwrite => return Err(destination_exists_error(destination)),
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(crate::ImError::Io {
                detail: format!("check destination {}: {err}", destination.display()),
            });
        }
    }

    Ok(())
}

pub(crate) fn write_bytes_atomic(
    destination: &Path,
    body: &[u8],
    overwrite: bool,
) -> crate::ImResult<PathBuf> {
    validate_destination(destination, overwrite)?;

    let (temp, mut file) =
        crate::internal::attachment_runtime::temp_file::SiblingTempFile::create(destination)?;
    file.write_all(body).map_err(|err| crate::ImError::Io {
        detail: format!("write temp file {}: {err}", temp.path().display()),
    })?;
    file.sync_all().map_err(|err| crate::ImError::Io {
        detail: format!("sync temp file {}: {err}", temp.path().display()),
    })?;
    drop(file);

    if overwrite {
        crate::internal::atomic_file::replace(temp.path(), destination)?;
        temp.persist();
    } else if !crate::internal::atomic_file::publish_if_absent(temp.path(), destination)? {
        return Err(destination_exists_error(destination));
    }
    sync_parent_directory(destination)?;

    Ok(destination.to_path_buf())
}

pub(crate) async fn write_stream_atomic(
    destination: &Path,
    mut response: crate::internal::transport::AsyncAttachmentObjectResponse,
    overwrite: bool,
) -> crate::ImResult<PathBuf> {
    validate_destination(destination, overwrite)?;

    let (temp, file) =
        crate::internal::attachment_runtime::temp_file::AsyncSiblingTempFile::create(destination)
            .await?;
    let mut file = file;
    while let Some(chunk) = response.next_chunk().await? {
        file.write_all(&chunk)
            .await
            .map_err(|err| crate::ImError::Io {
                detail: format!("write temp file {}: {err}", temp.path().display()),
            })?;
    }
    file.sync_all().await.map_err(|err| crate::ImError::Io {
        detail: format!("sync temp file {}: {err}", temp.path().display()),
    })?;
    drop(file);

    if overwrite {
        crate::internal::atomic_file::replace(temp.path(), destination)?;
        temp.persist();
    } else if !crate::internal::atomic_file::publish_if_absent(temp.path(), destination)? {
        return Err(destination_exists_error(destination));
    }
    sync_parent_directory(destination)?;

    Ok(destination.to_path_buf())
}

pub(crate) fn resumable_partial_path(destination: &Path) -> PathBuf {
    let mut value = destination.as_os_str().to_os_string();
    value.push(RESUMABLE_PARTIAL_SUFFIX);
    PathBuf::from(value)
}

pub(crate) async fn prepare_resumable_partial(
    destination: &Path,
    overwrite: bool,
    expected_size: Option<u64>,
) -> crate::ImResult<(PathBuf, u64)> {
    validate_destination(destination, overwrite)?;
    let partial = resumable_partial_path(destination);
    let mut size = match tokio::fs::metadata(&partial).await {
        Ok(metadata) if metadata.is_file() => metadata.len(),
        Ok(_) => {
            return Err(crate::ImError::Io {
                detail: format!(
                    "attachment partial path is not a file: {}",
                    partial.display()
                ),
            });
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => 0,
        Err(err) => {
            return Err(crate::ImError::Io {
                detail: format!("inspect attachment partial {}: {err}", partial.display()),
            });
        }
    };
    if expected_size.is_some_and(|expected| size > expected) {
        tokio::fs::remove_file(&partial)
            .await
            .map_err(|err| crate::ImError::Io {
                detail: format!(
                    "remove oversized attachment partial {}: {err}",
                    partial.display()
                ),
            })?;
        size = 0;
    }
    Ok((partial, size))
}

pub(crate) async fn reset_resumable_partial(path: &Path) -> crate::ImResult<()> {
    let file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .await
        .map_err(|err| crate::ImError::Io {
            detail: format!("reset attachment partial {}: {err}", path.display()),
        })?;
    file.sync_all().await.map_err(|err| crate::ImError::Io {
        detail: format!("sync reset attachment partial {}: {err}", path.display()),
    })
}

pub(crate) async fn append_resumable_stream(
    path: &Path,
    mut response: crate::internal::transport::AsyncAttachmentObjectResponse,
    initial_size: u64,
    expected_size: Option<u64>,
    idle_timeout: std::time::Duration,
    cancellation: &tokio_util::sync::CancellationToken,
) -> crate::ImResult<u64> {
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await
        .map_err(|err| crate::ImError::Io {
            detail: format!("open attachment partial {}: {err}", path.display()),
        })?;
    let mut received = initial_size;
    loop {
        let next_chunk = response.next_chunk_with_idle_timeout(idle_timeout);
        let chunk = match tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err(cancelled_transfer(received, expected_size)),
            result = next_chunk => result,
        } {
            Ok(chunk) => chunk,
            Err(error) => {
                let _ = file.flush().await;
                return Err(with_transfer_progress(error, received, expected_size));
            }
        };
        let Some(chunk) = chunk else {
            break;
        };
        received = received.saturating_add(chunk.len() as u64);
        if expected_size.is_some_and(|expected| received > expected) {
            let _ = file.flush().await;
            return Err(crate::ImError::AttachmentTransfer {
                failure: crate::AttachmentTransferFailure::Incomplete,
                received_bytes: received,
                expected_bytes: expected_size,
                retryable: false,
                detail: "attachment response exceeded the declared object size".to_owned(),
            });
        }
        file.write_all(&chunk)
            .await
            .map_err(|err| crate::ImError::Io {
                detail: format!("write attachment partial {}: {err}", path.display()),
            })?;
    }
    file.sync_all().await.map_err(|err| crate::ImError::Io {
        detail: format!("sync attachment partial {}: {err}", path.display()),
    })?;
    Ok(received)
}

pub(crate) async fn commit_resumable_partial(
    partial: &Path,
    destination: &Path,
    overwrite: bool,
) -> crate::ImResult<PathBuf> {
    validate_destination(destination, overwrite)?;
    if overwrite {
        crate::internal::atomic_file::replace(partial, destination)?;
    } else if crate::internal::atomic_file::publish_if_absent(partial, destination)? {
        match tokio::fs::remove_file(partial).await {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(crate::ImError::Io {
                    detail: format!(
                        "remove committed attachment partial {}: {err}",
                        partial.display()
                    ),
                });
            }
        }
    } else {
        return Err(destination_exists_error(destination));
    }
    sync_parent_directory(destination)?;
    Ok(destination.to_path_buf())
}

#[cfg(unix)]
fn sync_parent_directory(destination: &Path) -> crate::ImResult<()> {
    let Some(parent) = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    else {
        return Ok(());
    };
    std::fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|err| crate::ImError::Io {
            detail: format!(
                "sync attachment destination directory {}: {err}",
                parent.display()
            ),
        })
}

#[cfg(not(unix))]
fn sync_parent_directory(_destination: &Path) -> crate::ImResult<()> {
    Ok(())
}

fn cancelled_transfer(received_bytes: u64, expected_bytes: Option<u64>) -> crate::ImError {
    crate::ImError::AttachmentTransfer {
        failure: crate::AttachmentTransferFailure::Cancelled,
        received_bytes,
        expected_bytes,
        retryable: false,
        detail: "attachment download was cancelled; partial bytes were retained".to_owned(),
    }
}

fn with_transfer_progress(
    error: crate::ImError,
    received_bytes: u64,
    expected_bytes: Option<u64>,
) -> crate::ImError {
    match error {
        crate::ImError::AttachmentTransfer {
            failure,
            retryable,
            detail,
            ..
        } => crate::ImError::AttachmentTransfer {
            failure,
            received_bytes,
            expected_bytes,
            retryable,
            detail,
        },
        other => other,
    }
}

fn destination_exists_error(destination: &Path) -> crate::ImError {
    crate::ImError::invalid_input(
        Some("destination".to_string()),
        format!(
            "destination already exists and overwrite is false: {}",
            destination.display()
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn atomic_write_creates_destination_without_overwrite() {
        let root = unique_temp_root("atomic-create");
        fs::create_dir_all(&root).unwrap();
        let destination = root.join("download.bin");

        let path = write_bytes_atomic(&destination, b"downloaded", false).unwrap();

        assert_eq!(path, destination);
        assert_eq!(fs::read(&destination).unwrap(), b"downloaded");
        assert_no_temp_files(&root);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn atomic_write_rejects_existing_destination_without_overwrite() {
        let root = unique_temp_root("atomic-no-overwrite");
        fs::create_dir_all(&root).unwrap();
        let destination = root.join("download.bin");
        fs::write(&destination, b"existing").unwrap();

        let err = write_bytes_atomic(&destination, b"new", false).unwrap_err();

        assert_eq!(fs::read(&destination).unwrap(), b"existing");
        assert!(matches!(
            err,
            crate::ImError::InvalidInput { field: Some(field), message }
                if field == "destination" && message.contains("overwrite is false")
        ));
        assert_no_temp_files(&root);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn atomic_write_replaces_destination_with_overwrite() {
        let root = unique_temp_root("atomic-overwrite");
        fs::create_dir_all(&root).unwrap();
        let destination = root.join("download.bin");
        fs::write(&destination, b"existing").unwrap();

        write_bytes_atomic(&destination, b"new", true).unwrap();

        assert_eq!(fs::read(&destination).unwrap(), b"new");
        assert_no_temp_files(&root);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn atomic_write_cleans_temp_when_rename_fails() {
        let root = unique_temp_root("atomic-cleanup");
        let destination = root.join("download.bin");
        fs::create_dir_all(&destination).unwrap();

        let err = write_bytes_atomic(&destination, b"new", true).unwrap_err();

        assert!(matches!(
            err,
            crate::ImError::InvalidInput { field: Some(field), message }
                if field == "destination" && message.contains("directory")
        ));
        assert_no_temp_files(&root);
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn cancelled_resumable_append_keeps_existing_partial() {
        let root = unique_temp_root("resumable-cancel");
        fs::create_dir_all(&root).unwrap();
        let partial = root.join("download.bin.awiki-part");
        fs::write(&partial, b"existing").unwrap();
        let cancellation = tokio_util::sync::CancellationToken::new();
        cancellation.cancel();

        let error = append_resumable_stream(
            &partial,
            crate::internal::transport::AsyncAttachmentObjectResponse::Bytes {
                body: b"new bytes".to_vec(),
                content_type: None,
                consumed: false,
            },
            8,
            Some(17),
            std::time::Duration::from_secs(1),
            &cancellation,
        )
        .await
        .expect_err("cancelled transfer must stop");

        assert!(matches!(
            error,
            crate::ImError::AttachmentTransfer {
                failure: crate::AttachmentTransferFailure::Cancelled,
                received_bytes: 8,
                expected_bytes: Some(17),
                retryable: false,
                ..
            }
        ));
        assert_eq!(fs::read(&partial).unwrap(), b"existing");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn resumable_commit_atomically_replaces_existing_destination() {
        let root = unique_temp_root("resumable-commit-overwrite");
        fs::create_dir_all(&root).unwrap();
        let destination = root.join("download.bin");
        let partial = resumable_partial_path(&destination);
        fs::write(&destination, b"existing").unwrap();
        fs::write(&partial, b"complete").unwrap();

        let committed = commit_resumable_partial(&partial, &destination, true)
            .await
            .unwrap();

        assert_eq!(committed, destination);
        assert_eq!(fs::read(&destination).unwrap(), b"complete");
        assert!(!partial.exists());
        let _ = fs::remove_dir_all(root);
    }

    fn assert_no_temp_files(root: &std::path::Path) {
        let leftovers: Vec<_> = fs::read_dir(root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".awiki-attachment-download-"))
            .collect();
        assert_eq!(leftovers, Vec::<String>::new());
    }

    fn unique_temp_root(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("im-core-{name}-{}-{nanos}", std::process::id()))
    }
}
