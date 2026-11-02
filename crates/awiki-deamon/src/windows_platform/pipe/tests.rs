use super::*;
fn accept(listener: &Listener) -> Stream {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match listener.accept() {
            Ok((stream, _)) => return stream,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("native pipe accept failed: {error}"),
        }
    }
}
#[test]
fn private_pipe_roundtrip_checks_kernel_peers_and_rejects_duplicate_listener() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("rpc/local.sock");
    let stop = Arc::new(AtomicBool::new(false));
    let listener = Listener::bind(&path, stop).unwrap();
    assert!(Listener::bind(&path, Arc::new(AtomicBool::new(false))).is_err());
    let child = std::thread::spawn(move || {
        let mut stream = accept(&listener);
        let mut request = [0u8; 4];
        stream.read_exact(&mut request).unwrap();
        assert_eq!(&request, b"ping");
        stream.write_all(b"pong").unwrap();
    });
    let mut client = Stream::connect(&path, Some(Duration::from_secs(3))).unwrap();
    client.write_all(b"ping").unwrap();
    let mut response = [0u8; 4];
    client.read_exact(&mut response).unwrap();
    assert_eq!(&response, b"pong");
    child.join().unwrap();
}
#[test]
fn a_stalled_read_times_out_without_leaving_pending_io() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("rpc/local.sock");
    let stop = Arc::new(AtomicBool::new(false));
    let listener = Listener::bind(&path, stop).unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    let child = std::thread::spawn(move || {
        let stream = accept(&listener);
        send.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(250));
        drop(stream);
    });
    let mut client = Stream::connect(&path, Some(Duration::from_millis(150))).unwrap();
    receive.recv().unwrap();
    let mut byte = [0u8; 1];
    assert_eq!(
        client.read(&mut byte).unwrap_err().kind(),
        io::ErrorKind::TimedOut
    );
    child.join().unwrap();
}
#[test]
fn shutdown_interrupts_a_connected_client_that_sends_no_request() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("rpc/local.sock");
    let stop = Arc::new(AtomicBool::new(false));
    let listener = Listener::bind(&path, stop.clone()).unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    let child = std::thread::spawn(move || {
        let mut stream = accept(&listener);
        send.send(()).unwrap();
        let mut byte = [0u8; 1];
        stream.read(&mut byte).unwrap_err().kind()
    });
    let _client = Stream::connect(&path, Some(Duration::from_secs(2))).unwrap();
    receive.recv().unwrap();
    stop.store(true, Ordering::Release);
    assert_eq!(child.join().unwrap(), io::ErrorKind::ConnectionAborted);
}
