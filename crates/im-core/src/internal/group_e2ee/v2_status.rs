//! Secret-free local readiness and WAL repair for P6 v2.
//!
//! The ANP SDK owns both OpenMLS private state and its SQLite schema. Core uses
//! only the SDK's typed, secret-free inspection and reconciliation operations;
//! it never reads the SDK tables, exports a Leaf, or copies state to a sibling
//! device.

use anp::group_e2ee::operations::v2::{
    V2InspectLocalGroupInput, V2LocalGroupReadiness, V2ReconcilePendingInput,
};

use super::v2_runtime::GroupE2eeV2Runtime;

pub(crate) struct GroupE2eeV2StatusRuntime {
    runtime: GroupE2eeV2Runtime,
}

impl GroupE2eeV2StatusRuntime {
    pub(crate) fn new(runtime: GroupE2eeV2Runtime) -> Self {
        Self { runtime }
    }

    pub(crate) fn status(
        &self,
        group: crate::ids::GroupRef,
    ) -> crate::ImResult<crate::secure::GroupSecureStatus> {
        require_group_did(group.as_str())?;
        let scope = self.runtime.owner_scope()?;
        let inspected = self.runtime.inspect_local_group(V2InspectLocalGroupInput {
            owner_did: scope.owner_did,
            owner_device_id: scope.device_id,
            group_did: group.as_str().to_owned(),
            request_id: format!(
                "im-core-p6-v2-inspect-{}",
                crate::internal::wire::common::generate_operation_id()
            ),
        })?;
        let pending_commits = inspected
            .auto_reconcile_pending_count
            .checked_add(inspected.host_recheck_pending_count)
            .ok_or_else(|| crate::ImError::LocalStateUnavailable {
                detail: "P6 v2 pending commit count overflow".to_owned(),
            })?;
        let active = inspected.readiness == V2LocalGroupReadiness::Active;

        let (state, problem, warnings) = if inspected.host_recheck_pending_count > 0 {
            (
                crate::secure::GroupSecureState::NeedsRepair,
                Some(crate::secure::SecureProblem {
                    code: crate::secure::SecureProblemCode::SessionNeedsRepair,
                    message: "group MLS commit requires an exact Host acceptance recheck"
                        .to_owned(),
                    retryable: true,
                }),
                vec!["group MLS commit is awaiting a Host decision".to_owned()],
            )
        } else if inspected.auto_reconcile_pending_count > 0 {
            (
                crate::secure::GroupSecureState::Syncing,
                None,
                vec!["group MLS durable local update needs reconciliation".to_owned()],
            )
        } else {
            match inspected.readiness {
                V2LocalGroupReadiness::Active => {
                    (crate::secure::GroupSecureState::Ready, None, Vec::new())
                }
                V2LocalGroupReadiness::Inactive => (
                    crate::secure::GroupSecureState::WaitingForMembershipUpdate,
                    Some(crate::secure::SecureProblem {
                        code: crate::secure::SecureProblemCode::GroupStateUnavailable,
                        message: "this device is not an active MLS member of the group".to_owned(),
                        retryable: false,
                    }),
                    Vec::new(),
                ),
                V2LocalGroupReadiness::Missing => return Ok(missing_status(group)),
            }
        };

        Ok(crate::secure::GroupSecureStatus {
            group,
            state,
            can_send_secure: active && pending_commits == 0,
            local_readiness: crate::secure::GroupSecureLocalReadiness {
                has_local_state: inspected.readiness != V2LocalGroupReadiness::Missing,
                has_active_membership: active,
            },
            pending_work: crate::secure::GroupSecurePendingWork {
                // Welcome/Commit remain durable device-targeted Host notices;
                // the control-notice path consumes them idempotently.
                pending_notices: 0,
                pending_commits,
            },
            problem,
            warnings,
        })
    }

    pub(crate) fn repair(
        &self,
        group: crate::ids::GroupRef,
        request_id: impl Into<String>,
    ) -> crate::ImResult<crate::secure::GroupSecureRepairResult> {
        require_group_did(group.as_str())?;
        let reconciled = self.runtime.reconcile_pending(V2ReconcilePendingInput {
            request_id: request_id.into(),
        })?;
        let target_entries = reconciled
            .pending_commits
            .iter()
            .filter(|entry| entry.group_did == group.as_str())
            .collect::<Vec<_>>();
        let repaired = target_entries
            .iter()
            .any(|entry| matches!(entry.status.as_str(), "aborted" | "finalized"));
        let status = self.status(group.clone())?;
        let mut warnings = status.warnings;
        if target_entries
            .iter()
            .any(|entry| entry.status == "prepared")
        {
            warnings.push(
                "an exact Host result is required before the prepared MLS commit can finish"
                    .to_owned(),
            );
        }
        Ok(crate::secure::GroupSecureRepairResult {
            group,
            state: status.state,
            repaired,
            problem: status.problem,
            warnings,
        })
    }
}

fn missing_status(group: crate::ids::GroupRef) -> crate::secure::GroupSecureStatus {
    crate::secure::GroupSecureStatus {
        group,
        state: crate::secure::GroupSecureState::MissingLocalState,
        can_send_secure: false,
        local_readiness: crate::secure::GroupSecureLocalReadiness {
            has_local_state: false,
            has_active_membership: false,
        },
        pending_work: crate::secure::GroupSecurePendingWork::default(),
        problem: Some(crate::secure::SecureProblem {
            code: crate::secure::SecureProblemCode::GroupStateUnavailable,
            message: "this device has not processed its P6 v2 Welcome for the group".to_owned(),
            retryable: true,
        }),
        warnings: Vec::new(),
    }
}

fn require_group_did(group: &str) -> crate::ImResult<()> {
    if group.trim().starts_with("did:") {
        return Ok(());
    }
    Err(crate::ImError::invalid_input(
        Some("group".to_owned()),
        "P6 v2 group target must be a DID",
    ))
}
