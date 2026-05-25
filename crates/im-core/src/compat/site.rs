//! Migration-only Site wire helpers for contract tests and CLI cutover checks.

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
pub const SITE_RPC_ENDPOINT: &str = crate::site::wire::SITE_RPC_ENDPOINT;

#[doc(hidden)]
pub fn build_get_root_rpc_call(domain: crate::site::SiteDomain) -> RpcCall {
    crate::site::wire::build_get_root_rpc_call(domain).into()
}

#[doc(hidden)]
pub fn build_set_root_rpc_call(draft: crate::site::SiteRootDraft) -> RpcCall {
    crate::site::wire::build_set_root_rpc_call(draft).into()
}

#[doc(hidden)]
pub fn build_list_pages_rpc_call(query: crate::site::SitePageQuery) -> RpcCall {
    crate::site::wire::build_list_pages_rpc_call(query).into()
}

#[doc(hidden)]
pub fn build_get_page_rpc_call(page: crate::site::SitePageRef) -> RpcCall {
    crate::site::wire::build_get_page_rpc_call(page).into()
}

#[doc(hidden)]
pub fn build_create_page_rpc_call(draft: crate::site::SitePageDraft) -> RpcCall {
    crate::site::wire::build_create_page_rpc_call(draft).into()
}

#[doc(hidden)]
pub fn build_update_page_rpc_call(
    page: crate::site::SitePageRef,
    patch: crate::site::SitePageUpdate,
) -> RpcCall {
    crate::site::wire::build_update_page_rpc_call(page, patch).into()
}

#[doc(hidden)]
pub fn build_rename_page_rpc_call(
    page: crate::site::SitePageRef,
    target: crate::content::PageSlug,
) -> RpcCall {
    crate::site::wire::build_rename_page_rpc_call(page, target).into()
}

#[doc(hidden)]
pub fn build_delete_page_rpc_call(page: crate::site::SitePageRef) -> RpcCall {
    crate::site::wire::build_delete_page_rpc_call(page).into()
}

#[doc(hidden)]
pub fn normalize_root(
    value: Value,
    domain: &crate::site::SiteDomain,
) -> crate::ImResult<crate::site::SiteRootDocument> {
    crate::site::wire::normalize_root(value, domain)
}

#[doc(hidden)]
pub fn normalize_page_list(
    domain: &crate::site::SiteDomain,
    value: Value,
) -> crate::ImResult<crate::ids::Page<crate::site::SitePageDocument>> {
    crate::site::wire::normalize_page_list(domain, value)
}

impl From<crate::site::wire::TransportProfile> for TransportProfile {
    fn from(value: crate::site::wire::TransportProfile) -> Self {
        match value {
            crate::site::wire::TransportProfile::RpcDefault => Self::RpcDefault,
            crate::site::wire::TransportProfile::RpcReadHeavy => Self::RpcReadHeavy,
        }
    }
}

impl From<crate::site::wire::RpcCall> for RpcCall {
    fn from(value: crate::site::wire::RpcCall) -> Self {
        Self {
            endpoint: value.endpoint,
            method: value.method,
            profile: value.profile.into(),
            params: value.params,
        }
    }
}
