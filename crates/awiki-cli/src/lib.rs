pub mod anpsdk;
pub mod app;
pub mod authsdk;
pub mod buildinfo;
pub mod cli;
pub mod cmdmeta;
pub mod config;
pub mod content;
pub mod docs;
pub mod doctor;
pub mod durablefs;
pub mod identity;
pub mod im_core_adapter;
// TODO(sdk-refactor-email-e7): delete this legacy module once historical
// parity references have fully moved to `im_core::email`.
#[allow(dead_code)]
pub(crate) mod mail;
pub mod message;
pub mod output;
pub mod runtime;
pub mod site;
pub mod store;
pub mod traceutil;
pub mod transportcfg;
pub mod update;
pub mod upgrade;

pub use app::execute;
