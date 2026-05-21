//! Migration-only wire helpers for `awiki-cli` wrappers.

#[doc(hidden)]
pub fn now_rfc3339() -> String {
    crate::internal::wire::common::now_rfc3339()
}

#[doc(hidden)]
pub fn generate_operation_id() -> String {
    crate::internal::wire::common::generate_operation_id()
}

#[doc(hidden)]
pub fn content_type_for_message_kind(
    kind: crate::messages::MessageKind,
    message_type: Option<&str>,
) -> &'static str {
    crate::internal::wire::common::content_type_for_message_kind(kind, message_type)
}
