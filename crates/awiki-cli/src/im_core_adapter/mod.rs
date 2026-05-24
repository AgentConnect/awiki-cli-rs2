//! CLI boundary adapter for `im-core`.
//!
//! Cutover policy:
//!
//! - Stable responsibility: CLI config/path/flag/output conversion, `ImError`
//!   mapping, unsupported capability output, realtime host signal conversion,
//!   and high-level event projection owned by the CLI shell.
//! - No new legacy business bridge may be added here. New code should call
//!   `im-core` public APIs and return CLI envelopes at the boundary.
//! - Existing legacy bridge code is temporary migration-only and must be called
//!   out in the owning module with the PR that removes it.
//! - Delete remaining bridge exceptions during the default-path cutover and
//!   cleanup slices: C2, C3, C4, C5, and C7.

pub(crate) mod active_identity;
pub mod auth;
pub mod config;
pub mod core;
pub mod email;
pub mod error;
pub mod groups;
pub mod identity;
pub mod identity_replace_did_plan;
pub mod message_result;
pub mod messages;
pub mod paths;
pub mod people;
pub mod realtime;
pub mod render;
pub mod unsupported;

pub use auth::auth_scope_from_cli;
pub use config::build_im_core_config;
pub use core::{build_im_client, build_im_core};
pub use error::map_im_error;
pub use identity::{cli_identity_selector, register_handle_request};
pub use paths::build_im_core_paths;
pub use render::success_envelope_for_sdk_value;
pub use unsupported::unsupported_cutover_command;

#[cfg(test)]
mod tests;
