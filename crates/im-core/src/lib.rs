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
pub mod external_http_auth;
pub mod groups;
pub mod identity;
pub mod ids;
#[cfg(feature = "sqlite")]
pub mod local_state_upgrade;
pub mod messages;
pub mod onboarding;
pub mod paths;
pub mod prelude;
pub mod realtime;
pub mod secure;
pub mod site;
pub mod system_notifications;
pub mod vault;

#[doc(hidden)]
pub mod compat;

mod internal;

pub use self::config::{
    ClientVersionInfo, ImCoreConfig, MessageTransportPolicy, ServiceEndpoint, CLIENT_VERSION_HEADER,
};
pub use crate::attachments::AttachmentService;
pub use crate::content::ContentService;
pub use crate::core::{
    CoreBootstrap, IdentitySecretStoragePolicy, ImClient, ImCore, ImCoreOpenOptions,
    ImCoreSecretVaultOptions,
};
pub use crate::directory::{DirectoryService, HandleLookupResult};
pub use crate::email::EmailService;
pub use crate::error::{
    AttachmentTransferFailure, DeviceRevokeOutcomeCategory, IdentityVaultFailure, ImError, ImResult,
};
pub use crate::external_http_auth::{
    ExternalHttpAuthAttempt, ExternalHttpAuthDecision, ExternalHttpAuthService, ExternalHttpHeader,
    ExternalHttpRequest, ExternalHttpResponse, EXTERNAL_HTTP_AUTH_MAX_BODY_BYTES,
};
pub use crate::groups::GroupService;
pub use crate::identity::{
    ActiveSyncAccountBinding, AgentIdentityKind, DeleteLocalIdentityResult,
    HostBackedAuthTokenPersistence, HostBackedDeviceIdentityMaterial, HostedIdentityMaterial,
    IdentityDeviceAuthorizationStatus, IdentityDeviceRole, IdentitySecretStorageBackend,
    IdentitySelector, IdentitySummary, IdentityVaultMigrationReport, IdentityVaultStatus,
    IdentityVaultVerificationReport, LegacyRegistryEpochAdoptionAuthority,
    VNextAgentBootstrapMaterial, VNextAgentLegacyUpgradeReconciliation,
    VNextAgentLegacyUpgradeSession,
};
#[cfg(feature = "sqlite")]
pub use crate::local_state_upgrade::{
    inspect_local_state_upgrade, restore_local_state_backup, upgrade_local_state,
    LocalStateConversationAliasMapping, LocalStateRestoreResult, LocalStateUpgradeEligibility,
    LocalStateUpgradeInspection, LocalStateUpgradeResult, LocalStateUpgradeStatus,
};
pub use crate::messages::{
    IncomingMessageRecoveryItem, IncomingMessageRecoveryPage, IncomingMessageRecoveryPageToken,
    IncomingMessageRecoveryQuery,
};
pub use crate::onboarding::{
    SkillClaimPhase, SkillClaimRequest, SkillClaimResult, SkillClaimStatus, SkillOnboardingService,
    SkillOnboardingToken, SkillResumeRequest,
};
pub use crate::paths::{IdentityRegistryPaths, ImCorePaths, LocalStatePaths, RuntimePaths};
pub use crate::realtime::RealtimeService;
pub use crate::secure::SecureService;
pub use crate::site::SiteService;
pub use crate::system_notifications::SystemNotificationService;
