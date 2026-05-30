use std::fs;
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;

use anyhow::{bail, Context, Result};

use crate::local_rpc::{handle_runtime_rpc_request, read_request_from, write_response_to};
use crate::state::DaemonState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerCredential {
    pub uid: u32,
    pub gid: u32,
    pub pid: Option<u32>,
}

pub fn serve_one_uds_request(state: &DaemonState, socket_path: &Path) -> Result<()> {
    prepare_socket_path(socket_path)?;
    let listener = UnixListener::bind(socket_path)
        .with_context(|| format!("bind daemon local RPC {}", socket_path.display()))?;
    fs::set_permissions(socket_path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("chmod daemon local RPC socket {}", socket_path.display()))?;
    let (mut stream, _) = listener.accept()?;
    verify_peer_credential(&stream)?;
    let request = read_request_from(stream.try_clone()?)?;
    let response = handle_runtime_rpc_request(state, request);
    write_response_to(&mut stream, &response)?;
    Ok(())
}

pub fn verify_socket_permissions(socket_path: &Path) -> Result<()> {
    let metadata = fs::metadata(socket_path)
        .with_context(|| format!("read daemon local RPC socket {}", socket_path.display()))?;
    let mode = metadata.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        bail!("daemon local RPC socket permissions are too broad: {mode:o}");
    }
    let parent = socket_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("daemon local RPC socket has no parent directory"))?;
    let parent_mode = fs::metadata(parent)?.permissions().mode() & 0o777;
    if parent_mode & 0o077 != 0 {
        bail!("daemon local RPC socket parent permissions are too broad: {parent_mode:o}");
    }
    Ok(())
}

fn prepare_socket_path(socket_path: &Path) -> Result<()> {
    let parent = socket_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("daemon local RPC socket has no parent directory"))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("create daemon local RPC directory {}", parent.display()))?;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("chmod daemon local RPC directory {}", parent.display()))?;
    match fs::remove_file(socket_path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err).context("remove stale daemon local RPC socket"),
    }
    Ok(())
}

fn verify_peer_credential(stream: &UnixStream) -> Result<PeerCredential> {
    let credential = peer_credential(stream)?;
    let current_uid = unsafe { libc::geteuid() } as u32;
    if credential.uid != current_uid {
        bail!(
            "daemon local RPC peer uid {} does not match daemon uid {}",
            credential.uid,
            current_uid
        );
    }
    Ok(credential)
}

#[cfg(target_os = "linux")]
fn peer_credential(stream: &UnixStream) -> Result<PeerCredential> {
    let mut credential = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut credential as *mut _ as *mut libc::c_void,
            &mut len,
        )
    };
    if rc != 0 {
        return Err(std::io::Error::last_os_error())
            .context("read daemon local RPC peer credential");
    }
    Ok(PeerCredential {
        uid: credential.uid,
        gid: credential.gid,
        pid: Some(credential.pid as u32),
    })
}

#[cfg(target_os = "macos")]
fn peer_credential(stream: &UnixStream) -> Result<PeerCredential> {
    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;
    let rc = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error())
            .context("read daemon local RPC peer credential");
    }
    Ok(PeerCredential {
        uid,
        gid,
        pid: None,
    })
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn peer_credential(_stream: &UnixStream) -> Result<PeerCredential> {
    bail!("peer credential check is not implemented for this Unix platform yet");
}
