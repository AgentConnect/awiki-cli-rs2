//! Migration-only Content wire helpers for contract tests and CLI cutover checks.

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportProfile {
    RpcDefault,
    RpcReadHeavy,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RpcCall {
    pub endpoint: &'static str,
    pub method: &'static str,
    pub profile: TransportProfile,
    pub params: Value,
}

#[doc(hidden)]
pub const CONTENT_RPC_ENDPOINT: &str = crate::content::wire::CONTENT_RPC_ENDPOINT;

#[doc(hidden)]
pub fn build_create_page_rpc_call(draft: crate::content::PageDraft) -> crate::ImResult<RpcCall> {
    crate::content::wire::build_create_page_rpc_call(draft).map(Into::into)
}

#[doc(hidden)]
pub fn build_list_pages_rpc_call(query: crate::content::ContentPageQuery) -> RpcCall {
    crate::content::wire::build_list_pages_rpc_call(query).into()
}

#[doc(hidden)]
pub fn build_get_page_rpc_call(page: crate::content::PageRef) -> RpcCall {
    crate::content::wire::build_get_page_rpc_call(page).into()
}

#[doc(hidden)]
pub fn build_update_page_rpc_call(
    page: crate::content::PageRef,
    patch: crate::content::PageUpdate,
) -> crate::ImResult<RpcCall> {
    crate::content::wire::build_update_page_rpc_call(page, patch).map(Into::into)
}

#[doc(hidden)]
pub fn build_rename_page_rpc_call(
    page: crate::content::PageRef,
    target: crate::content::PageSlug,
) -> RpcCall {
    crate::content::wire::build_rename_page_rpc_call(page, target).into()
}

#[doc(hidden)]
pub fn build_delete_page_rpc_call(page: crate::content::PageRef) -> RpcCall {
    crate::content::wire::build_delete_page_rpc_call(page).into()
}

#[doc(hidden)]
pub fn normalize_page(value: Value) -> crate::ImResult<crate::content::PageDocument> {
    crate::content::wire::normalize_page(value, None)
}

#[doc(hidden)]
pub fn normalize_page_list(
    value: Value,
) -> crate::ImResult<crate::ids::Page<crate::content::PageDocument>> {
    crate::content::wire::normalize_page_list(value)
}

impl From<crate::content::wire::TransportProfile> for TransportProfile {
    fn from(value: crate::content::wire::TransportProfile) -> Self {
        match value {
            crate::content::wire::TransportProfile::RpcDefault => Self::RpcDefault,
            crate::content::wire::TransportProfile::RpcReadHeavy => Self::RpcReadHeavy,
        }
    }
}

impl From<crate::content::wire::RpcCall> for RpcCall {
    fn from(value: crate::content::wire::RpcCall) -> Self {
        Self {
            endpoint: value.endpoint,
            method: value.method,
            profile: value.profile.into(),
            params: value.params,
        }
    }
}
