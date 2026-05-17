use crate::config::Resolved;
use crate::identity::client::Client;
use crate::identity::types::IdentityError;
use crate::identity::wire::{
    build_handle_lookup_by_did_rpc_call, handle_lookup_error_is_not_found,
    normalize_handle_lookup_result, HandleLookupResult,
};

pub fn lookup_listener_handle_by_did(
    resolved: &Resolved,
    did: &str,
) -> anyhow::Result<Option<String>> {
    let call = build_handle_lookup_by_did_rpc_call(did)?;
    let client = Client::new(resolved)?;
    let lookup: HandleLookupResult =
        match client.rpc_call_profile(call.profile, call.endpoint, call.method, call.params) {
            Ok(lookup) => lookup,
            Err(IdentityError::Service(err)) if handle_lookup_error_is_not_found(&err) => {
                return Ok(None);
            }
            Err(err) => return Err(err.into()),
        };
    let Some(lookup) = normalize_handle_lookup_result(lookup) else {
        return Ok(None);
    };
    Ok(Some(lookup.handle))
}
