use super::types::LockMetadata;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use time::format_description::well_known::Rfc3339;
use time::{Date, Duration, Month, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset};

const OS_FILE_LOCK_SCHEME: &str = "os_file_lock_v1";
const LEGACY_LOCK_MAX_AGE: Duration = Duration::hours(24);

#[derive(Debug)]
pub enum LockError {
    RequiredPath,
    CreateDir(std::io::Error),
    Open(std::io::Error),
    Locked {
        path: String,
    },
    AcquireOs(std::io::Error),
    Seek(std::io::Error),
    Read(std::io::Error),
    Rewind(std::io::Error),
    Marshal(serde_json::Error),
    Truncate(std::io::Error),
    Write(std::io::Error),
    Sync(std::io::Error),
    ReleaseOs(std::io::Error),
    Close(std::io::Error),
    ReleaseAndClose {
        release: std::io::Error,
        close: std::io::Error,
    },
}

impl LockError {
    pub fn is_locked(&self) -> bool {
        matches!(self, Self::Locked { .. })
    }
}

impl fmt::Display for LockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RequiredPath => f.write_str("workspace lock path is required"),
            Self::CreateDir(err) => write!(f, "create upgrade lock dir: {err}"),
            Self::Open(err) => write!(f, "open upgrade lock: {err}"),
            Self::Locked { path } => write!(f, "workspace upgrade is already running: {path}"),
            Self::AcquireOs(err) => write!(f, "acquire OS upgrade lock: {err}"),
            Self::Seek(err) => write!(f, "seek upgrade lock: {err}"),
            Self::Read(err) => write!(f, "read upgrade lock: {err}"),
            Self::Rewind(err) => write!(f, "rewind upgrade lock: {err}"),
            Self::Marshal(err) => write!(f, "marshal upgrade lock: {err}"),
            Self::Truncate(err) => write!(f, "truncate upgrade lock: {err}"),
            Self::Write(err) => write!(f, "write upgrade lock: {err}"),
            Self::Sync(err) => write!(f, "sync upgrade lock: {err}"),
            Self::ReleaseOs(err) => write!(f, "release OS upgrade lock: {err}"),
            Self::Close(err) => write!(f, "close upgrade lock: {err}"),
            Self::ReleaseAndClose { release, close } => write!(
                f,
                "release OS upgrade lock: {release}; close upgrade lock: {close}"
            ),
        }
    }
}

impl std::error::Error for LockError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CreateDir(err)
            | Self::Open(err)
            | Self::AcquireOs(err)
            | Self::Seek(err)
            | Self::Read(err)
            | Self::Rewind(err)
            | Self::Truncate(err)
            | Self::Write(err)
            | Self::Sync(err)
            | Self::ReleaseOs(err)
            | Self::Close(err) => Some(err),
            Self::Marshal(err) => Some(err),
            Self::RequiredPath | Self::Locked { .. } | Self::ReleaseAndClose { .. } => None,
        }
    }
}

#[derive(Debug)]
pub struct UpgradeLockGuard {
    file: Option<File>,
}

impl UpgradeLockGuard {
    pub fn release(mut self) -> Result<(), LockError> {
        self.release_inner()
    }

    fn release_inner(&mut self) -> Result<(), LockError> {
        if let Some(file) = self.file.take() {
            release_and_close_file(file)
        } else {
            Ok(())
        }
    }
}

impl Drop for UpgradeLockGuard {
    fn drop(&mut self) {
        let _ = self.release_inner();
    }
}

pub fn acquire_file_lock(path: &str, app_version: &str) -> Result<UpgradeLockGuard, LockError> {
    if path.is_empty() {
        return Err(LockError::RequiredPath);
    }
    let lock_path = Path::new(path);
    let parent = lock_path
        .parent()
        .filter(|value| !value.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    create_lock_dir(parent).map_err(LockError::CreateDir)?;

    let mut file = open_lock_file(lock_path).map_err(LockError::Open)?;
    match acquire_os_file_lock(&file) {
        Ok(()) => {}
        Err(OSLockError::Locked) => {
            return Err(LockError::Locked {
                path: path.to_string(),
            });
        }
        Err(OSLockError::Io(err)) => return Err(LockError::AcquireOs(err)),
    }

    let cleanup_locked = |file: File| {
        let _ = release_os_file_lock(&file);
        drop(file);
    };

    let existing = match read_lock_metadata(&mut file) {
        Ok(existing) => existing,
        Err(err) => {
            cleanup_locked(file);
            return Err(err);
        }
    };
    if existing
        .as_ref()
        .map(|metadata| is_active_legacy_lock(metadata))
        .unwrap_or(false)
    {
        cleanup_locked(file);
        return Err(LockError::Locked {
            path: path.to_string(),
        });
    }

    let metadata = new_lock_metadata(app_version);
    if let Err(err) = write_lock_metadata(&mut file, &metadata) {
        cleanup_locked(file);
        return Err(err);
    }
    Ok(UpgradeLockGuard { file: Some(file) })
}

fn create_lock_dir(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .mode(0o700)
            .recursive(true)
            .create(path)
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(path)
    }
}

fn open_lock_file(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

fn new_lock_metadata(app_version: &str) -> LockMetadata {
    LockMetadata {
        lock_scheme: OS_FILE_LOCK_SCHEME.to_string(),
        pid: std::process::id() as i64,
        app_version: app_version.to_string(),
        started_at: format_lock_time(OffsetDateTime::now_utc()),
        hostname: hostname(),
        executable: std::env::current_exe()
            .ok()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default(),
    }
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            fs::read_to_string("/etc/hostname")
                .unwrap_or_default()
                .trim()
                .to_string()
        })
}

fn read_lock_metadata(file: &mut File) -> Result<Option<LockMetadata>, LockError> {
    file.seek(SeekFrom::Start(0)).map_err(LockError::Seek)?;
    let mut raw = Vec::new();
    file.read_to_end(&mut raw).map_err(LockError::Read)?;
    file.seek(SeekFrom::Start(0)).map_err(LockError::Rewind)?;
    if String::from_utf8_lossy(&raw).trim().is_empty() {
        return Ok(None);
    }
    match serde_json::from_slice(&raw) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(_) => Ok(None),
    }
}

fn write_lock_metadata(file: &mut File, metadata: &LockMetadata) -> Result<(), LockError> {
    let mut raw = serde_json::to_vec_pretty(metadata).map_err(LockError::Marshal)?;
    raw.push(b'\n');
    file.set_len(0).map_err(LockError::Truncate)?;
    file.seek(SeekFrom::Start(0)).map_err(LockError::Rewind)?;
    file.write_all(&raw).map_err(LockError::Write)?;
    file.sync_all().map_err(LockError::Sync)
}

fn is_active_legacy_lock(metadata: &LockMetadata) -> bool {
    if metadata.lock_scheme == OS_FILE_LOCK_SCHEME {
        return false;
    }
    if metadata.pid <= 0 {
        return false;
    }
    if !process_alive(metadata.pid) {
        return false;
    }
    let Some(started_at) = parse_lock_started_at(&metadata.started_at) else {
        return false;
    };
    OffsetDateTime::now_utc() - started_at <= LEGACY_LOCK_MAX_AGE
}

fn parse_lock_started_at(value: &str) -> Option<OffsetDateTime> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    parse_compact_lock_time(value)
        .or_else(|| OffsetDateTime::parse(value, &Rfc3339).ok())
        .map(|value| value.to_offset(UtcOffset::UTC))
}

fn format_lock_time(value: OffsetDateTime) -> String {
    let value = value.to_offset(UtcOffset::UTC);
    let month: u8 = value.month().into();
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        value.year(),
        month,
        value.day(),
        value.hour(),
        value.minute(),
        value.second()
    )
}

fn parse_compact_lock_time(value: &str) -> Option<OffsetDateTime> {
    let raw = value.as_bytes();
    if raw.len() != 16 || raw[8] != b'T' || raw[15] != b'Z' {
        return None;
    }
    let year = parse_ascii_int::<i32>(&raw[0..4])?;
    let month = parse_ascii_int::<u8>(&raw[4..6])?;
    let day = parse_ascii_int::<u8>(&raw[6..8])?;
    let hour = parse_ascii_int::<u8>(&raw[9..11])?;
    let minute = parse_ascii_int::<u8>(&raw[11..13])?;
    let second = parse_ascii_int::<u8>(&raw[13..15])?;
    let date = Date::from_calendar_date(year, Month::try_from(month).ok()?, day).ok()?;
    let time = Time::from_hms(hour, minute, second).ok()?;
    Some(PrimitiveDateTime::new(date, time).assume_utc())
}

fn parse_ascii_int<T: std::str::FromStr>(raw: &[u8]) -> Option<T> {
    if !raw.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(raw).ok()?.parse().ok()
}

enum OSLockError {
    Locked,
    Io(std::io::Error),
}

#[cfg(not(windows))]
fn acquire_os_file_lock(file: &File) -> Result<(), OSLockError> {
    use std::os::fd::AsRawFd;
    const LOCK_EX: std::os::raw::c_int = 2;
    const LOCK_NB: std::os::raw::c_int = 4;
    let result = unsafe { flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) };
    if result == 0 {
        return Ok(());
    }
    let err = std::io::Error::last_os_error();
    match err.kind() {
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::PermissionDenied => {
            Err(OSLockError::Locked)
        }
        _ => Err(OSLockError::Io(err)),
    }
}

#[cfg(not(windows))]
fn release_os_file_lock(file: &File) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    const LOCK_UN: std::os::raw::c_int = 8;
    let result = unsafe { flock(file.as_raw_fd(), LOCK_UN) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(windows))]
fn process_alive(pid: i64) -> bool {
    if pid <= 0 || pid > i32::MAX as i64 {
        return false;
    }
    let result = unsafe { kill(pid as std::os::raw::c_int, 0) };
    if result == 0 {
        return true;
    }
    std::io::Error::last_os_error().kind() == std::io::ErrorKind::PermissionDenied
}

#[cfg(not(windows))]
extern "C" {
    fn flock(fd: std::os::raw::c_int, operation: std::os::raw::c_int) -> std::os::raw::c_int;
    fn kill(pid: std::os::raw::c_int, sig: std::os::raw::c_int) -> std::os::raw::c_int;
    fn close(fd: std::os::raw::c_int) -> std::os::raw::c_int;
}

#[cfg(not(windows))]
fn release_and_close_file(file: File) -> Result<(), LockError> {
    use std::os::fd::IntoRawFd;
    let unlock_result = release_os_file_lock(&file);
    let fd = file.into_raw_fd();
    let close_result = unsafe { close(fd) };
    let close_result = if close_result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    };
    match (unlock_result, close_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(release), Ok(())) => Err(LockError::ReleaseOs(release)),
        (Ok(()), Err(close)) => Err(LockError::Close(close)),
        (Err(release), Err(close)) => Err(LockError::ReleaseAndClose { release, close }),
    }
}

#[cfg(windows)]
fn acquire_os_file_lock(file: &File) -> Result<(), OSLockError> {
    use std::os::windows::io::AsRawHandle;
    let mut overlapped = Overlapped::default();
    let ok = unsafe {
        LockFileEx(
            file.as_raw_handle() as Handle,
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            0,
            1,
            0,
            &mut overlapped,
        )
    };
    if ok != 0 {
        return Ok(());
    }
    let err = std::io::Error::last_os_error();
    match err.raw_os_error() {
        Some(ERROR_LOCK_VIOLATION) | Some(ERROR_SHARING_VIOLATION) => Err(OSLockError::Locked),
        _ => Err(OSLockError::Io(err)),
    }
}

#[cfg(windows)]
fn release_os_file_lock(file: &File) -> std::io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    let mut overlapped = Overlapped::default();
    let ok = unsafe { UnlockFileEx(file.as_raw_handle() as Handle, 0, 1, 0, &mut overlapped) };
    if ok != 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(windows)]
fn process_alive(pid: i64) -> bool {
    if pid <= 0 || pid > u32::MAX as i64 {
        return false;
    }
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid as u32) };
    if handle.is_null() {
        return false;
    }
    unsafe {
        CloseHandle(handle);
    }
    true
}

#[cfg(windows)]
fn release_and_close_file(file: File) -> Result<(), LockError> {
    use std::os::windows::io::IntoRawHandle;
    let unlock_result = release_os_file_lock(&file);
    let handle = file.into_raw_handle() as Handle;
    let close_result = unsafe { CloseHandle(handle) };
    let close_result = if close_result != 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    };
    match (unlock_result, close_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(release), Ok(())) => Err(LockError::ReleaseOs(release)),
        (Ok(()), Err(close)) => Err(LockError::Close(close)),
        (Err(release), Err(close)) => Err(LockError::ReleaseAndClose { release, close }),
    }
}

#[cfg(windows)]
type Handle = *mut std::ffi::c_void;

#[cfg(windows)]
const LOCKFILE_FAIL_IMMEDIATELY: u32 = 0x0000_0001;
#[cfg(windows)]
const LOCKFILE_EXCLUSIVE_LOCK: u32 = 0x0000_0002;
#[cfg(windows)]
const ERROR_SHARING_VIOLATION: i32 = 32;
#[cfg(windows)]
const ERROR_LOCK_VIOLATION: i32 = 33;
#[cfg(windows)]
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct Overlapped {
    internal: usize,
    internal_high: usize,
    offset: u32,
    offset_high: u32,
    h_event: Handle,
}

#[cfg(windows)]
extern "system" {
    fn LockFileEx(
        hFile: Handle,
        dwFlags: u32,
        dwReserved: u32,
        nNumberOfBytesToLockLow: u32,
        nNumberOfBytesToLockHigh: u32,
        lpOverlapped: *mut Overlapped,
    ) -> i32;
    fn UnlockFileEx(
        hFile: Handle,
        dwReserved: u32,
        nNumberOfBytesToUnlockLow: u32,
        nNumberOfBytesToUnlockHigh: u32,
        lpOverlapped: *mut Overlapped,
    ) -> i32;
    fn OpenProcess(dwDesiredAccess: u32, bInheritHandle: i32, dwProcessId: u32) -> Handle;
    fn CloseHandle(hObject: Handle) -> i32;
}
