#![allow(dead_code)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_str_replace)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::filter_map_bool_then)]
#![allow(clippy::filter_next)]
#![allow(clippy::io_other_error)]
#![allow(clippy::large_enum_variant)]
#![allow(clippy::let_and_return)]
#![allow(clippy::manual_contains)]
#![allow(clippy::manual_pattern_char_comparison)]
#![allow(clippy::manual_strip)]
#![allow(clippy::map_identity)]
#![allow(clippy::misnamed_getters)]
#![allow(clippy::missing_const_for_thread_local)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::needless_question_mark)]
#![allow(clippy::needless_return)]
#![allow(clippy::ptr_arg)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::while_let_on_iterator)]

pub mod build_info;
mod builtin_tenants;
pub mod cli_completion;
pub mod cli_docs;
#[doc(hidden)]
pub mod cli_http;
pub mod cli_output;
pub mod cli_parser;
pub mod cli_shell;
pub mod cli_trace;
pub mod command_catalog;
pub mod diagnostics;
pub mod durable_fs;
pub mod host_runtime;
pub mod m_core_cli_adapter;
pub mod self_update;
pub mod workspace_config;
pub mod workspace_upgrade;

pub use cli_shell::{execute, execute_async};
