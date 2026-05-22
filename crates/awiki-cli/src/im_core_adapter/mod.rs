pub mod auth;
pub mod config;
pub mod core;
pub mod error;
pub mod feature_flag;
pub mod groups;
pub mod identity;
pub mod identity_replace_did_plan;
pub mod messages;
pub mod paths;
pub mod realtime;
pub mod render;

pub use auth::auth_scope_from_cli;
pub use config::build_im_core_config;
pub use core::{build_im_client, build_im_core};
pub use error::map_im_error;
pub use feature_flag::use_im_core_mvp;
pub use identity::{cli_identity_selector, register_handle_request};
pub use paths::build_im_core_paths;
pub use render::success_envelope_for_sdk_value;

#[cfg(test)]
mod tests;
