//! Stale Direct delivery recovery for canonical Handle conversations.

#[derive(Debug, Clone)]
pub(crate) struct DirectRebindRequest {
    pub(crate) conversation_id: String,
    pub(crate) stale_did: Option<String>,
    pub(crate) known_handle: Option<String>,
    pub(crate) peer_scope: Option<crate::internal::local_state::owner_scope::DirectPeerScope>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DirectRebindResult {
    pub(crate) target_did: String,
    pub(crate) full_handle: String,
    pub(crate) peer_scope: crate::internal::local_state::owner_scope::DirectPeerScope,
}

pub(crate) async fn rebind_for_stale_error(
    client: &crate::core::ImClient,
    request: DirectRebindRequest,
    error: &crate::ImError,
) -> crate::ImResult<Option<DirectRebindResult>> {
    let Some(context) = rebind_context(client, &request, error)? else {
        return Ok(None);
    };

    let lock = client
        .core_inner()
        .direct_rebind_lock(&context.owner.owner_identity_id, &request.conversation_id);
    let _guard = lock.lock().await;

    if let Some(route) = current_route(
        client,
        &context.owner.owner_identity_id,
        &request.conversation_id,
    )? {
        if request
            .stale_did
            .as_deref()
            .is_some_and(|stale_did| route.current_did != stale_did)
        {
            return validate_route_result(
                client.did().as_str(),
                &request,
                &context.known_handle,
                route,
            )
            .map(Some);
        }
    }

    let lookup = crate::internal::handle_discovery::resolve_authoritative_direct_rebind_async(
        client,
        &context.known_handle,
    )
    .await?;
    let result = validate_authoritative_result(
        client.did().as_str(),
        &request,
        &context.known_handle,
        &lookup,
    )?;
    let projected_conversation_id = client
        .core_inner()
        .local_state_db()
        .await?
        .project_verified_handle(
            &context.owner.owner_identity_id,
            &context.owner.owner_did,
            lookup.clone(),
        )
        .await?;
    if projected_conversation_id != request.conversation_id {
        return Err(binding_conflict(
            "authoritative Handle projection changed the canonical Direct conversation",
        ));
    }
    Ok(Some(result))
}

pub(crate) fn rebind_for_stale_error_blocking(
    client: &crate::core::ImClient,
    request: DirectRebindRequest,
    error: &crate::ImError,
) -> crate::ImResult<Option<DirectRebindResult>> {
    let Some(context) = rebind_context(client, &request, error)? else {
        return Ok(None);
    };
    let lock = client
        .core_inner()
        .direct_rebind_lock(&context.owner.owner_identity_id, &request.conversation_id);
    let _guard = lock.blocking_lock();

    if let Some(route) = current_route(
        client,
        &context.owner.owner_identity_id,
        &request.conversation_id,
    )? {
        if request
            .stale_did
            .as_deref()
            .is_some_and(|stale_did| route.current_did != stale_did)
        {
            return validate_route_result(
                client.did().as_str(),
                &request,
                &context.known_handle,
                route,
            )
            .map(Some);
        }
    }

    let lookup = crate::internal::handle_discovery::resolve_authoritative_direct_rebind(
        client,
        &context.known_handle,
    )?;
    let result = validate_authoritative_result(
        client.did().as_str(),
        &request,
        &context.known_handle,
        &lookup,
    )?;
    let mut connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    let projected_conversation_id =
        crate::internal::local_state::peer_personas::project_verified_handle(
            &mut connection,
            &context.owner.owner_identity_id,
            &context.owner.owner_did,
            &lookup,
        )?;
    if projected_conversation_id != request.conversation_id {
        return Err(binding_conflict(
            "authoritative Handle projection changed the canonical Direct conversation",
        ));
    }
    Ok(Some(result))
}

struct DirectRebindContext {
    owner: crate::internal::local_state::owner_scope::OwnerScope,
    known_handle: String,
}

fn rebind_context(
    client: &crate::core::ImClient,
    request: &DirectRebindRequest,
    error: &crate::ImError,
) -> crate::ImResult<Option<DirectRebindContext>> {
    let Some(hint) = crate::internal::service_error::stale_target_binding_from_error(
        error,
        client.did().as_str(),
    ) else {
        return Ok(None);
    };
    let owner = crate::internal::local_state::owner_scope::OwnerScope::for_client(client)?;
    let known_handle = request
        .known_handle
        .as_deref()
        .or_else(|| {
            request
                .peer_scope
                .as_ref()
                .map(|scope| scope.full_handle.as_str())
        })
        .or(hint.full_handle.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    Ok(known_handle.map(|known_handle| DirectRebindContext {
        owner,
        known_handle,
    }))
}

fn validate_authoritative_result(
    owner_did: &str,
    request: &DirectRebindRequest,
    known_handle: &str,
    lookup: &crate::directory::HandleLookupResult,
) -> crate::ImResult<DirectRebindResult> {
    let generation =
        lookup
            .binding_generation
            .as_deref()
            .ok_or_else(|| crate::ImError::IdentityUnresolved {
                detail: "authoritative Handle lookup omitted binding_generation".to_owned(),
            })?;
    crate::internal::local_state::sync_v2::validate_positive_decimal(
        "binding_generation",
        generation,
    )?;
    let persona = lookup.peer_persona()?;
    let expected_scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
        persona.authority_subject_id.clone(),
        persona.full_handle.clone(),
    )?;
    let known_scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
        expected_scope.user_id.clone(),
        known_handle,
    )?;
    if known_scope.full_handle != expected_scope.full_handle {
        return Err(binding_conflict(
            "authoritative Handle lookup returned a different Handle",
        ));
    }
    if request
        .peer_scope
        .as_ref()
        .is_some_and(|scope| scope != &expected_scope)
    {
        return Err(binding_conflict(
            "authoritative Handle lookup changed the immutable peer scope",
        ));
    }
    if persona.direct_conversation_id() != request.conversation_id {
        return Err(binding_conflict(
            "authoritative Handle lookup changed the canonical Direct conversation",
        ));
    }
    let target_did = canonical_target_did(owner_did, lookup.did.as_str())?;
    let Some(stale_did) = request.stale_did.as_deref() else {
        return Err(binding_conflict(
            "stale Direct delivery did not retain its original target DID",
        ));
    };
    if target_did == stale_did.trim() {
        return Err(binding_conflict(
            "authoritative Handle lookup still resolves to the stale target DID",
        ));
    }
    Ok(DirectRebindResult {
        target_did,
        full_handle: persona.full_handle,
        peer_scope: expected_scope,
    })
}

fn validate_route_result(
    owner_did: &str,
    request: &DirectRebindRequest,
    known_handle: &str,
    route: crate::internal::local_state::direct_peer_routes::DirectPeerRouteRecord,
) -> crate::ImResult<DirectRebindResult> {
    let peer_scope = route.peer_scope();
    let known_scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
        peer_scope.user_id.clone(),
        known_handle,
    )?;
    if known_scope.full_handle != peer_scope.full_handle
        || request
            .peer_scope
            .as_ref()
            .is_some_and(|scope| scope != &peer_scope)
    {
        return Err(binding_conflict(
            "refreshed Direct route changed the immutable peer scope",
        ));
    }
    Ok(DirectRebindResult {
        target_did: canonical_target_did(owner_did, &route.current_did)?,
        full_handle: route.full_handle,
        peer_scope,
    })
}

#[cfg(feature = "sqlite")]
fn current_route(
    client: &crate::core::ImClient,
    owner_identity_id: &str,
    conversation_id: &str,
) -> crate::ImResult<Option<crate::internal::local_state::direct_peer_routes::DirectPeerRouteRecord>>
{
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    crate::internal::local_state::direct_peer_routes::get(
        &connection,
        owner_identity_id,
        conversation_id,
    )
}

#[cfg(not(feature = "sqlite"))]
fn current_route(
    _client: &crate::core::ImClient,
    _owner_identity_id: &str,
    _conversation_id: &str,
) -> crate::ImResult<Option<crate::internal::local_state::direct_peer_routes::DirectPeerRouteRecord>>
{
    Ok(None)
}

fn canonical_target_did(owner_did: &str, value: &str) -> crate::ImResult<String> {
    let did = crate::ids::Did::parse(value.trim())?;
    if did.as_str() == owner_did.trim() {
        return Err(binding_conflict(
            "authoritative Direct route resolved to the owner DID",
        ));
    }
    Ok(did.as_str().to_owned())
}

fn binding_conflict(detail: &str) -> crate::ImError {
    crate::ImError::IdentityBindingConflict {
        detail: detail.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use crate::internal::local_state::owner_scope::DirectPeerScope;

    const OWNER_DID: &str = "did:wba:awiki.test:user:alice:e1";
    const OLD_DID: &str = "did:wba:awiki.test:user:bob:e1-old";
    const NEW_DID: &str = "did:wba:awiki.test:user:bob:e1-new";

    fn request() -> super::DirectRebindRequest {
        let peer_scope = DirectPeerScope::new("user-bob", "bob.awiki.test").unwrap();
        super::DirectRebindRequest {
            conversation_id:
                crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(
                    &peer_scope,
                ),
            stale_did: Some(OLD_DID.to_owned()),
            known_handle: Some("bob.awiki.test".to_owned()),
            peer_scope: Some(peer_scope),
        }
    }

    fn lookup(
        user_id: &str,
        handle: &str,
        did: &str,
        generation: Option<&str>,
    ) -> crate::directory::HandleLookupResult {
        crate::directory::HandleLookupResult {
            handle: crate::ids::Handle::parse(handle, "").unwrap(),
            did: crate::ids::Did::parse(did).unwrap(),
            user_id: user_id.to_owned(),
            domain: Some("awiki.test".to_owned()),
            status: Some("active".to_owned()),
            binding_generation: generation.map(ToOwned::to_owned),
            profile: None,
            warnings: Vec::new(),
        }
    }

    #[test]
    fn authoritative_result_preserves_canonical_scope_and_uses_authority_did() {
        let result = super::validate_authoritative_result(
            OWNER_DID,
            &request(),
            "bob.awiki.test",
            &lookup("user-bob", "bob.awiki.test", NEW_DID, Some("2")),
        )
        .unwrap();

        assert_eq!(result.target_did, NEW_DID);
        assert_eq!(result.full_handle, "bob.awiki.test");
        assert_eq!(result.peer_scope.user_id, "user-bob");
    }

    #[test]
    fn authoritative_result_fails_closed_on_identity_or_conversation_change() {
        let base = request();
        let mut changed_conversation = base.clone();
        changed_conversation.conversation_id = "dm:peer-scope:v1:other".to_owned();
        let mut changed_scope = base.clone();
        changed_scope.peer_scope =
            Some(DirectPeerScope::new("another-user", "bob.awiki.test").unwrap());

        for (request, lookup) in [
            (
                base.clone(),
                lookup("user-bob", "mallory.awiki.test", NEW_DID, Some("2")),
            ),
            (
                base.clone(),
                lookup("another-user", "bob.awiki.test", NEW_DID, Some("2")),
            ),
            (
                changed_scope,
                lookup("user-bob", "bob.awiki.test", NEW_DID, Some("2")),
            ),
            (
                changed_conversation,
                lookup("user-bob", "bob.awiki.test", NEW_DID, Some("2")),
            ),
        ] {
            assert!(matches!(
                super::validate_authoritative_result(
                    OWNER_DID,
                    &request,
                    "bob.awiki.test",
                    &lookup,
                ),
                Err(crate::ImError::IdentityBindingConflict { .. })
            ));
        }
    }

    #[test]
    fn authoritative_result_requires_new_non_owner_did_and_valid_generation() {
        for lookup in [
            lookup("user-bob", "bob.awiki.test", NEW_DID, None),
            lookup("user-bob", "bob.awiki.test", NEW_DID, Some("0")),
            lookup("user-bob", "bob.awiki.test", OLD_DID, Some("2")),
            lookup("user-bob", "bob.awiki.test", OWNER_DID, Some("2")),
        ] {
            assert!(super::validate_authoritative_result(
                OWNER_DID,
                &request(),
                "bob.awiki.test",
                &lookup,
            )
            .is_err());
        }
    }

    #[test]
    fn refreshed_route_can_be_reused_after_another_sender_updates_it() {
        let request = request();
        let route = crate::internal::local_state::direct_peer_routes::DirectPeerRouteRecord::new(
            "owner-alice",
            request.conversation_id.clone(),
            "user-bob",
            "bob.awiki.test",
            NEW_DID,
        )
        .unwrap();

        let result =
            super::validate_route_result(OWNER_DID, &request, "bob.awiki.test", route).unwrap();
        assert_eq!(result.target_did, NEW_DID);
    }
}
