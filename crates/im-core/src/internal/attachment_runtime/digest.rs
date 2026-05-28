use std::path::Path;

use base64::Engine;
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PreparedAttachmentPayload {
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: u64,
    pub size_string: String,
    pub digest_b64u: String,
    pub payload: Vec<u8>,
}

pub(crate) fn prepare_attachment_payload_from_path(
    path: &Path,
    mime_override: &str,
    payload: Vec<u8>,
) -> crate::ImResult<PreparedAttachmentPayload> {
    let filename = filename_from_path(path)?;
    prepare_attachment_payload(&filename, mime_override, payload)
}

pub(crate) fn prepare_attachment_payload(
    filename: &str,
    mime_override: &str,
    payload: Vec<u8>,
) -> crate::ImResult<PreparedAttachmentPayload> {
    let filename = filename.trim();
    if filename.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("filename".to_string()),
            "attachment filename is required",
        ));
    }
    let mime_type = if mime_override.trim().is_empty() {
        detect_attachment_mime_type(filename, &payload)
    } else {
        mime_override.trim().to_string()
    };
    let size_bytes = payload.len() as u64;
    Ok(PreparedAttachmentPayload {
        filename: filename.to_string(),
        mime_type,
        size_bytes,
        size_string: size_bytes.to_string(),
        digest_b64u: sha256_digest_b64u(&payload),
        payload,
    })
}

pub(crate) async fn prepare_attachment_metadata_from_path(
    path: &Path,
    mime_override: &str,
) -> crate::ImResult<PreparedAttachmentPayload> {
    let filename = filename_from_path(path)?;
    let mime_type = if mime_override.trim().is_empty() {
        detect_attachment_mime_type_from_path(&filename, path).await?
    } else {
        mime_override.trim().to_string()
    };
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|err| crate::ImError::Io {
            detail: format!("read attachment metadata {}: {err}", path.display()),
        })?;
    if metadata.is_dir() {
        return Err(crate::ImError::invalid_input(
            Some("file_path".to_string()),
            format!("attachment file path is a directory: {}", path.display()),
        ));
    }
    let digest_b64u = sha256_digest_file_b64u(path).await?;
    let size_bytes = metadata.len();
    Ok(PreparedAttachmentPayload {
        filename,
        mime_type,
        size_bytes,
        size_string: size_bytes.to_string(),
        digest_b64u,
        payload: Vec::new(),
    })
}

pub(crate) fn filename_from_path(path: &Path) -> crate::ImResult<String> {
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if filename.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("file_path".to_string()),
            format!("attachment filename could not be derived from {path:?}"),
        ));
    }
    Ok(filename.to_string())
}

pub(crate) fn sha256_digest_b64u(payload: &[u8]) -> String {
    let digest = Sha256::digest(payload);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

pub(crate) async fn sha256_digest_file_b64u(path: &Path) -> crate::ImResult<String> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|err| crate::ImError::Io {
            detail: format!("open attachment file {}: {err}", path.display()),
        })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let count = file
            .read(&mut buffer)
            .await
            .map_err(|err| crate::ImError::Io {
                detail: format!("read attachment file {}: {err}", path.display()),
            })?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hasher.finalize()))
}

pub(crate) fn detect_attachment_mime_type(filename: &str, payload: &[u8]) -> String {
    let ext = Path::new(filename)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if let Some(mime) = mime_type_by_extension(&ext) {
        return mime.to_string();
    }
    detect_attachment_content_type(payload)
}

async fn detect_attachment_mime_type_from_path(
    filename: &str,
    path: &Path,
) -> crate::ImResult<String> {
    let ext = Path::new(filename)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if let Some(mime) = mime_type_by_extension(&ext) {
        return Ok(mime.to_string());
    }
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|err| crate::ImError::Io {
            detail: format!("open attachment file {}: {err}", path.display()),
        })?;
    let mut sample = vec![0_u8; 512];
    let count = file
        .read(&mut sample)
        .await
        .map_err(|err| crate::ImError::Io {
            detail: format!("read attachment file {}: {err}", path.display()),
        })?;
    sample.truncate(count);
    Ok(detect_attachment_content_type(&sample))
}

fn mime_type_by_extension(ext: &str) -> Option<&'static str> {
    match ext {
        "txt" | "text" | "log" | "md" | "csv" => Some("text/plain; charset=utf-8"),
        "json" => Some("application/json"),
        "html" | "htm" => Some("text/html; charset=utf-8"),
        "css" => Some("text/css; charset=utf-8"),
        "js" | "mjs" => Some("text/javascript; charset=utf-8"),
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "pdf" => Some("application/pdf"),
        "xml" => Some("text/xml; charset=utf-8"),
        "zip" => Some("application/zip"),
        "gz" => Some("application/gzip"),
        "tar" => Some("application/x-tar"),
        "wasm" => Some("application/wasm"),
        _ => None,
    }
}

fn detect_attachment_content_type(payload: &[u8]) -> String {
    if payload.is_empty() {
        return "application/octet-stream".to_string();
    }
    let sample = &payload[..payload.len().min(512)];
    if sample.starts_with(b"\x89PNG\r\n\x1a\n") {
        return "image/png".to_string();
    }
    if sample.starts_with(b"\xff\xd8\xff") {
        return "image/jpeg".to_string();
    }
    if sample.starts_with(b"GIF87a") || sample.starts_with(b"GIF89a") {
        return "image/gif".to_string();
    }
    if sample.starts_with(b"%PDF-") {
        return "application/pdf".to_string();
    }
    if sample.starts_with(b"PK\x03\x04") {
        return "application/zip".to_string();
    }
    if sample
        .iter()
        .all(|byte| matches!(*byte, b'\t' | b'\n' | b'\r' | 0x20..=0x7e))
    {
        return "text/plain; charset=utf-8".to_string();
    }
    "application/octet-stream".to_string()
}
