//! Verify the connected pipe instance before sending a plaintext user intent.
use std::io;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, PSID};
use windows_sys::Win32::Security::{
    EqualSid, GetTokenInformation, IsWellKnownSid, TokenUser, WinLocalSystemSid, TOKEN_QUERY,
    TOKEN_USER,
};
use windows_sys::Win32::System::Pipes::GetNamedPipeServerProcessId;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};

struct OwnedHandle(HANDLE);
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

struct UserToken {
    bytes: Vec<usize>,
}
impl UserToken {
    fn read(process: HANDLE) -> io::Result<Self> {
        let mut token = 0;
        // SAFETY: the process handle is live and token points to writable storage.
        if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = OwnedHandle(token);
        let mut required = 0;
        // SAFETY: null output with length zero is the documented size query.
        unsafe {
            GetTokenInformation(token.0, TokenUser, std::ptr::null_mut(), 0, &mut required);
        }
        if required < std::mem::size_of::<TOKEN_USER>() as u32 || required > 4096 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid token user size",
            ));
        }
        let mut bytes = vec![0usize; (required as usize).div_ceil(std::mem::size_of::<usize>())];
        let mut written = 0;
        // SAFETY: usize storage has TOKEN_USER alignment; the supplied byte
        // length fits the allocation. Windows initializes TOKEN_USER and SID.
        if unsafe {
            GetTokenInformation(
                token.0,
                TokenUser,
                bytes.as_mut_ptr().cast(),
                required,
                &mut written,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if written > required || written < std::mem::size_of::<TOKEN_USER>() as u32 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid token user result",
            ));
        }
        Ok(Self { bytes })
    }
    fn sid(&self) -> PSID {
        // SAFETY: initialized aligned TOKEN_USER lives in this owned buffer;
        // the SID it references stays valid while self is alive.
        unsafe { (*self.bytes.as_ptr().cast::<TOKEN_USER>()).User.Sid }
    }
}

pub(super) fn require_trusted_server(pipe: HANDLE) -> io::Result<()> {
    let mut pid = 0;
    // SAFETY: connected pipe handle; pid points to writable storage.
    if unsafe { GetNamedPipeServerProcessId(pipe, &mut pid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: query-only process access, no inherited handle.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process == 0 {
        return Err(io::Error::last_os_error());
    }
    let process = OwnedHandle(process);
    let server = UserToken::read(process.0)?;
    // SAFETY: GetCurrentProcess returns a live pseudo-handle, which is not closed.
    let current = UserToken::read(unsafe { GetCurrentProcess() })?;
    // SAFETY: both SIDs are kernel-returned TOKEN_USER outputs in live buffers.
    let trusted = unsafe {
        EqualSid(server.sid(), current.sid()) != 0
            || IsWellKnownSid(server.sid(), WinLocalSystemSid) != 0
    };
    if !trusted {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "listener server must belong to the current user or LocalSystem service",
        ));
    }
    Ok(())
}
