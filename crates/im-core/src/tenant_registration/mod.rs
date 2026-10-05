//! Domain registration administration through the existing authenticated User Service.

mod dto;
mod service;

pub use dto::{
    AllowlistContact, AllowlistEntry, AllowlistPage, ConfigureRegistration, ContactType,
    ListRegistrationEntries, RegistrationMode, RegistrationOperation, RegistrationPolicy,
    RegistrationPolicySource,
};
pub use service::TenantRegistrationService;

#[cfg(test)]
mod tests;
