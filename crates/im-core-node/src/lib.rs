#![deny(unsafe_code)]

mod client;
mod dto;
mod error;
mod state;

pub use client::NativeImCoreNodeClient;
pub use dto::*;

use napi::bindgen_prelude::create_custom_tokio_runtime;
use napi_derive::napi;

const NODE_ASYNC_WORKER_STACK_BYTES: usize = 16 * 1024 * 1024;

fn build_node_async_runtime() -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("awiki-im-core-node")
        .thread_stack_size(NODE_ASYNC_WORKER_STACK_BYTES)
        .build()
}

#[napi_derive::module_init]
fn initialize_node_async_runtime() {
    let runtime = build_node_async_runtime().expect("build AWiki IM Core Node async runtime");
    create_custom_tokio_runtime(runtime);
}

/// Opens one environment-scoped Rust IM Core and its default identity-bound
/// client. The same state root cannot be open in two processes or instances.
#[napi(catch_unwind, js_name = "openNativeClient")]
pub async fn open_native_client(options: NodeOpenOptions) -> napi::Result<NativeImCoreNodeClient> {
    error::napi_result(client::open(options).await)
}

/// Native facade contract version consumed by the TypeScript loader.
#[napi(js_name = "nativeApiVersion")]
pub fn native_api_version() -> u32 {
    1
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod lib_tests;
