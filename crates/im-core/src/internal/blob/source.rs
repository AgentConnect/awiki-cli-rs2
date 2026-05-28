use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BlobSource {
    pub filename: Option<String>,
    pub mime_type: Option<String>,
    pub path: Option<PathBuf>,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AsyncBlobSource {
    Memory {
        filename: Option<String>,
        mime_type: Option<String>,
        bytes: Vec<u8>,
    },
    LocalFile {
        path: PathBuf,
    },
}

pub(crate) fn attachment_input_to_blob_source(
    input: crate::attachments::AttachmentInput,
) -> crate::ImResult<BlobSource> {
    match input {
        crate::attachments::AttachmentInput::LocalFile(path) => blob_source_from_path(path),
        crate::attachments::AttachmentInput::Bytes {
            filename,
            mime_type,
            bytes,
        } => Ok(BlobSource {
            filename,
            mime_type,
            path: None,
            bytes,
        }),
    }
}

pub(crate) fn attachment_input_to_async_blob_source(
    input: crate::attachments::AttachmentInput,
) -> crate::ImResult<AsyncBlobSource> {
    match input {
        crate::attachments::AttachmentInput::LocalFile(path) => {
            if path.as_os_str().is_empty() {
                return Err(crate::ImError::invalid_input(
                    Some("file_path".to_string()),
                    "attachment file path is required",
                ));
            }
            Ok(AsyncBlobSource::LocalFile { path })
        }
        crate::attachments::AttachmentInput::Bytes {
            filename,
            mime_type,
            bytes,
        } => Ok(AsyncBlobSource::Memory {
            filename,
            mime_type,
            bytes,
        }),
    }
}

fn blob_source_from_path(path: PathBuf) -> crate::ImResult<BlobSource> {
    if path.as_os_str().is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("file_path".to_string()),
            "attachment file path is required",
        ));
    }
    let bytes = std::fs::read(Path::new(&path))?;
    Ok(BlobSource {
        filename: None,
        mime_type: None,
        path: Some(path),
        bytes,
    })
}
