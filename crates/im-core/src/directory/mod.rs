mod dto;
mod service;

pub use self::dto::{
    Contact, ContactListQuery, DirectoryResolution, HandleLookupResult, IdentitySubject,
    PublicProfile, RelationStatus, SaveContactRequest,
};
pub use self::service::DirectoryService;
