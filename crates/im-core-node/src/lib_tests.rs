use super::build_node_async_runtime;

const PROBE_FRAME_BYTES: usize = 4 * 1024;
const PROBE_DEPTH: usize = 768;

#[inline(never)]
fn consume_worker_stack(depth: usize, seed: u8) -> u8 {
    let frame = [seed; PROBE_FRAME_BYTES];
    std::hint::black_box(&frame);
    let result = if depth == 0 {
        frame[0]
    } else {
        consume_worker_stack(depth - 1, seed.wrapping_add(1)) ^ frame[0]
    };
    std::hint::black_box(&frame);
    result
}

#[test]
fn node_async_runtime_supports_deep_message_futures() {
    let runtime = build_node_async_runtime().unwrap();

    runtime.block_on(async {
        tokio::spawn(async { consume_worker_stack(PROBE_DEPTH, 1) })
            .await
            .unwrap();
    });
}
