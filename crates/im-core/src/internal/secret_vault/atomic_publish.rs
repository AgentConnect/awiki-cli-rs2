use std::io;
use std::path::Path;

/// Atomically makes `destination` visible only when it does not already exist.
///
/// `true` means this caller published the file. `false` means another complete
/// destination already won. Callers must clean up `source` after either result:
/// native rename implementations consume it on success, while failed or
/// already-existing publishes leave it available for cleanup.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub(super) fn publish_noreplace(source: &Path, destination: &Path) -> io::Result<bool> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let source = CString::new(source.as_os_str().as_bytes()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "atomic publish source path contains a NUL byte",
        )
    })?;
    let destination = CString::new(destination.as_os_str().as_bytes()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "atomic publish destination path contains a NUL byte",
        )
    })?;

    // Use the syscall directly so Android support does not depend on the
    // device's bionic version exporting a renameat2 symbol.
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            destination.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        return Ok(true);
    }

    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::EEXIST) {
        return Ok(false);
    }
    Err(error)
}

#[cfg(target_vendor = "apple")]
pub(super) fn publish_noreplace(source: &Path, destination: &Path) -> io::Result<bool> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let source = CString::new(source.as_os_str().as_bytes()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "atomic publish source path contains a NUL byte",
        )
    })?;
    let destination = CString::new(destination.as_os_str().as_bytes()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "atomic publish destination path contains a NUL byte",
        )
    })?;

    let result =
        unsafe { libc::renamex_np(source.as_ptr(), destination.as_ptr(), libc::RENAME_EXCL) };
    if result == 0 {
        Ok(true)
    } else {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::EEXIST) {
            Ok(false)
        } else {
            Err(error)
        }
    }
}

#[cfg(windows)]
pub(super) fn publish_noreplace(source: &Path, destination: &Path) -> io::Result<bool> {
    use std::iter;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS};
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect::<Vec<_>>();

    let result = unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), 0) };
    if result != 0 {
        Ok(true)
    } else {
        let error = io::Error::last_os_error();
        if error.raw_os_error().is_some_and(|code| {
            code as u32 == ERROR_ALREADY_EXISTS || code as u32 == ERROR_FILE_EXISTS
        }) {
            Ok(false)
        } else {
            Err(error)
        }
    }
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_vendor = "apple",
    windows
)))]
pub(super) fn publish_noreplace(source: &Path, destination: &Path) -> io::Result<bool> {
    publish_with_hard_link_paths(source, destination)
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_vendor = "apple",
    windows
)))]
fn publish_with_hard_link_paths(source: &Path, destination: &Path) -> io::Result<bool> {
    match std::fs::hard_link(source, destination) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
#[path = "atomic_publish_tests.rs"]
mod tests;
