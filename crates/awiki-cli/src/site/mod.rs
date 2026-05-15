mod client;
mod service;
mod types;
mod wire;

pub use client::Client;
pub use service::{
    create_page, delete_page, get_page, get_root, list_pages, rename_page, set_root, update_page,
};
pub use types::{
    CommandResult, CreatePageParams, IdentitySummary, RenamePageParams, SetRootParams, SiteError,
    UpdatePageParams, DID_AUTH_RPC_ENDPOINT, SITE_RPC_ENDPOINT,
};
pub use wire::{
    build_create_page_rpc_call, build_delete_page_rpc_call, build_get_page_rpc_call,
    build_get_root_rpc_call, build_list_pages_rpc_call, build_rename_page_rpc_call,
    build_set_root_rpc_call, build_update_page_rpc_call, create_page_summary, delete_page_summary,
    get_page_summary, get_root_summary, list_pages_summary, normalize_domain,
    normalize_live_domain, normalize_slug, page_create_result, page_delete_result, page_get_result,
    page_list_result, page_rename_result, page_update_result, rename_page_summary, root_get_result,
    root_set_result, set_root_summary, site_page_action_result, site_page_delete_result,
    site_page_list_result, site_page_rename_result, site_root_action_result, update_page_summary,
    RpcCall,
};
