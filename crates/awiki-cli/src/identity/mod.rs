pub mod did;
pub mod handle_input;
pub mod layout;
pub mod legacy;
pub mod service;
pub mod store;
pub mod types;
pub mod wire;

pub use did::{did_suffix, generate_identity};
pub use handle_input::{
    complete_bare_handle, default_string as default_handle_string, derive_full_handle_from_did,
    normalize_handle_input, NormalizedHandle,
};
pub use layout::Manager;
pub use service::{
    create_identity, current_identity, import_v1, list_identities, refresh_token_plan,
    replace_did_plan, sanitize_public_value, status as identity_status, switch_default_identity,
    use_plan, CommandResult,
};
pub use store::choose_default_identity_name;
pub use types::{IdentityError, IdentitySummary, LegacyScan, UserState};
