use super::meta::write_atomic_file;
use super::types::Journal;
use std::fmt;
use std::fs;

#[derive(Debug)]
pub enum JournalError {
    Read(std::io::Error),
    Parse(serde_json::Error),
    Marshal(serde_json::Error),
    Write(std::io::Error),
    Remove(std::io::Error),
    RequiredPath,
}

impl fmt::Display for JournalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(err) => write!(f, "read workspace upgrade journal: {err}"),
            Self::Parse(err) => write!(f, "parse workspace upgrade journal: {err}"),
            Self::Marshal(err) => write!(f, "marshal workspace upgrade journal: {err}"),
            Self::Write(err) => write!(f, "write workspace upgrade journal: {err}"),
            Self::Remove(err) => write!(f, "remove workspace upgrade journal: {err}"),
            Self::RequiredPath => f.write_str("workspace journal path is required"),
        }
    }
}

impl std::error::Error for JournalError {}

pub fn load_journal(path: &str) -> Result<Option<Journal>, JournalError> {
    if path.trim().is_empty() {
        return Ok(None);
    }
    let raw = match fs::read(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(JournalError::Read(err)),
    };
    serde_json::from_slice(&raw)
        .map(Some)
        .map_err(JournalError::Parse)
}

pub fn save_journal(path: &str, journal: &Journal) -> Result<(), JournalError> {
    if path.trim().is_empty() {
        return Err(JournalError::RequiredPath);
    }
    let raw = serde_json::to_vec_pretty(journal).map_err(JournalError::Marshal)?;
    write_atomic_file(path, &raw, 0o600).map_err(JournalError::Write)
}

pub fn clear_journal(path: &str) -> Result<(), JournalError> {
    if path.trim().is_empty() {
        return Ok(());
    }
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(JournalError::Remove(err)),
    }
}
