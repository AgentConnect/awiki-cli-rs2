//! Cross-platform atomic replacement for security-sensitive local metadata.

use std::io::Write;
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

/// Publishes `temporary` to `target` only when `target` does not exist.
///
/// Returns `true` when this call created `target`. Returns `false` when
/// `target` already existed and was left unchanged. Android SELinux denies
/// `link(2)` on app data, so this path must not depend on hard links.
pub(crate) fn publish_if_absent(temporary: &Path, target: &Path) -> crate::ImResult<bool> {
    match publish_if_absent_platform(temporary, target) {
        Ok(created) => Ok(created),
        Err(error) if is_already_exists(&error) => Ok(false),
        Err(error) if is_noreplace_unsupported(&error) => {
            publish_if_absent_exclusive(temporary, target).map_err(|fallback_error| {
                crate::ImError::Io {
                    detail: format!(
                        "publish {} without replacing {}: {fallback_error}",
                        temporary.display(),
                        target.display()
                    ),
                }
            })
        }
        Err(error) => Err(crate::ImError::Io {
            detail: format!(
                "publish {} without replacing {}: {error}",
                temporary.display(),
                target.display()
            ),
        }),
    }
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

#[cfg(any(target_os = "linux", target_os = "android"))]
fn publish_if_absent_platform(temporary: &Path, target: &Path) -> std::io::Result<bool> {
    let temporary = unix_c_path(temporary)?;
    let target = unix_c_path(target)?;
    // SAFETY: both pointers refer to live, NUL-terminated C strings for the
    // duration of the call. Use the syscall directly so Android support does
    // not depend on the device's bionic version exporting renameat2.
    let renamed = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            temporary.as_ptr(),
            libc::AT_FDCWD,
            target.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if renamed == 0 {
        Ok(true)
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "macos")]
fn publish_if_absent_platform(temporary: &Path, target: &Path) -> std::io::Result<bool> {
    let temporary = unix_c_path(temporary)?;
    let target = unix_c_path(target)?;
    // SAFETY: both pointers refer to live, NUL-terminated C strings for the
    // duration of the call. RENAME_EXCL fails instead of replacing.
    let renamed =
        unsafe { libc::renamex_np(temporary.as_ptr(), target.as_ptr(), libc::RENAME_EXCL) };
    if renamed == 0 {
        Ok(true)
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(windows)]
fn publish_if_absent_platform(temporary: &Path, target: &Path) -> std::io::Result<bool> {
    use std::os::windows::ffi::OsStrExt as _;
    use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};

    let temporary = windows_extended_path_wide(
        absolute_windows_path(temporary)?
            .as_os_str()
            .encode_wide()
            .collect::<Vec<_>>(),
    )?;
    let target = windows_extended_path_wide(
        absolute_windows_path(target)?
            .as_os_str()
            .encode_wide()
            .collect::<Vec<_>>(),
    )?;
    // SAFETY: both pointers refer to live, NUL-terminated UTF-16 buffers.
    // Omitting MOVEFILE_REPLACE_EXISTING keeps an existing target intact.
    let moved = unsafe { MoveFileExW(temporary.as_ptr(), target.as_ptr(), MOVEFILE_WRITE_THROUGH) };
    if moved == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(true)
    }
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    windows
)))]
fn publish_if_absent_platform(temporary: &Path, target: &Path) -> std::io::Result<bool> {
    publish_if_absent_exclusive(temporary, target)
}

#[cfg(unix)]
fn unix_c_path(path: &Path) -> std::io::Result<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt as _;
    std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "path contains an interior NUL",
        )
    })
}

fn publish_if_absent_exclusive(temporary: &Path, target: &Path) -> std::io::Result<bool> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = match options.open(target) {
        Ok(file) => file,
        Err(error) if is_already_exists(&error) => return Ok(false),
        Err(error) => return Err(error),
    };
    let bytes = std::fs::read(temporary)?;
    if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(target);
        return Err(error);
    }
    Ok(true)
}

fn is_already_exists(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::AlreadyExists
}

fn is_noreplace_unsupported(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::Unsupported | std::io::ErrorKind::PermissionDenied
    ) || matches!(
        error.raw_os_error(),
        Some(code) if {
            #[cfg(unix)]
            {
                code == libc::ENOSYS
                    || code == libc::EINVAL
                    || code == libc::ENOTSUP
                    || code == libc::EOPNOTSUPP
                    || code == libc::EPERM
                    || code == libc::EACCES
            }
            #[cfg(not(unix))]
            {
                let _ = code;
                false
            }
        }
    )
}

#[cfg(test)]
mod tests;
