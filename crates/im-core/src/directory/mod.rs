mod dto;
mod service;

pub use self::dto::{
    Contact, ContactListQuery, DirectoryResolution, HandleLookupResult, RelationStatus,
    SaveContactRequest,
};
pub use self::service::DirectoryService;
