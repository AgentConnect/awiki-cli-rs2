mod auth;
mod client;
mod did;
mod handle_input;
mod key_compat;
mod layout;
mod legacy;
#[doc(hidden)]
mod legacy_store;
mod service;
pub mod types;
mod wire;

#[cfg(test)]
mod legacy_import_tests;

pub(crate) use layout::Manager;
pub(crate) use legacy_store::choose_default_identity_name;
pub(crate) use service::{create_migration_identity, import_v1_migration, CommandResult};
pub(crate) use types::{IdentityError, ImportResult, INDEX_FILE_NAME};

pub(crate) use key_compat::ensure_all_identity_private_keys_compatible;
