#![deny(unsafe_code)]

mod client;
mod dto;
mod error;
#[cfg(test)]
mod mail_tests;
#[cfg(test)]
mod registration_tests;
mod state;

pub use client::{NativeExternalHttpAuthAttempt, NativeImCoreNodeClient, NativeRealtimeSession};
pub use dto::*;

use napi::bindgen_prelude::create_custom_tokio_runtime;
use napi_derive::{module_init, napi};

const NAPI_WORKER_STACK_BYTES: usize = 8 * 1024 * 1024;

#[module_init]
fn configure_napi_runtime() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("awiki-im-core-node")
        .thread_stack_size(NAPI_WORKER_STACK_BYTES)
        .build()
        .expect("awiki-im-core-node failed to create its async runtime");
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
    9
}

#[cfg(test)]
mod tests {
    #[test]
    fn recovery_attestation_uses_native_api_v9() {
        assert_eq!(super::native_api_version(), 9);
    }

    #[test]
    fn napi_runtime_reserves_stack_for_deep_core_futures() {
        assert_eq!(super::NAPI_WORKER_STACK_BYTES, 8 * 1024 * 1024);
    }
}

#[cfg(test)]
mod group_tests;
