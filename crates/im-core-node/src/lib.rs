#![deny(unsafe_code)]

mod client;
mod dto;
mod error;
mod state;

pub use client::{
    NativeExternalHttpAuthAttempt, NativeImCoreNodeClient, NativeImCoreNodeIdentityClient,
};
pub use dto::*;

use napi_derive::napi;

/// Opens one environment-scoped Rust IM Core and its default identity-bound
/// client. The same state root cannot be open in two processes or instances.
#[napi(catch_unwind, js_name = "openNativeClient")]
pub async fn open_native_client(options: NodeOpenOptions) -> napi::Result<NativeImCoreNodeClient> {
    error::napi_result(client::open(options).await)
}

/// Native facade contract version consumed by the TypeScript loader.
#[napi(js_name = "nativeApiVersion")]
pub fn native_api_version() -> u32 {
    4
}

#[cfg(test)]
mod tests {
    #[test]
    fn multi_identity_contract_uses_native_api_v4() {
        assert_eq!(super::native_api_version(), 4);
    }
}
