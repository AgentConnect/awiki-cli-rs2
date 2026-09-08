//! Closed business signing facade for `awiki-information-publish-v1`.
//! The host owns trusted Node selection and human review; Core owns all keys.
mod review;
mod service;
#[cfg(test)]
mod tests;
pub use review::InformationPublicationReview;
pub use service::{InformationPublicationCapability, InformationPublicationService};
