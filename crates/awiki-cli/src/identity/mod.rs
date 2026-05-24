pub mod client;
pub mod did;
pub mod handle_input;
pub mod key_compat;
pub mod layout;
pub mod legacy;
pub mod recover;
pub mod replace_did;
pub mod service;
pub mod store;
pub mod types;
pub mod wire;

pub use did::{
    build_agent_anp_message_service, default_anp_service_did, default_anp_service_endpoint,
    did_suffix, generate_identity, generate_identity_with_path_segments, validate_anp_service_did,
    validate_anp_service_endpoint,
};
pub use handle_input::{
    complete_bare_handle, default_string as default_handle_string, derive_full_handle_from_did,
    normalize_handle_input, NormalizedHandle,
};
pub use key_compat::ensure_key1_private_pem_compatible;
pub use layout::Manager;
pub use recover::{
    finalize_recovered_handle, recover_identity_ignored_warning, recover_preview,
    RecoverFinalizeError, RecoverFinalizeRequest, RecoverPlan,
};
pub use replace_did::{replace_did, ReplaceDidBackupManifest, ReplaceDidBackupResult};
pub use service::{
    bind, bind_plan, create_identity, create_migration_identity, current_identity, get_profile,
    import_v1, import_v1_migration, list_identities, recover, refresh_token, refresh_token_plan,
    register, register_plan, replace_did_danger_warning, replace_did_plan, resolve_identity,
    sanitize_public_value, set_profile, status as identity_status, switch_default_identity,
    use_plan, CommandResult, GetProfileParams, ResolveParams, SetProfileParams,
};
pub use store::choose_default_identity_name;
pub use types::{
    BindParams, IdentityError, IdentitySummary, LegacyScan, RecoverParams, RegisterParams,
    ReplaceDidParams, UserState,
};
