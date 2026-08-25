//! Closed parsing and proof-first primitives for did:wba transition retries.

use std::collections::HashMap;

use anp::authentication::{
    DidDocumentFetcher, DidTransitionError, TransitionAssurance, TransitionErrorKind,
    TransitionResult, ANP_DID_SUPERSEDED, ANP_DID_TRANSITION_CONFLICT, ANP_DID_TRANSITION_INVALID,
    DEFAULT_MAX_TRANSITION_HOPS,
};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DidTransitionReason {
    UnsupportedProfile,
    InvalidDocument,
    InvalidProof,
    RecoveryNotPreauthorized,
    InvalidProviderAssertion,
    StablePathMismatch,
    DirectSuccessorRequired,
    Cycle,
    Conflict,
    MaxHopsExceeded,
    NetworkError,
}

impl DidTransitionReason {
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "transition_profile_not_supported" | "unsupported_profile" => Self::UnsupportedProfile,
            "invalid_document" => Self::InvalidDocument,
            "invalid_proof" => Self::InvalidProof,
            "recovery_not_preauthorized" => Self::RecoveryNotPreauthorized,
            "invalid_provider_assertion" => Self::InvalidProviderAssertion,
            "stable_path_mismatch" => Self::StablePathMismatch,
            "direct_successor_required" => Self::DirectSuccessorRequired,
            "cycle" => Self::Cycle,
            "conflict" => Self::Conflict,
            "max_hops_exceeded" => Self::MaxHopsExceeded,
            "network_error" => Self::NetworkError,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DidTransitionServiceError {
    Superseded {
        requested_did: String,
        current_did: String,
    },
    Invalid {
        reason: DidTransitionReason,
    },
    Conflict {
        reason: DidTransitionReason,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UntrustedDidSupersededHint {
    pub(crate) requested_did: String,
    pub(crate) current_did: String,
}

pub(crate) fn parse_service_error(error: &crate::ImError) -> Option<DidTransitionServiceError> {
    let crate::ImError::Service { code, data, .. } = error else {
        return None;
    };
    let code = numeric_service_code(code.as_deref(), data.as_ref())?;
    let data = data.as_ref()?.as_object()?;
    if !only_fields(data, &["anp_code", "json_rpc_code", "retryable", "details"]) {
        return None;
    }
    let details = data.get("details")?.as_object()?;
    match code {
        ANP_DID_SUPERSEDED => {
            if !only_fields(details, &["requested_did", "current_did"])
                || data.get("retryable").and_then(Value::as_bool) != Some(true)
            {
                return None;
            }
            Some(DidTransitionServiceError::Superseded {
                requested_did: canonical_did(details.get("requested_did")?)?,
                current_did: canonical_did(details.get("current_did")?)?,
            })
        }
        ANP_DID_TRANSITION_INVALID | ANP_DID_TRANSITION_CONFLICT => {
            if !only_fields(details, &["reason"])
                || data.get("retryable").and_then(Value::as_bool) != Some(false)
            {
                return None;
            }
            let reason = DidTransitionReason::parse(details.get("reason")?.as_str()?)?;
            if code == ANP_DID_TRANSITION_INVALID {
                Some(DidTransitionServiceError::Invalid { reason })
            } else {
                Some(DidTransitionServiceError::Conflict { reason })
            }
        }
        _ => None,
    }
}

pub(crate) fn parse_http_409_hint(
    status_code: u16,
    body: &Value,
) -> Option<UntrustedDidSupersededHint> {
    if status_code != 409 {
        return None;
    }
    let object = body.as_object()?;
    if !only_fields(object, &["error", "requestedDid", "currentDid"])
        || object.get("error")?.as_str()? != "did_superseded"
    {
        return None;
    }
    Some(UntrustedDidSupersededHint {
        requested_did: canonical_did(object.get("requestedDid")?)?,
        current_did: canonical_did(object.get("currentDid")?)?,
    })
}

#[cfg(feature = "sqlite")]
pub(crate) fn resolve_and_cache_verified(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    requested_did: &str,
    fetcher: &dyn DidDocumentFetcher,
    trusted_documents: &HashMap<String, Value>,
    provider_fetcher: Option<&dyn DidDocumentFetcher>,
) -> crate::ImResult<TransitionResult> {
    let mut cache =
        crate::internal::local_state::did_transition_edges::VerifiedDidTransitionCache::load(
            connection,
            owner_identity_id,
        )?;
    let result = anp::authentication::resolve_current_did(
        requested_did,
        fetcher,
        trusted_documents,
        provider_fetcher,
        &mut cache,
        DEFAULT_MAX_TRANSITION_HOPS,
    )
    .map_err(map_resolver_error)?;

    for hop in &result.hops {
        if matches!(
            hop.assurance,
            TransitionAssurance::Verified | TransitionAssurance::RecoveryVerified
        ) {
            crate::internal::local_state::did_transition_edges::compare_and_set_verified(
                connection,
                owner_identity_id,
                &crate::internal::local_state::did_transition_edges::VerifiedDidTransitionEdge {
                    predecessor_did: hop.predecessor_did.clone(),
                    successor_did: hop.successor_did.clone(),
                    assurance: hop.assurance,
                },
            )?;
        }
    }
    Ok(result)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DidBoundRetryRequest {
    pub(crate) message_id: String,
    pub(crate) operation_id: String,
    pub(crate) target_did: String,
    pub(crate) payload: Value,
    pub(crate) did_bound_digest: String,
    pub(crate) signature: String,
}

pub(crate) fn rebuild_once_for_verified_successor(
    original: &DidBoundRetryRequest,
    verified_current_did: &str,
    mut sign: impl FnMut(&str, &Value) -> crate::ImResult<(String, String)>,
) -> crate::ImResult<DidBoundRetryRequest> {
    let current_did = crate::ids::Did::parse(verified_current_did.trim())?;
    if current_did.as_str() == original.target_did {
        return Err(crate::ImError::IdentityBindingConflict {
            detail: "DID transition retry target did not advance".to_owned(),
        });
    }
    let (did_bound_digest, signature) = sign(current_did.as_str(), &original.payload)?;
    if did_bound_digest == original.did_bound_digest || signature == original.signature {
        return Err(crate::ImError::IdentityBindingConflict {
            detail: "DID-bound retry material was not rebuilt".to_owned(),
        });
    }
    Ok(DidBoundRetryRequest {
        message_id: original.message_id.clone(),
        operation_id: original.operation_id.clone(),
        target_did: current_did.as_str().to_owned(),
        payload: original.payload.clone(),
        did_bound_digest,
        signature,
    })
}

fn map_resolver_error(error: DidTransitionError) -> crate::ImError {
    let reason = match error.kind {
        TransitionErrorKind::UnsupportedProfile => "transition_profile_not_supported",
        TransitionErrorKind::InvalidDocument => "invalid_document",
        TransitionErrorKind::InvalidProof => "invalid_proof",
        TransitionErrorKind::RecoveryNotPreauthorized => "recovery_not_preauthorized",
        TransitionErrorKind::InvalidProviderAssertion => "invalid_provider_assertion",
        TransitionErrorKind::StablePathMismatch => "stable_path_mismatch",
        TransitionErrorKind::DirectSuccessorRequired => "direct_successor_required",
        TransitionErrorKind::Cycle => "cycle",
        TransitionErrorKind::Conflict => "conflict",
        TransitionErrorKind::MaxHopsExceeded => "max_hops_exceeded",
        TransitionErrorKind::NetworkError => "network_error",
    };
    crate::ImError::Service {
        status_code: None,
        code: Some(error.code.to_string()),
        message: "DID transition verification failed".to_owned(),
        data: Some(serde_json::json!({
            "json_rpc_code": error.code,
            "reason": reason,
        })),
    }
}

fn numeric_service_code(code: Option<&str>, data: Option<&Value>) -> Option<u16> {
    let direct = code?.parse::<u16>().ok();
    let nested = data?
        .get("json_rpc_code")
        .and_then(|value| value.as_u64())
        .and_then(|value| u16::try_from(value).ok());
    match (direct, nested) {
        (Some(left), Some(right)) if left == right => Some(left),
        (Some(value), None) | (None, Some(value)) => Some(value),
        _ => None,
    }
}

fn only_fields(object: &serde_json::Map<String, Value>, allowed: &[&str]) -> bool {
    object.keys().all(|key| allowed.contains(&key.as_str()))
}

fn canonical_did(value: &Value) -> Option<String> {
    let value = value.as_str()?.trim();
    let did = crate::ids::Did::parse(value).ok()?;
    (did.as_str() == value).then(|| value.to_owned())
}

#[cfg(test)]
mod tests;
