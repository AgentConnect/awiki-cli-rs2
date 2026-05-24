#![allow(unused_imports)]

mod client;
mod service;
mod types;
mod wire;

pub use client::Client;
pub use service::{
    account, attachment, inbox, mark_read, notifications, notifications_plan, read, send,
    split_mail_list,
};
pub use types::{
    account_plan, attachment_download_plan, inbox_plan, mark_read_plan, read_plan, send_plan,
    AccountRequest, AttachmentRequest, CommandResult, InboxRequest, MailError, MarkReadRequest,
    ReadRequest, SendRequest, MAIL_RPC_ENDPOINT,
};
pub use wire::{
    account_summary, attachment_summary, build_account_rpc_call, build_attachment_rpc_call,
    build_inbox_rpc_call, build_mark_read_rpc_call, build_read_rpc_call, build_send_rpc_call,
    inbox_summary, mark_read_summary, read_summary, send_summary, RpcCall, ServiceError,
};
