mod dto;
mod profile;
mod registry;
mod service;

pub use self::dto::{
    ContactBindingMethod, ContactBindingMethodKind, ContactBindingRequest, ContactBindingResult,
    ContactBindingState, DefaultIdentityChange, HandleRegistrationResult, IdentityMissingItem,
    IdentityReadiness, IdentitySelector, IdentitySummary, InitialProfile, Profile,
    ProfileAttribute, ProfilePatch, RecoverGeneratedIdentity, RecoverHandleRequest,
    RecoverHandleResult, RecoverHandleState, RecoveredIdentity, RegisterHandleRequest,
    VerificationInput,
};
pub use self::registry::IdentityRegistry;
pub use self::service::IdentityService;
