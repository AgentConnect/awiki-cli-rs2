//! Windows host primitives. No secret, descriptor, or token contents in errors.
use anyhow::{bail, Context, Result};
use std::ffi::c_void;
use std::fs::File;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::MetadataExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::Path;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Security::Authorization::*;
use windows_sys::Win32::Security::Cryptography::*;
use windows_sys::Win32::Security::*;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::SystemServices::ACCESS_ALLOWED_ACE_TYPE;
use windows_sys::Win32::System::Threading::*;
use windows_sys::Win32::System::IO::OVERLAPPED;
use zeroize::Zeroizing;

pub(crate) struct OwnedHandle(pub(crate) HANDLE);
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if self.0 != 0 && self.0 != INVALID_HANDLE_VALUE {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}
struct LocalAllocation(*mut c_void);
impl Drop for LocalAllocation {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                LocalFree(self.0 as _);
            }
        }
    }
}

pub(crate) fn wide(value: &std::ffi::OsStr) -> Result<Vec<u16>> {
    let mut value: Vec<u16> = value.encode_wide().collect();
    if value.contains(&0) {
        bail!("Windows host input contains an embedded NUL");
    }
    value.push(0);
    Ok(value)
}
fn checked(ok: BOOL, operation: &'static str) -> Result<()> {
    if ok == 0 {
        return Err(io::Error::last_os_error()).context(operation);
    }
    Ok(())
}
fn sid_string(sid: PSID) -> Result<String> {
    if sid.is_null() || unsafe { IsValidSid(sid) } == 0 {
        bail!("Windows host identity has an invalid SID");
    }
    let mut raw = null_mut();
    checked(
        unsafe { ConvertSidToStringSidW(sid, &mut raw) },
        "format Windows host identity",
    )?;
    let allocation = LocalAllocation(raw as _);
    let mut length = 0;
    unsafe {
        while *raw.add(length) != 0 {
            length += 1;
        }
    }
    let text = String::from_utf16(unsafe { std::slice::from_raw_parts(raw, length) })
        .context("decode Windows identity")?;
    drop(allocation);
    Ok(text)
}
fn process_token(process: HANDLE) -> Result<OwnedHandle> {
    let mut token = 0;
    checked(
        unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) },
        "open Windows process identity",
    )?;
    Ok(OwnedHandle(token))
}
fn token_information(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> Result<Vec<usize>> {
    let mut length = 0;
    unsafe {
        GetTokenInformation(token, class, null_mut(), 0, &mut length);
    }
    if unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER || length == 0 {
        return Err(io::Error::last_os_error()).context("measure Windows process identity");
    }
    let mut data = vec![
        0usize;
        (length as usize + std::mem::size_of::<usize>() - 1)
            / std::mem::size_of::<usize>()
    ];
    checked(
        unsafe { GetTokenInformation(token, class, data.as_mut_ptr() as _, length, &mut length) },
        "read Windows process identity",
    )?;
    Ok(data)
}
pub(crate) fn process_user_sid(process: HANDLE) -> Result<String> {
    let token = process_token(process)?;
    let data = token_information(token.0, TokenUser)?;
    let user = unsafe { &*(data.as_ptr() as *const TOKEN_USER) };
    sid_string(user.User.Sid)
}
pub(crate) fn current_user_sid() -> Result<String> {
    process_user_sid(unsafe { GetCurrentProcess() })
}
fn elevated() -> Result<bool> {
    let token = process_token(unsafe { GetCurrentProcess() })?;
    let mut elevation: TOKEN_ELEVATION = unsafe { std::mem::zeroed() };
    let mut length = 0;
    checked(
        unsafe {
            GetTokenInformation(
                token.0,
                TokenElevation,
                &mut elevation as *mut _ as _,
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut length,
            )
        },
        "read Windows elevation state",
    )?;
    Ok(elevation.TokenIsElevated != 0)
}

pub(crate) struct PrivateDescriptor {
    allocation: LocalAllocation,
}
impl PrivateDescriptor {
    pub(crate) fn new(inherit: bool) -> Result<Self> {
        let sid = current_user_sid()?;
        let flags = if inherit { "OICI" } else { "" };
        let sddl = format!("O:{sid}D:P(A;{flags};FA;;;{sid})(A;{flags};FA;;;SY)");
        let input = wide(std::ffi::OsStr::new(&sddl))?;
        let mut descriptor = null_mut();
        checked(
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    input.as_ptr(),
                    SDDL_REVISION_1,
                    &mut descriptor,
                    null_mut(),
                )
            },
            "create private Windows security descriptor",
        )?;
        Ok(Self {
            allocation: LocalAllocation(descriptor),
        })
    }
    pub(crate) fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.allocation.0,
            bInheritHandle: 0,
        }
    }
    fn owner_and_dacl(&self) -> Result<(PSID, *mut ACL)> {
        let mut owner = null_mut();
        let mut defaulted = 0;
        checked(
            unsafe { GetSecurityDescriptorOwner(self.allocation.0, &mut owner, &mut defaulted) },
            "read private descriptor owner",
        )?;
        let mut present = 0;
        let mut dacl = null_mut();
        checked(
            unsafe {
                GetSecurityDescriptorDacl(
                    self.allocation.0,
                    &mut present,
                    &mut dacl,
                    &mut defaulted,
                )
            },
            "read private descriptor ACL",
        )?;
        if present == 0 || dacl.is_null() {
            bail!("private Windows descriptor has no ACL");
        }
        Ok((owner, dacl))
    }
}

pub(crate) fn reject_reparse_path(path: &Path) -> Result<()> {
    for part in path.ancestors() {
        match std::fs::symlink_metadata(part) {
            Ok(metadata) if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 => {
                bail!("Windows daemon paths cannot traverse reparse points")
            }
            Ok(_) => (),
            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
            Err(error) => return Err(error).context("inspect Windows daemon path"),
        }
    }
    Ok(())
}
fn open_security_handle(path: &Path, write: bool) -> Result<OwnedHandle> {
    reject_reparse_path(path)?;
    let path = wide(path.as_os_str())?;
    let access =
        READ_CONTROL | FILE_READ_ATTRIBUTES | if write { WRITE_DAC | WRITE_OWNER } else { 0 };
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            0,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error()).context("open Windows daemon path security");
    }
    let handle = OwnedHandle(handle);
    let mut info = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
    checked(
        unsafe { GetFileInformationByHandle(handle.0, info.as_mut_ptr()) },
        "inspect opened Windows daemon path",
    )?;
    if unsafe { info.assume_init() }.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        bail!("Windows daemon path is a reparse point");
    }
    Ok(handle)
}
fn security_info(
    handle: HANDLE,
    object_type: SE_OBJECT_TYPE,
) -> Result<(LocalAllocation, PSID, *mut ACL)> {
    let mut owner = null_mut();
    let mut acl = null_mut();
    let mut descriptor = null_mut();
    let code = unsafe {
        GetSecurityInfo(
            handle,
            object_type,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            &mut acl,
            null_mut(),
            &mut descriptor,
        )
    };
    if code != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(code as i32))
            .context("read Windows daemon security");
    }
    Ok((LocalAllocation(descriptor), owner, acl))
}
pub(crate) fn verify_private_handle(handle: HANDLE, object_type: SE_OBJECT_TYPE) -> Result<()> {
    let (descriptor, owner, acl) = security_info(handle, object_type)?;
    let user = current_user_sid()?;
    if sid_string(owner)? != user || acl.is_null() {
        bail!("Windows daemon object owner or ACL is not private");
    }
    let mut control = 0;
    let mut revision = 0;
    checked(
        unsafe { GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision) },
        "read Windows daemon ACL protection",
    )?;
    if control & SE_DACL_PROTECTED == 0 {
        bail!("Windows daemon object inherits an unprotected ACL");
    }
    let mut user_allowed = false;
    let mut system_allowed = false;
    for index in 0..unsafe { (*acl).AceCount } as u32 {
        let mut ace = null_mut();
        checked(
            unsafe { GetAce(acl, index, &mut ace) },
            "read Windows daemon ACL entry",
        )?;
        let header = unsafe { &*(ace as *const ACE_HEADER) };
        if u32::from(header.AceType) != ACCESS_ALLOWED_ACE_TYPE {
            bail!("Windows daemon ACL contains an unsupported entry");
        }
        let allowed = unsafe { &*(ace as *const ACCESS_ALLOWED_ACE) };
        let sid = sid_string(std::ptr::addr_of!(allowed.SidStart) as PSID)?;
        if sid != user && sid != "S-1-5-18" {
            bail!("Windows daemon object grants access outside its owner and SYSTEM");
        }
        if u32::from(header.AceFlags) & INHERIT_ONLY_ACE == 0 {
            if sid == user {
                user_allowed = true;
            }
            if sid == "S-1-5-18" {
                system_allowed = true;
            }
        }
    }
    if !user_allowed || !system_allowed {
        bail!("Windows daemon ACL omits its owner or SYSTEM");
    }
    Ok(())
}
pub(crate) fn secure_path(path: &Path, directory: bool) -> Result<()> {
    let handle = open_security_handle(path, true)?;
    let (_allocation, owner, _) = security_info(handle.0, SE_FILE_OBJECT)?;
    let original = sid_string(owner)?;
    if original != current_user_sid()? && !(original == "S-1-5-32-544" && elevated()?) {
        bail!("Windows daemon refuses a path owned by another identity");
    }
    let descriptor = PrivateDescriptor::new(directory)?;
    let (owner, dacl) = descriptor.owner_and_dacl()?;
    let code = unsafe {
        SetSecurityInfo(
            handle.0,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION
                | DACL_SECURITY_INFORMATION
                | PROTECTED_DACL_SECURITY_INFORMATION,
            owner,
            null_mut(),
            dacl,
            null_mut(),
        )
    };
    if code != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(code as i32))
            .context("protect Windows daemon path ACL");
    }
    verify_private_handle(handle.0, SE_FILE_OBJECT)
}
pub(crate) fn verify_private_path(path: &Path) -> Result<()> {
    let handle = open_security_handle(path, false)?;
    verify_private_handle(handle.0, SE_FILE_OBJECT)
}
pub(crate) fn create_private_directory(path: &Path) -> Result<()> {
    reject_reparse_path(path)?;
    if path.exists() {
        if !path.is_dir() {
            bail!("Windows daemon directory path is not a directory");
        }
        return secure_path(path, true);
    }
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            create_private_directory(parent)?;
        }
    }
    let descriptor = PrivateDescriptor::new(true)?;
    let attributes = descriptor.attributes();
    let name = wide(path.as_os_str())?;
    if unsafe { CreateDirectoryW(name.as_ptr(), &attributes) } == 0 {
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::AlreadyExists {
            return Err(error).context("create private Windows daemon directory");
        }
    }
    verify_private_path(path)
}
pub(crate) fn create_private_file(path: &Path) -> io::Result<File> {
    create_private_file_with_sharing(path, FILE_SHARE_READ)
}
fn create_private_file_with_sharing(path: &Path, sharing: FILE_SHARE_MODE) -> io::Result<File> {
    let result = (|| -> Result<File> {
        reject_reparse_path(path)?;
        let descriptor = PrivateDescriptor::new(false)?;
        let attributes = descriptor.attributes();
        let name = wide(path.as_os_str())?;
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                sharing,
                &attributes,
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT,
                0,
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error()).context("create private Windows daemon file");
        }
        let file = unsafe { File::from_raw_handle(handle as _) };
        verify_private_handle(handle, SE_FILE_OBJECT)?;
        Ok(file)
    })();
    result.map_err(|error| {
        error
            .downcast::<io::Error>()
            .unwrap_or_else(|error| io::Error::new(io::ErrorKind::Other, error))
    })
}
pub(crate) fn lock_private_file(path: &Path) -> Result<File> {
    if let Some(parent) = path.parent() {
        create_private_directory(parent)?;
    }
    let file = match create_private_file_with_sharing(path, FILE_SHARE_READ | FILE_SHARE_WRITE) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            verify_private_path(path)?;
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .context("open Windows daemon owner lock")?
        }
        Err(error) => return Err(error).context("create Windows daemon owner lock"),
    };
    let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
    if unsafe {
        LockFileEx(
            file.as_raw_handle() as _,
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            0,
            1,
            0,
            &mut overlapped,
        )
    } == 0
    {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_LOCK_VIOLATION as i32) {
            bail!("daemon_state_root_busy: another process owns this state root");
        }
        return Err(error).context("lock Windows daemon state root");
    }
    Ok(file)
}
const ROOT_ENTROPY: &[u8] = b"awiki.daemon.windows.vault-root.v1";
pub(crate) fn protect_secret(bytes: &[u8]) -> Result<Vec<u8>> {
    dpapi(bytes, true).map(|value| value.to_vec())
}
pub(crate) fn unprotect_secret(bytes: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    dpapi(bytes, false)
}
fn dpapi(bytes: &[u8], protect: bool) -> Result<Zeroizing<Vec<u8>>> {
    let length: u32 = bytes
        .len()
        .try_into()
        .context("Windows protected secret is too large")?;
    let input = CRYPT_INTEGER_BLOB {
        cbData: length,
        pbData: bytes.as_ptr() as _,
    };
    let entropy = CRYPT_INTEGER_BLOB {
        cbData: ROOT_ENTROPY.len() as u32,
        pbData: ROOT_ENTROPY.as_ptr() as _,
    };
    let mut output: CRYPT_INTEGER_BLOB = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        if protect {
            CryptProtectData(
                &input,
                null(),
                &entropy,
                null(),
                null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                null_mut(),
                &entropy,
                null(),
                null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    checked(ok, "protect or open Windows daemon secret")?;
    let allocation = LocalAllocation(output.pbData as _);
    let data = Zeroizing::new(if output.cbData == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec()
    });
    if !protect {
        unsafe {
            std::ptr::write_bytes(output.pbData, 0, output.cbData as usize);
        }
    }
    drop(allocation);
    Ok(data)
}

#[cfg(test)]
mod tests;

pub(crate) mod pipe;
