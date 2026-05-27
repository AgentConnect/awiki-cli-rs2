#![allow(dead_code)]
#![allow(clippy::large_enum_variant)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]

pub mod attachments;
pub mod auth;
pub mod config;
pub mod content;
pub mod core;
pub mod directory;
pub mod email;
pub mod error;
pub mod groups;
pub mod identity;
pub mod ids;
pub mod messages;
pub mod paths;
pub mod prelude;
pub mod realtime;
pub mod secure;
pub mod site;

#[doc(hidden)]
pub mod compat;

mod internal;

pub use self::config::{ImCoreConfig, MessageTransportPolicy, ServiceEndpoint};
pub use crate::attachments::AttachmentService;
pub use crate::content::ContentService;
pub use crate::core::{CoreBootstrap, ImClient, ImCore};
pub use crate::directory::{DirectoryService, HandleLookupResult};
pub use crate::email::EmailService;
pub use crate::error::{ImError, ImResult};
pub use crate::groups::GroupService;
pub use crate::identity::{IdentitySelector, IdentitySummary};
pub use crate::paths::{IdentityRegistryPaths, ImCorePaths, LocalStatePaths, RuntimePaths};
pub use crate::realtime::RealtimeService;
pub use crate::secure::SecureService;
pub use crate::site::SiteService;
