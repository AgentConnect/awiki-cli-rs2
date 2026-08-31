//! Exact durable continuation routing for registration-triggered Recovery Join.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JoinedRegistrationResumeEvidence {
    pub(crate) join_session_id: String,
    pub(crate) owner_identity_id: String,
    pub(crate) remote_create_state: crate::internal::identity_device_join::RemoteCreateState,
    pub(crate) reason: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RegistrationJoinContinuation {
    None,
    Resume(JoinedRegistrationResumeEvidence),
    FinalizeIdentitySwitched {
        join_session_id: String,
        owner_identity_id: String,
    },
    TerminalCleanupThenRetry {
        join_session_id: String,
        reason: &'static str,
    },
    WaitForLegacyDeadline,
    Conflict,
}

pub(crate) fn resolve(
    core: &crate::core::ImCore,
    transition: &crate::internal::identity_registration_join_preparation::RegistrationJoinTransition,
    full_handle: &crate::ids::Handle,
) -> crate::ImResult<RegistrationJoinContinuation> {
    let sqlite_path = &core.inner().sdk_paths().local_state.sqlite_path;
    let markers =
        crate::internal::identity_transition_pending::load_active_joined_devices(sqlite_path)?;
    let scoped = markers
        .into_iter()
        .filter(|marker| {
            (marker.account_user_id == transition.account_user_id
                && marker.handle == full_handle.as_str())
                || marker.previous_did == transition.previous_did
                || marker.current_did == transition.current_did
        })
        .collect::<Vec<_>>();
    if scoped.is_empty() {
        return Ok(RegistrationJoinContinuation::None);
    }
    let [marker] = scoped.as_slice() else {
        return Ok(RegistrationJoinContinuation::Conflict);
    };
    if marker.account_user_id != transition.account_user_id
        || marker.handle != full_handle.as_str()
        || marker.previous_did != transition.previous_did
        || marker.current_did != transition.current_did
        || marker.binding_generation != transition.binding_generation
        || marker.source_kind
            != crate::internal::identity_transition_pending::TransitionSourceKind::JoinedDevice
        || marker.source_id.trim().is_empty()
    {
        return Ok(RegistrationJoinContinuation::Conflict);
    }
    let Some(session) = crate::internal::identity_device_join::registration_join_session_evidence(
        core,
        &marker.source_id,
    )?
    else {
        return Ok(RegistrationJoinContinuation::Conflict);
    };
    if session.join_session_id != marker.source_id
        || session.did != marker.current_did
        || session.device_id.trim().is_empty()
    {
        return Ok(RegistrationJoinContinuation::Conflict);
    }
    let index_relation = identity_index_relation(core, marker)?;
    if marker.phase
        == crate::internal::identity_transition_pending::TransitionPhase::IdentitySwitched
    {
        if (session.phase != crate::identity::DeviceJoinLocalPhase::Authorized
            && !session.activation_pending)
            || index_relation != MarkerIndexRelation::Current
        {
            return Ok(RegistrationJoinContinuation::Conflict);
        }
        return Ok(RegistrationJoinContinuation::FinalizeIdentitySwitched {
            join_session_id: marker.source_id.clone(),
            owner_identity_id: marker.owner_identity_id.clone(),
        });
    }
    if marker.phase != crate::internal::identity_transition_pending::TransitionPhase::Pending {
        return Ok(RegistrationJoinContinuation::Conflict);
    }
    if index_relation == MarkerIndexRelation::Conflict
        || (index_relation == MarkerIndexRelation::Current
            && !session.activation_pending
            && session.phase != crate::identity::DeviceJoinLocalPhase::Authorized)
    {
        return Ok(RegistrationJoinContinuation::Conflict);
    }
    if matches!(
        session.phase,
        crate::identity::DeviceJoinLocalPhase::Cancelled
            | crate::identity::DeviceJoinLocalPhase::Expired
    ) {
        return Ok(
            if session.terminal_evidence
                == Some(
                    crate::internal::identity_device_join::JoinTerminalEvidence::LegacyUnverified,
                )
                && !session.deadline_elapsed
            {
                RegistrationJoinContinuation::WaitForLegacyDeadline
            } else {
                RegistrationJoinContinuation::TerminalCleanupThenRetry {
                    join_session_id: marker.source_id.clone(),
                    reason: if session.deadline_elapsed {
                        "cleanup_terminal_deadline"
                    } else {
                        "cleanup_terminal"
                    },
                }
            },
        );
    }
    match session.remote_create_state {
        crate::internal::identity_device_join::RemoteCreateState::LocalOnly => {
            Ok(RegistrationJoinContinuation::TerminalCleanupThenRetry {
                join_session_id: marker.source_id.clone(),
                reason: "cleanup_local_only",
            })
        }
        state @ (crate::internal::identity_device_join::RemoteCreateState::Attempting
        | crate::internal::identity_device_join::RemoteCreateState::UnknownLegacy
        | crate::internal::identity_device_join::RemoteCreateState::Bound) => Ok(
            RegistrationJoinContinuation::Resume(JoinedRegistrationResumeEvidence {
                join_session_id: marker.source_id.clone(),
                owner_identity_id: marker.owner_identity_id.clone(),
                remote_create_state: state,
                reason: if state == crate::internal::identity_device_join::RemoteCreateState::Bound
                {
                    "resume_bound"
                } else {
                    "resume_attempting"
                },
            }),
        ),
    }
}

pub(crate) fn recover_all(core: &crate::core::ImCore) -> crate::ImResult<()> {
    let sqlite_path = &core.inner().sdk_paths().local_state.sqlite_path;
    for marker in
        crate::internal::identity_transition_pending::load_active_joined_devices(sqlite_path)?
    {
        let Some(session) =
            crate::internal::identity_device_join::registration_join_session_evidence(
                core,
                &marker.source_id,
            )?
        else {
            return Err(crate::ImError::PermissionDenied);
        };
        if session.join_session_id != marker.source_id
            || session.did != marker.current_did
            || session.device_id.trim().is_empty()
        {
            return Err(crate::ImError::PermissionDenied);
        }
        let index_relation = identity_index_relation(core, &marker)?;
        match marker.phase {
            crate::internal::identity_transition_pending::TransitionPhase::IdentitySwitched => {
                if (session.phase != crate::identity::DeviceJoinLocalPhase::Authorized
                    && !session.activation_pending)
                    || index_relation != MarkerIndexRelation::Current
                {
                    return Err(crate::ImError::PermissionDenied);
                }
                crate::internal::identity_handle_recovery_runtime::mark_joined_transition_applied(
                    core, &marker,
                )?;
            }
            crate::internal::identity_transition_pending::TransitionPhase::Pending => {
                if index_relation == MarkerIndexRelation::Conflict
                    || (index_relation == MarkerIndexRelation::Current
                        && !session.activation_pending
                        && session.phase != crate::identity::DeviceJoinLocalPhase::Authorized)
                {
                    return Err(crate::ImError::PermissionDenied);
                }
                if index_relation == MarkerIndexRelation::Current {
                    continue;
                }
                if matches!(
                    session.phase,
                    crate::identity::DeviceJoinLocalPhase::Cancelled
                        | crate::identity::DeviceJoinLocalPhase::Expired
                ) {
                    if session.terminal_evidence
                        == Some(
                            crate::internal::identity_device_join::JoinTerminalEvidence::LegacyUnverified,
                        ) && !session.deadline_elapsed
                    {
                        continue;
                    }
                    crate::internal::identity_device_join::cleanup_terminal_registration_join_sync(
                        core,
                        &marker.source_id,
                    )?;
                    continue;
                }
                match session.remote_create_state {
                    crate::internal::identity_device_join::RemoteCreateState::LocalOnly => {
                        crate::internal::identity_device_join::abort_local_only_registration_join_sync(
                            core,
                            &marker.source_id,
                        )?;
                    }
                    crate::internal::identity_device_join::RemoteCreateState::Attempting
                    | crate::internal::identity_device_join::RemoteCreateState::UnknownLegacy
                        if session.deadline_elapsed =>
                    {
                        crate::internal::identity_device_join::expire_registration_join_at_deadline_sync(
                            core,
                            &marker.source_id,
                        )?;
                    }
                    crate::internal::identity_device_join::RemoteCreateState::Attempting
                    | crate::internal::identity_device_join::RemoteCreateState::UnknownLegacy
                    | crate::internal::identity_device_join::RemoteCreateState::Bound => {}
                }
            }
            crate::internal::identity_transition_pending::TransitionPhase::Completed => {
                return Err(crate::ImError::PermissionDenied)
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkerIndexRelation {
    Previous,
    Current,
    Conflict,
}

fn identity_index_relation(
    core: &crate::core::ImCore,
    marker: &crate::internal::identity_transition_pending::IdentityTransitionMarker,
) -> crate::ImResult<MarkerIndexRelation> {
    let index =
        crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities)
            .load_index()?;
    let matches = index
        .credentials
        .values()
        .filter(|entry| entry.unique_id == marker.owner_identity_id)
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Ok(MarkerIndexRelation::Conflict);
    }
    let entry = matches[0];
    if entry.user_id != marker.account_user_id || entry.full_handle != marker.handle {
        return Ok(MarkerIndexRelation::Conflict);
    }
    if entry.did == marker.previous_did {
        return Ok(MarkerIndexRelation::Previous);
    }
    if entry.did == marker.current_did
        && entry.binding_generation.as_deref() == Some(marker.binding_generation.as_str())
    {
        return Ok(MarkerIndexRelation::Current);
    }
    Ok(MarkerIndexRelation::Conflict)
}

#[cfg(test)]
mod tests;
