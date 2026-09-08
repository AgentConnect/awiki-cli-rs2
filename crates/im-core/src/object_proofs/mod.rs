//! Generic standard JSON Object Proofs using the current device assertion key.
//! Applications own business schema, content review, target trust and authorization.
mod review;
mod service;
#[cfg(test)]
mod tests;
pub use review::ObjectProofReview;
pub use service::{ObjectProofCapability, ObjectProofService};
