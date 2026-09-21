//! Local DID resolution for bootstrap tests. No external identity service.
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::JoinHandle,
    time::Duration,
};

pub(super) struct FixtureRoot {
    root: tempfile::TempDir,
    address: SocketAddr,
    stopped: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl FixtureRoot {
    pub(super) fn new(root: tempfile::TempDir, document: PathBuf) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let stopped = Arc::new(AtomicBool::new(false));
        let shutdown = stopped.clone();
        let worker = std::thread::spawn(move || {
            for socket in listener.incoming() {
                if shutdown.load(Ordering::Acquire) {
                    break;
                }
                let mut socket = socket.unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                socket
                    .set_write_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = [0; 8192];
                let Ok(count) = socket.read(&mut request) else {
                    continue;
                };
                let valid = request[..count].starts_with(b"GET /user/alice/did.json HTTP/");
                let body = if valid {
                    std::fs::read(&document).ok()
                } else {
                    None
                };
                let status = if body.is_some() {
                    "200 OK"
                } else {
                    "404 Not Found"
                };
                let body = body.unwrap_or_default();
                let header = format!("HTTP/1.1 {status}\r\nContent-Type: application/did+json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = socket
                    .write_all(header.as_bytes())
                    .and_then(|_| socket.write_all(&body));
            }
        });
        Self {
            root,
            address,
            stopped,
            worker: Some(worker),
        }
    }

    pub(super) fn path(&self) -> &Path {
        self.root.path()
    }
    pub(super) fn base_url(&self) -> String {
        format!("http://{}", self.address)
    }
}

impl Drop for FixtureRoot {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        let _ = TcpStream::connect(self.address);
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}
