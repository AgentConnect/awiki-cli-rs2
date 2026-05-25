use super::types::Meta;
use std::fmt;
use std::fs;

#[derive(Debug)]
pub enum MetaError {
    Read(std::io::Error),
    Parse(serde_json::Error),
    Marshal(serde_json::Error),
    Write(std::io::Error),
    RequiredPath,
}

impl fmt::Display for MetaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(err) => write!(f, "read workspace meta: {err}"),
            Self::Parse(err) => write!(f, "parse workspace meta: {err}"),
            Self::Marshal(err) => write!(f, "marshal workspace meta: {err}"),
            Self::Write(err) => write!(f, "write workspace meta: {err}"),
            Self::RequiredPath => f.write_str("workspace meta path is required"),
        }
    }
}

impl std::error::Error for MetaError {}

pub fn load_meta(path: &str) -> Result<Option<Meta>, MetaError> {
    if path.trim().is_empty() {
        return Ok(None);
    }
    let raw = match fs::read(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(MetaError::Read(err)),
    };
    serde_json::from_slice(&raw)
        .map(Some)
        .map_err(MetaError::Parse)
}

pub fn save_meta(path: &str, meta: &Meta) -> Result<(), MetaError> {
    if path.trim().is_empty() {
        return Err(MetaError::RequiredPath);
    }
    let raw = serde_json::to_vec_pretty(meta).map_err(MetaError::Marshal)?;
    super::fsutil::write_atomic_file(path, &raw, 0o600).map_err(MetaError::Write)
}
