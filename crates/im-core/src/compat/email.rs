//! Migration-only Email wire helpers for contract tests and CLI cutover checks.

use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct EmailRpcCall {
    pub endpoint: &'static str,
    pub method: &'static str,
    pub params: Value,
}

#[doc(hidden)]
pub const MAIL_RPC_ENDPOINT: &str = crate::internal::email_wire::MAIL_RPC_ENDPOINT;

#[doc(hidden)]
pub fn build_inbox_rpc_call(query: crate::email::EmailInboxQuery) -> EmailRpcCall {
    crate::internal::email_wire::build_inbox_rpc_call(query).into()
}

#[doc(hidden)]
pub fn build_read_rpc_call(id: crate::email::EmailMessageId) -> EmailRpcCall {
    crate::internal::email_wire::build_read_rpc_call(&id).into()
}

#[doc(hidden)]
pub fn build_mark_read_rpc_call(
    request: crate::email::EmailMarkReadRequest,
) -> crate::ImResult<EmailRpcCall> {
    crate::internal::email_wire::build_mark_read_rpc_call(request).map(Into::into)
}

#[doc(hidden)]
pub fn build_account_rpc_call() -> EmailRpcCall {
    crate::internal::email_wire::build_account_rpc_call().into()
}

#[doc(hidden)]
pub fn build_send_rpc_call(
    request: crate::email::SendEmailRequest,
) -> crate::ImResult<EmailRpcCall> {
    crate::internal::email_wire::build_send_rpc_call(request).map(Into::into)
}

#[doc(hidden)]
pub fn build_attachment_rpc_call(
    request: crate::email::EmailAttachmentDownloadRequest,
) -> EmailRpcCall {
    crate::internal::email_wire::build_attachment_rpc_call(&request).into()
}

#[doc(hidden)]
pub fn normalize_inbox(
    value: Value,
) -> crate::ImResult<crate::ids::Page<crate::email::EmailMessageSummary>> {
    crate::internal::email_wire::normalize::inbox(value)
}

impl From<crate::internal::email_wire::EmailRpcCall> for EmailRpcCall {
    fn from(value: crate::internal::email_wire::EmailRpcCall) -> Self {
        Self {
            endpoint: value.endpoint,
            method: value.method,
            params: value.params,
        }
    }
}
