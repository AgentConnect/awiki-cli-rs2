pub mod build_info;
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

pub use cli_shell::execute;
