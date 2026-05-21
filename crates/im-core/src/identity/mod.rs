mod dto;
mod profile;
mod registry;
mod service;

pub use self::dto::{
    ContactBindingRequest, ContactBindingResult, DefaultIdentityChange, HandleRegistrationResult,
    IdentityMissingItem, IdentityReadiness, IdentitySelector, IdentitySummary, InitialProfile,
    Profile, ProfileAttribute, ProfilePatch, RegisterHandleRequest, VerificationInput,
};
pub use self::registry::IdentityRegistry;
pub use self::service::IdentityService;
