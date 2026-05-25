pub mod anpsdk;
pub mod app;
pub mod authsdk;
pub mod buildinfo;
pub mod cli;
pub mod cmdmeta;
pub mod config;
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
#[doc(hidden)]
#[allow(dead_code)]
pub mod runtime_legacy;
pub mod store;
pub mod traceutil;
pub mod transportcfg;
pub mod update;
pub mod upgrade;

pub use app::execute;
