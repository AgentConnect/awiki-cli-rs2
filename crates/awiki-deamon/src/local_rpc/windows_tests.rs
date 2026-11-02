use super::*;
use crate::outbox::MemoryRuntimeOutbox;
use crate::DaemonConfig;
use serde_json::json;

#[test]
fn native_pipe_still_rejects_invalid_runtime_tokens_without_echoing_them() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open_with_root_key_bytes(&config, [0x43u8; 32]);
    state.initialize().unwrap();
    let listener =
        bind_pipe_listener(&config.local_socket_path, Arc::new(AtomicBool::new(false))).unwrap();
    let child = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(std::time::Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("native runtime RPC accept failed: {error}"),
            }
        };
        handle_pipe_stream_with_outbox(&state, &MemoryRuntimeOutbox::default(), stream).unwrap();
    });
    let token = "invalid-unit-runtime-token";
    let response = call_pipe_once(
        &config.local_socket_path,
        &RuntimeRpcRequest {
            runtime_rpc_token: token.into(),
            method: "task.status".into(),
            params: json!({"task_id":"unit-task"}),
            debug: None,
        },
        Some(Duration::from_secs(3)),
    )
    .unwrap();
    assert!(!response.ok);
    assert_eq!(response.error.as_ref().unwrap().code, "runtime_rpc_error");
    assert!(!serde_json::to_string(&response).unwrap().contains(token));
    child.join().unwrap();
}
