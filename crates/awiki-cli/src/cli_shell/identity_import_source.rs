use crate::cli_output::ExitError;
use std::path::Path;

// Override the migration input for this invocation, never persisted tenant config.
pub(super) fn select_credentials_directory(
    source: &mut String,
    explicit: Option<&str>,
) -> Result<(), ExitError> {
    if let Some(path) = explicit {
        let directory = Path::new(path).canonicalize().ok().filter(|p| p.is_dir());
        let Some(directory) = directory.filter(|_| !path.trim().is_empty()) else {
            return Err(invalid_source());
        };
        *source = directory.to_str().ok_or_else(invalid_source)?.to_owned();
    }
    if source.trim().is_empty() {
        return Err(invalid_source());
    }
    Ok(())
}

fn invalid_source() -> ExitError {
    ExitError::new(
        "invalid_argument",
        2,
        "Legacy credential source is absent or is not a readable directory.",
        "Pass --credentials-dir <directory> to select the legacy identities to import into this tenant.",
    )
}

#[cfg(test)]
#[path = "identity_import_source_tests.rs"]
mod tests;
