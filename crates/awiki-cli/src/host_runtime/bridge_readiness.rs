//! Wait for socket readiness instead of adding a polling sleep to every IPC.
use std::{io, os::fd::AsRawFd, os::unix::net::UnixListener, time::Duration};

pub(super) fn wait(listener: &UnixListener, timeout: Duration) -> io::Result<()> {
    let mut descriptor = libc::pollfd {
        fd: listener.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    let milliseconds = timeout.as_millis().min(i32::MAX as u128) as i32;
    // SAFETY: the borrowed listener remains live and the one-element pollfd
    // buffer is initialized, writable, and valid for this bounded call.
    let result = unsafe { libc::poll(&mut descriptor, 1, milliseconds) };
    if result < 0 {
        let error = io::Error::last_os_error();
        return if error.kind() == io::ErrorKind::Interrupted {
            Ok(())
        } else {
            Err(error)
        };
    }
    if descriptor.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
        return Err(io::Error::new(
            io::ErrorKind::ConnectionAborted,
            "listener readiness failed",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "bridge_readiness_tests.rs"]
mod tests;
