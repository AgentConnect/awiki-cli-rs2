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

    let temporary = temporary
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let target = target
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
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

#[cfg(not(any(unix, windows)))]
fn replace_platform(temporary: &Path, target: &Path) -> std::io::Result<()> {
    std::fs::rename(temporary, target)
}

#[cfg(test)]
mod tests;
