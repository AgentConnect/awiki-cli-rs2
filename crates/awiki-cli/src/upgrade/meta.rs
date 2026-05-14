use super::types::Meta;
use crate::durablefs;
use std::fmt;
use std::fs;
use std::path::Path;

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
    write_atomic_file(path, &raw, 0o600).map_err(MetaError::Write)
}

pub(super) fn write_atomic_file(path: &str, content: &[u8], mode: u32) -> std::io::Result<()> {
    let path = Path::new(path);
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let temp_path = unique_temp_path(parent);
    let mut cleanup = true;
    let result = (|| {
        fs::write(&temp_path, content)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&temp_path, fs::Permissions::from_mode(mode))?;
        }
        #[cfg(not(unix))]
        {
            let _ = mode;
        }
        let file = fs::OpenOptions::new().read(true).open(&temp_path)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp_path, path)?;
        durablefs::sync_directory(parent)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err.to_string()))?;
        cleanup = false;
        Ok(())
    })();
    if cleanup {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

fn unique_temp_path(parent: &Path) -> std::path::PathBuf {
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
