//! Windows carries the existing token-authorized runtime RPC over a private pipe.
use super::{
    execute_runtime_rpc_request_with_outbox, read_request_from, read_response_from,
    write_response_to, RuntimeRpcRequest, RuntimeRpcResponse,
};
use crate::windows_platform::pipe::{Listener, Stream};
use crate::{outbox::RuntimeOutbox, state::DaemonState};
use anyhow::Result;
use std::io::Write;
use std::path::Path;
use std::sync::{atomic::AtomicBool, Arc};
use std::time::Duration;

pub(crate) fn bind_pipe_listener(path: &Path, stop: Arc<AtomicBool>) -> Result<Listener> {
    Listener::bind(path, stop)
}
pub(crate) fn handle_pipe_stream_with_outbox(
    state: &DaemonState,
    outbox: &impl RuntimeOutbox,
    mut stream: Stream,
) -> Result<()> {
    let request = read_request_from(&mut stream)?;
    let response = match execute_runtime_rpc_request_with_outbox(state, outbox, request) {
        Ok(value) => value,
        Err(error) => RuntimeRpcResponse::failure("runtime_rpc_error", error.to_string()),
    };
    write_response_to(&mut stream, &response)
}
pub(crate) fn call_pipe_once(
    path: &Path,
    request: &RuntimeRpcRequest,
    timeout: Option<Duration>,
) -> Result<RuntimeRpcResponse> {
    let mut stream = Stream::connect(path, timeout)?;
    serde_json::to_writer(&mut stream, request)?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    read_response_from(stream)
}
