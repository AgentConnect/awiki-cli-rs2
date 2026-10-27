use super::*;
use std::{os::unix::net::UnixStream, sync::mpsc};

#[test]
fn queued_connection_wakes_the_listener_before_the_idle_deadline() {
    use std::os::unix::fs::DirBuilderExt;
    struct Directory(std::path::PathBuf);
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let directory = Directory(std::env::temp_dir().join(format!(
        "abr-{}-{:x}",
        std::process::id(),
        rand::random::<u64>()
    )));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&directory.0)
        .unwrap();
    let path = directory.0.join("listener.sock");
    let listener = UnixListener::bind(&path).unwrap();
    listener.set_nonblocking(true).unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let (accepted_tx, accepted_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        wait(&listener, Duration::from_secs(2)).unwrap();
        let (stream, _) = listener.accept().unwrap();
        accepted_tx.send(()).unwrap();
        drop(stream);
    });
    started_rx.recv().unwrap();
    let _client = UnixStream::connect(path).unwrap();
    // A fixed sleep of the supplied idle timeout would fail this bound.
    accepted_rx
        .recv_timeout(Duration::from_millis(500))
        .unwrap();
    worker.join().unwrap();
}
