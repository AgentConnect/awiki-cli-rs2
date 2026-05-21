pub mod auth;
pub mod config;
pub mod core;
pub mod directory;
pub mod error;
pub mod identity;
pub mod ids;
pub mod messages;
pub mod paths;
pub mod prelude;

#[doc(hidden)]
pub mod compat;

mod internal;

pub use self::config::{ImCoreConfig, MessageTransportPolicy, ServiceEndpoint};
pub use crate::core::{CoreBootstrap, ImClient, ImCore};
pub use crate::directory::{DirectoryService, HandleLookupResult};
pub use crate::error::{ImError, ImResult};
pub use crate::identity::{IdentitySelector, IdentitySummary};
pub use crate::paths::{IdentityRegistryPaths, ImCorePaths, LocalStatePaths, RuntimePaths};
