use std::io::Write;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

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
        std::fs::rename(temp.path(), destination).map_err(|err| crate::ImError::Io {
            detail: format!(
                "rename temp file {} to {}: {err}",
                temp.path().display(),
                destination.display()
            ),
        })?;
        temp.persist();
    } else {
        match std::fs::hard_link(temp.path(), destination) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(destination_exists_error(destination));
            }
            Err(err) => {
                return Err(crate::ImError::Io {
                    detail: format!(
                        "link temp file {} to {}: {err}",
                        temp.path().display(),
                        destination.display()
                    ),
                });
            }
        }
    }

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
        tokio::fs::rename(temp.path(), destination)
            .await
            .map_err(|err| crate::ImError::Io {
                detail: format!(
                    "rename temp file {} to {}: {err}",
                    temp.path().display(),
                    destination.display()
                ),
            })?;
        temp.persist();
    } else {
        match tokio::fs::hard_link(temp.path(), destination).await {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(destination_exists_error(destination));
            }
            Err(err) => {
                return Err(crate::ImError::Io {
                    detail: format!(
                        "link temp file {} to {}: {err}",
                        temp.path().display(),
                        destination.display()
                    ),
                });
            }
        }
    }

    Ok(destination.to_path_buf())
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
