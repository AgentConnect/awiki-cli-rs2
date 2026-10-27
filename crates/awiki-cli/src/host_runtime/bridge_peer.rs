//! Peer credentials protect intent plaintext even if a socket pathname is
//! replaced between capability probing and the business connection.
use std::{io, os::unix::net::UnixStream};

pub(super) fn require_same_user(stream: &UnixStream) -> io::Result<()> {
    let uid = peer_uid(stream)?;
    // SAFETY: geteuid has no arguments or memory preconditions.
    let own = unsafe { libc::geteuid() };
    require_uid(uid, own)
}

fn require_uid(peer: libc::uid_t, own: libc::uid_t) -> io::Result<()> {
    if peer != own {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "listener peer must have the current effective UID",
        ));
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn peer_uid(stream: &UnixStream) -> io::Result<libc::uid_t> {
    use std::os::fd::AsRawFd;
    let mut credentials = std::mem::MaybeUninit::<libc::ucred>::uninit();
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: live connected socket; writable ucred storage and exact supplied
    // length. Read initialized credentials only after success and length check.
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            credentials.as_mut_ptr().cast(),
            &mut length,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    if length as usize != std::mem::size_of::<libc::ucred>() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unexpected peer credential length",
        ));
    }
    // SAFETY: checked successful initialization of the complete structure.
    Ok(unsafe { credentials.assume_init() }.uid)
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]
fn peer_uid(stream: &UnixStream) -> io::Result<libc::uid_t> {
    use std::os::fd::AsRawFd;
    let (mut uid, mut gid) = (0, 0);
    // SAFETY: the socket is live and both outputs point to initialized values.
    if unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(uid)
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
)))]
fn peer_uid(_: &UnixStream) -> io::Result<libc::uid_t> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "peer credential verification is unavailable",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn connected_same_user_is_allowed_and_foreign_uid_is_rejected() {
        let (left, right) = UnixStream::pair().unwrap();
        require_same_user(&left).unwrap();
        require_same_user(&right).unwrap();
        assert_eq!(
            require_uid(10, 11).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
    }
}
