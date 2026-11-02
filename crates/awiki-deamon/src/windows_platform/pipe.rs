//! Local-only named pipes with OS identity checks, private ACLs and cancellable I/O.
use super::{
    create_private_directory, current_user_sid, process_user_sid, verify_private_handle,
    verify_private_path, wide, OwnedHandle, PrivateDescriptor,
};
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::io::{self, Read, Write};
use std::path::Path;
use std::ptr::{null, null_mut};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Security::Authorization::SE_KERNEL_OBJECT;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::Pipes::*;
use windows_sys::Win32::System::Threading::*;
use windows_sys::Win32::System::IO::*;

fn last_error(context: &'static str) -> anyhow::Error {
    anyhow::Error::from(io::Error::last_os_error()).context(context)
}
fn deadline(timeout: Option<Duration>) -> io::Result<Option<Instant>> {
    match timeout {
        Some(value) if value.is_zero() => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "local RPC timeout must be positive",
        )),
        Some(value) => Instant::now().checked_add(value).map(Some).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "local RPC timeout is too large",
            )
        }),
        None => Ok(None),
    }
}
pub(crate) fn pipe_name(socket_path: &Path) -> Result<String> {
    let parent = socket_path
        .parent()
        .context("Windows local RPC locator has no parent")?;
    verify_private_path(parent)?;
    let parent = parent
        .canonicalize()
        .context("resolve Windows local RPC directory")?;
    let file = socket_path
        .file_name()
        .context("Windows local RPC locator has no name")?;
    let mut digest = Sha256::new();
    digest.update(b"awiki.daemon.windows.local-rpc.v1\0");
    digest.update(current_user_sid()?.as_bytes());
    digest.update([0]);
    digest.update(
        parent
            .join(file)
            .as_os_str()
            .to_string_lossy()
            .to_lowercase()
            .as_bytes(),
    );
    Ok(format!(r"\\.\pipe\awiki-daemon-{:x}", digest.finalize()))
}
fn verify_peer(handle: HANDLE, server_side: bool) -> Result<()> {
    let mut pid = 0;
    let ok = unsafe {
        if server_side {
            GetNamedPipeClientProcessId(handle, &mut pid)
        } else {
            GetNamedPipeServerProcessId(handle, &mut pid)
        }
    };
    if ok == 0 || pid == 0 {
        return Err(last_error("read named pipe peer process identity"));
    }
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process == 0 {
        return Err(last_error("open named pipe peer process"));
    }
    let process = OwnedHandle(process);
    if process_user_sid(process.0)? != current_user_sid()? {
        bail!("Windows local RPC peer does not belong to the daemon user");
    }
    Ok(())
}

struct Operation {
    pipe: Arc<OwnedHandle>,
    event: OwnedHandle,
    overlapped: Box<OVERLAPPED>,
    pending: bool,
}
// OVERLAPPED is heap-stable for the complete kernel operation. Each operation
// has one owner; Drop cancels and drains before freeing its memory/handles.
unsafe impl Send for Operation {}
impl Operation {
    fn new(pipe: Arc<OwnedHandle>) -> io::Result<Self> {
        let event = unsafe { CreateEventW(null(), 1, 0, null()) };
        if event == 0 {
            return Err(io::Error::last_os_error());
        }
        let event = OwnedHandle(event);
        let mut overlapped: Box<OVERLAPPED> = Box::new(unsafe { std::mem::zeroed() });
        overlapped.hEvent = event.0;
        Ok(Self {
            pipe,
            event,
            overlapped,
            pending: false,
        })
    }
    fn cancel(&mut self) {
        if self.pending {
            unsafe {
                CancelIoEx(self.pipe.0, self.overlapped.as_mut());
            }
            let mut count = 0;
            unsafe {
                GetOverlappedResult(self.pipe.0, self.overlapped.as_mut(), &mut count, 1);
            }
            self.pending = false;
        }
    }
    fn poll(&mut self) -> io::Result<Option<u32>> {
        let result = unsafe { WaitForSingleObject(self.event.0, 0) };
        if result == WAIT_TIMEOUT {
            return Ok(None);
        }
        if result != WAIT_OBJECT_0 {
            self.cancel();
            return Err(io::Error::last_os_error());
        }
        let mut count = 0;
        let ok =
            unsafe { GetOverlappedResult(self.pipe.0, self.overlapped.as_mut(), &mut count, 0) };
        self.pending = false;
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Some(count))
    }
    fn wait(&mut self, until: Option<Instant>, stop: &AtomicBool) -> io::Result<u32> {
        loop {
            if stop.load(Ordering::Acquire) {
                self.cancel();
                return Err(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    "Windows local RPC stopped",
                ));
            }
            if until.is_some_and(|limit| Instant::now() >= limit) {
                self.cancel();
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Windows local RPC I/O timed out",
                ));
            }
            if let Some(count) = self.poll()? {
                return Ok(count);
            }
            let delay = until
                .map(|limit| {
                    limit
                        .saturating_duration_since(Instant::now())
                        .as_millis()
                        .min(20) as u32
                })
                .unwrap_or(20);
            let result = unsafe { WaitForSingleObject(self.event.0, delay.max(1)) };
            if result != WAIT_TIMEOUT && result != WAIT_OBJECT_0 {
                self.cancel();
                return Err(io::Error::last_os_error());
            }
        }
    }
}
impl Drop for Operation {
    fn drop(&mut self) {
        self.cancel();
    }
}

struct Pending {
    pipe: Arc<OwnedHandle>,
    connect: Operation,
    connected: bool,
}
impl Pending {
    fn new(name: &[u16], first: bool) -> Result<Self> {
        let descriptor = PrivateDescriptor::new(false)?;
        let attributes = descriptor.attributes();
        let flags = PIPE_ACCESS_DUPLEX
            | FILE_FLAG_OVERLAPPED
            | if first {
                FILE_FLAG_FIRST_PIPE_INSTANCE
            } else {
                0
            };
        let handle = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                flags,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                2,
                8192,
                8192,
                0,
                &attributes,
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(last_error("create private Windows local RPC pipe"));
        }
        let pipe = Arc::new(OwnedHandle(handle));
        verify_private_handle(handle, SE_KERNEL_OBJECT)?;
        let mut connect = Operation::new(pipe.clone())?;
        let connected = if unsafe { ConnectNamedPipe(handle, connect.overlapped.as_mut()) } != 0 {
            true
        } else {
            let error = unsafe { GetLastError() };
            if error == ERROR_IO_PENDING {
                connect.pending = true;
                false
            } else if error == ERROR_PIPE_CONNECTED {
                true
            } else {
                return Err(io::Error::from_raw_os_error(error as i32))
                    .context("wait for Windows local RPC peer");
            }
        };
        Ok(Self {
            pipe,
            connect,
            connected,
        })
    }
}
pub(crate) struct Listener {
    name: Vec<u16>,
    pending: Mutex<Pending>,
    stop: Arc<AtomicBool>,
}
impl Listener {
    pub(crate) fn bind(socket_path: &Path, stop: Arc<AtomicBool>) -> Result<Self> {
        create_private_directory(
            socket_path
                .parent()
                .context("Windows RPC directory missing")?,
        )?;
        let name = wide(std::ffi::OsStr::new(&pipe_name(socket_path)?))?;
        let pending = Pending::new(&name, true)?;
        Ok(Self {
            name,
            pending: Mutex::new(pending),
            stop,
        })
    }
    pub(crate) fn accept(&self) -> io::Result<(Stream, ())> {
        if self.stop.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "Windows local RPC stopped",
            ));
        }
        let mut pending = self.pending.lock().map_err(|_| {
            io::Error::new(io::ErrorKind::Other, "Windows pipe listener is poisoned")
        })?;
        if !pending.connected {
            if pending.connect.poll()?.is_none() {
                return Err(io::Error::from(io::ErrorKind::WouldBlock));
            }
            pending.connected = true;
        }
        if verify_peer(pending.pipe.0, true).is_err() {
            *pending = Pending::new(&self.name, false).map_err(io::Error::other)?;
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Windows local RPC peer identity was rejected",
            ));
        }
        let stream = Stream {
            pipe: pending.pipe.clone(),
            until: None,
            stop: self.stop.clone(),
        };
        let replacement = Pending::new(&self.name, false).map_err(io::Error::other)?;
        *pending = replacement;
        Ok((stream, ()))
    }
}
pub(crate) struct Stream {
    pipe: Arc<OwnedHandle>,
    until: Option<Instant>,
    stop: Arc<AtomicBool>,
}
impl Stream {
    pub(crate) fn connect(socket_path: &Path, timeout: Option<Duration>) -> Result<Self> {
        let until = deadline(timeout)?;
        let connect_until = until.unwrap_or_else(|| Instant::now() + Duration::from_secs(10));
        let name = wide(std::ffi::OsStr::new(&pipe_name(socket_path)?))?;
        loop {
            let handle = unsafe {
                CreateFileW(
                    name.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    null(),
                    OPEN_EXISTING,
                    FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                    0,
                )
            };
            if handle != INVALID_HANDLE_VALUE {
                let pipe = Arc::new(OwnedHandle(handle));
                verify_peer(handle, false)?;
                return Ok(Self {
                    pipe,
                    until,
                    stop: Arc::new(AtomicBool::new(false)),
                });
            }
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_PIPE_BUSY as i32) {
                return Err(error).context("connect Windows local RPC pipe");
            }
            if Instant::now() >= connect_until {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Windows local RPC connect timed out",
                ))
                .context("connect Windows local RPC pipe");
            }
            unsafe {
                WaitNamedPipeW(name.as_ptr(), 20);
            }
        }
    }
}
impl Read for Stream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let mut operation = Operation::new(self.pipe.clone())?;
        let ok = unsafe {
            ReadFile(
                self.pipe.0,
                buffer.as_mut_ptr(),
                buffer.len().min(u32::MAX as usize) as u32,
                null_mut(),
                operation.overlapped.as_mut(),
            )
        };
        if ok == 0 {
            let error = unsafe { GetLastError() };
            if error == ERROR_BROKEN_PIPE {
                return Ok(0);
            }
            if error != ERROR_IO_PENDING {
                return Err(io::Error::from_raw_os_error(error as i32));
            }
        }
        operation.pending = true;
        match operation.wait(self.until, &self.stop) {
            Ok(count) => Ok(count as usize),
            Err(error) if error.raw_os_error() == Some(ERROR_BROKEN_PIPE as i32) => Ok(0),
            Err(error) => Err(error),
        }
    }
}
impl Write for Stream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        let mut operation = Operation::new(self.pipe.clone())?;
        let ok = unsafe {
            WriteFile(
                self.pipe.0,
                bytes.as_ptr(),
                bytes.len().min(u32::MAX as usize) as u32,
                null_mut(),
                operation.overlapped.as_mut(),
            )
        };
        if ok == 0 {
            let error = unsafe { GetLastError() };
            if error != ERROR_IO_PENDING {
                return Err(io::Error::from_raw_os_error(error as i32));
            }
        }
        operation.pending = true;
        operation
            .wait(self.until, &self.stop)
            .map(|count| count as usize)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
