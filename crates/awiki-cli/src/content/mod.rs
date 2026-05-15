mod client;
mod service;
mod types;
mod wire;

pub use client::Client;
pub use service::{create_page, delete_page, get_page, list_pages, rename_page, update_page};
pub use types::{
    CommandResult, ContentError, CreatePageParams, IdentitySummary, RenamePageParams,
    UpdatePageParams, CONTENT_RPC_ENDPOINT, DID_AUTH_RPC_ENDPOINT,
};
pub use wire::{
    build_create_page_rpc_call, build_delete_page_rpc_call, build_get_page_rpc_call,
    build_list_pages_rpc_call, build_rename_page_rpc_call, build_update_page_rpc_call,
    create_page_summary, delete_page_summary, get_page_summary, list_pages_summary,
    normalize_visibility, page_action_result, page_delete_result, page_list_result,
    page_rename_result, page_update_result, read_heavy_page_action_result, rename_page_summary,
    update_page_summary, RpcCall,
};
