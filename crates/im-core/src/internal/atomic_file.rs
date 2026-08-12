//! Cross-platform atomic replacement for security-sensitive local metadata.

use std::path::Path;

/// Atomically replaces `target` with the fully-written file at `temporary`.
///
/// The two paths must be on the same filesystem. Callers remain responsible
/// for syncing the temporary file before replacement and the parent directory
/// afterwards.
pub(crate) fn replace(temporary: &Path, target: &Path) -> crate::ImResult<()> {
    replace_platform(temporary, target).map_err(|error| crate::ImError::Io {
        detail: format!(
            "atomically replace {} with {}: {error}",
            target.display(),
            temporary.display()
        ),
    })
}

#[cfg(unix)]
fn replace_platform(temporary: &Path, target: &Path) -> std::io::Result<()> {
    std::fs::rename(temporary, target)
}

#[cfg(windows)]
fn replace_platform(temporary: &Path, target: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt as _;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    // Always opt in explicitly instead of depending on the host-wide
    // LongPathsEnabled policy. Attachment staging paths can exceed MAX_PATH
    // once a storage scope and canonical group message id are combined.
    let temporary = absolute_windows_path(temporary)?
        .as_os_str()
        .encode_wide()
        .collect::<Vec<_>>();
    let temporary = windows_extended_path_wide(temporary)?;
    let target = absolute_windows_path(target)?
        .as_os_str()
        .encode_wide()
        .collect::<Vec<_>>();
    let target = windows_extended_path_wide(target)?;
    // SAFETY: both pointers refer to live, NUL-terminated UTF-16 buffers for
    // the duration of the call. The flags request replacement without a
    // delete-then-rename gap and durable write-through semantics.
    let moved = unsafe {
        MoveFileExW(
            temporary.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn absolute_windows_path(path: &Path) -> std::io::Result<std::path::PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    std::path::absolute(path)
}

#[cfg(any(windows, test))]
fn windows_extended_path_wide(mut path: Vec<u16>) -> std::io::Result<Vec<u16>> {
    const BACKSLASH: u16 = b'\\' as u16;
    const FORWARD_SLASH: u16 = b'/' as u16;
    const EXTENDED_PREFIX: &[u16] = &[BACKSLASH, BACKSLASH, b'?' as u16, BACKSLASH];
    const DEVICE_PREFIX: &[u16] = &[BACKSLASH, BACKSLASH, b'.' as u16, BACKSLASH];
    const UNC_PREFIX: &[u16] = &[
        BACKSLASH,
        BACKSLASH,
        b'?' as u16,
        BACKSLASH,
        b'U' as u16,
        b'N' as u16,
        b'C' as u16,
        BACKSLASH,
    ];

    if path.contains(&0) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Windows path contains an interior NUL",
        ));
    }
    for unit in &mut path {
        if *unit == FORWARD_SLASH {
            *unit = BACKSLASH;
        }
    }

    let mut extended = if path.starts_with(EXTENDED_PREFIX) || path.starts_with(DEVICE_PREFIX) {
        path
    } else if path.starts_with(&[BACKSLASH, BACKSLASH]) {
        let mut value = UNC_PREFIX.to_vec();
        value.extend_from_slice(&path[2..]);
        value
    } else {
        let mut value = EXTENDED_PREFIX.to_vec();
        value.extend_from_slice(&path);
        value
    };
    extended.push(0);
    Ok(extended)
}

#[cfg(not(any(unix, windows)))]
fn replace_platform(temporary: &Path, target: &Path) -> std::io::Result<()> {
    std::fs::rename(temporary, target)
}

#[cfg(test)]
mod tests;
