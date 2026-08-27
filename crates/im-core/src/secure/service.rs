pub struct SecureService<'a> {
    client: &'a crate::core::ImClient,
}

impl<'a> SecureService<'a> {
    pub(crate) fn new(client: &'a crate::core::ImClient) -> Self {
        Self { client }
    }

    pub fn direct(&self, peer: crate::ids::PeerRef) -> DirectSecureConversation<'a> {
        DirectSecureConversation {
            client: self.client,
            peer,
        }
    }

    pub fn group(&self, group: crate::ids::GroupRef) -> GroupSecureConversation<'a> {
        GroupSecureConversation {
            client: self.client,
            group,
        }
    }
}

pub struct DirectSecureConversation<'a> {
    client: &'a crate::core::ImClient,
    peer: crate::ids::PeerRef,
}

impl DirectSecureConversation<'_> {
    pub fn status(&self) -> crate::ImResult<super::DirectSecureStatus> {
        #[cfg(feature = "blocking")]
        {
            direct_status_to_dto(
                crate::internal::secure_direct::status::direct_status_for_client(
                    self.client,
                    self.peer.clone(),
                )?,
            )
        }
        #[cfg(not(feature = "blocking"))]
        {
            let _ = self.client.current_identity();
            Err(crate::ImError::unsupported("sync-direct-secure-status"))
        }
    }

    pub async fn status_async(&self) -> crate::ImResult<super::DirectSecureStatus> {
        direct_status_to_dto(
            crate::internal::secure_direct::status::direct_status_for_client_async(
                self.client,
                self.peer.clone(),
            )
            .await?,
        )
    }
}

fn direct_status_to_dto(
    status: crate::internal::secure_direct::status::DirectSecureLocalStatus,
) -> crate::ImResult<super::DirectSecureStatus> {
    Ok(super::DirectSecureStatus {
        peer: status.peer,
        resolved_peer: status.resolved_peer,
        state: status.state,
        can_send_secure: status.can_send_secure,
        pending_outbox_count: status.pending_outbox_count,
        problem: status.problem,
        warnings: status.warnings,
    })
}

pub struct GroupSecureConversation<'a> {
    client: &'a crate::core::ImClient,
    group: crate::ids::GroupRef,
}

impl GroupSecureConversation<'_> {
    pub fn status(&self) -> crate::ImResult<super::GroupSecureStatus> {
        #[cfg(all(feature = "group-e2ee", feature = "blocking"))]
        {
            if !self.client.core_inner().group_e2ee_v2_enabled() {
                return Ok(group_status_unavailable(
                    self.group.clone(),
                    crate::ImError::unsupported("group-e2ee-v2"),
                ));
            }
            let authoritative = match crate::internal::group_runtime::read::GroupReadRuntime::new(
                self.client,
                crate::internal::auth::session::FileSessionProvider::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
            )
            .get_with_policy(self.group.clone())
            {
                Ok(authoritative) => authoritative,
                Err(err) => {
                    return Ok(group_status_unavailable(self.group.clone(), err));
                }
            };
            let runtime =
                match crate::internal::group_e2ee::v2_runtime::runtime_for_client(self.client) {
                    Ok(runtime) => runtime,
                    Err(err) => return Ok(group_status_unavailable(self.group.clone(), err)),
                };
            let status =
                crate::internal::group_e2ee::v2_status::GroupE2eeV2StatusRuntime::new(runtime)
                    .status(self.group.clone());
            match status.and_then(|status| overlay_group_maintenance_status(status, &authoritative))
            {
                Ok(status) => Ok(status),
                Err(err) if group_status_can_downgrade_error(&err) => {
                    Ok(group_status_unavailable(self.group.clone(), err))
                }
                Err(err) => Err(err),
            }
        }
        #[cfg(all(feature = "group-e2ee", not(feature = "blocking")))]
        {
            let _ = self.client.current_identity();
            Err(crate::ImError::unsupported("sync-group-secure-status"))
        }
        #[cfg(not(feature = "group-e2ee"))]
        {
            let _ = self.client.current_identity();
            Ok(super::GroupSecureStatus {
                group: self.group.clone(),
                state: super::GroupSecureState::Unavailable,
                can_send_secure: false,
                local_readiness: super::GroupSecureLocalReadiness {
                    has_local_state: false,
                    has_active_membership: false,
                },
                pending_work: super::GroupSecurePendingWork::default(),
                problem: Some(super::SecureProblem {
                    code: super::SecureProblemCode::Unsupported,
                    message: "group-e2ee-status is not available yet".to_owned(),
                    retryable: false,
                }),
                warnings: Vec::new(),
            })
        }
    }

    pub fn prepare(&self) -> crate::ImResult<super::GroupSecurePrepareResult> {
        let status = self.status()?;
        Ok(super::GroupSecurePrepareResult {
            group: status.group,
            state: status.state,
            can_send_secure: status.can_send_secure,
            warnings: status.warnings,
        })
    }

    pub async fn status_async(&self) -> crate::ImResult<super::GroupSecureStatus> {
        #[cfg(feature = "group-e2ee")]
        {
            if !self.client.core_inner().group_e2ee_v2_enabled() {
                return Ok(group_status_unavailable(
                    self.group.clone(),
                    crate::ImError::unsupported("group-e2ee-v2"),
                ));
            }
            let authoritative = match crate::internal::group_runtime::read::GroupReadRuntime::new(
                self.client,
                crate::internal::auth::session::FileSessionProvider::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
            )
            .get_with_policy_async(self.group.clone())
            .await
            {
                Ok(authoritative) => authoritative,
                Err(err) => {
                    return Ok(group_status_unavailable(self.group.clone(), err));
                }
            };
            let runtime =
                match crate::internal::group_e2ee::v2_runtime::runtime_for_client(self.client) {
                    Ok(runtime) => runtime,
                    Err(err) => return Ok(group_status_unavailable(self.group.clone(), err)),
                };
            let group = self.group.clone();
            let status = crate::internal::runtime::worker::run_blocking(move || {
                crate::internal::group_e2ee::v2_status::GroupE2eeV2StatusRuntime::new(runtime)
                    .status(group)
            })
            .await
            .map_err(|err| crate::ImError::Internal {
                message: format!("P6 v2 status worker failed: {err}"),
            })?;
            match status.and_then(|status| overlay_group_maintenance_status(status, &authoritative))
            {
                Ok(status) => Ok(status),
                Err(err) if group_status_can_downgrade_error(&err) => {
                    Ok(group_status_unavailable(self.group.clone(), err))
                }
                Err(err) => Err(err),
            }
        }
        #[cfg(not(feature = "group-e2ee"))]
        {
            let _ = self.client.current_identity();
            Ok(super::GroupSecureStatus {
                group: self.group.clone(),
                state: super::GroupSecureState::Unavailable,
                can_send_secure: false,
                local_readiness: super::GroupSecureLocalReadiness {
                    has_local_state: false,
                    has_active_membership: false,
                },
                pending_work: super::GroupSecurePendingWork::default(),
                problem: Some(super::SecureProblem {
                    code: super::SecureProblemCode::Unsupported,
                    message: "group-e2ee-status is not available yet".to_owned(),
                    retryable: false,
                }),
                warnings: Vec::new(),
            })
        }
    }

    pub async fn prepare_async(&self) -> crate::ImResult<super::GroupSecurePrepareResult> {
        let status = self.status_async().await?;
        Ok(super::GroupSecurePrepareResult {
            group: status.group,
            state: status.state,
            can_send_secure: status.can_send_secure,
            warnings: status.warnings,
        })
    }

    pub fn repair(&self) -> crate::ImResult<super::GroupSecureRepairResult> {
        #[cfg(all(feature = "group-e2ee", feature = "blocking"))]
        {
            if !self.client.core_inner().group_e2ee_v2_enabled() {
                return Err(crate::ImError::unsupported("group-e2ee-v2"));
            }
            let roster = crate::internal::group_e2ee::v2_lifecycle::reconcile_group_device_roster(
                self.client,
                self.group.clone(),
            )?;
            let runtime = crate::internal::group_e2ee::v2_runtime::runtime_for_client(self.client)?;
            let mut result =
                crate::internal::group_e2ee::v2_status::GroupE2eeV2StatusRuntime::new(runtime)
                    .repair(
                        self.group.clone(),
                        format!(
                            "im-core-p6-v2-repair-{}",
                            crate::internal::wire::common::generate_operation_id()
                        ),
                    )?;
            result.added_devices = u32::try_from(roster.added_devices).unwrap_or(u32::MAX);
            result.removed_devices = u32::try_from(roster.removed_devices).unwrap_or(u32::MAX);
            result.remaining_devices = u32::try_from(roster.remaining_devices).unwrap_or(u32::MAX);
            result.repaired |= roster.added_devices > 0
                || roster.removed_devices > 0
                || roster.repaired_wal_entries > 0;
            Ok(result)
        }
        #[cfg(all(feature = "group-e2ee", not(feature = "blocking")))]
        {
            let _ = self.client.current_identity();
            Err(crate::ImError::unsupported("sync-group-secure-repair"))
        }
        #[cfg(not(feature = "group-e2ee"))]
        {
            Err(crate::ImError::unsupported("group-e2ee"))
        }
    }

    pub async fn repair_async(&self) -> crate::ImResult<super::GroupSecureRepairResult> {
        #[cfg(feature = "group-e2ee")]
        {
            if !self.client.core_inner().group_e2ee_v2_enabled() {
                return Err(crate::ImError::unsupported("group-e2ee-v2"));
            }
            let request_id = format!(
                "im-core-p6-v2-repair-{}",
                crate::internal::wire::common::generate_operation_id()
            );
            let roster =
                crate::internal::group_e2ee::v2_lifecycle::reconcile_group_device_roster_async(
                    self.client,
                    self.group.clone(),
                )
                .await?;
            let runtime = crate::internal::group_e2ee::v2_runtime::runtime_for_client(self.client)?;
            let mut result =
                crate::internal::group_e2ee::v2_status::GroupE2eeV2StatusRuntime::new(runtime)
                    .repair(self.group.clone(), request_id)?;
            result.added_devices = u32::try_from(roster.added_devices).unwrap_or(u32::MAX);
            result.removed_devices = u32::try_from(roster.removed_devices).unwrap_or(u32::MAX);
            result.remaining_devices = u32::try_from(roster.remaining_devices).unwrap_or(u32::MAX);
            result.repaired |= roster.added_devices > 0
                || roster.removed_devices > 0
                || roster.repaired_wal_entries > 0;
            Ok(result)
        }
        #[cfg(not(feature = "group-e2ee"))]
        {
            Err(crate::ImError::unsupported("group-e2ee"))
        }
    }
}

#[cfg(feature = "group-e2ee")]
fn group_status_unavailable(
    group: crate::ids::GroupRef,
    err: crate::ImError,
) -> super::GroupSecureStatus {
    let problem_code = group_status_problem_code(&err);
    super::GroupSecureStatus {
        group,
        state: super::GroupSecureState::Unavailable,
        can_send_secure: false,
        local_readiness: super::GroupSecureLocalReadiness {
            has_local_state: false,
            has_active_membership: false,
        },
        pending_work: super::GroupSecurePendingWork::default(),
        problem: Some(super::SecureProblem {
            code: problem_code,
            message: format!("group E2EE status is unavailable: {err}"),
            retryable: true,
        }),
        warnings: vec!["group E2EE status is unavailable".to_owned()],
    }
}

#[cfg(feature = "group-e2ee")]
fn overlay_group_maintenance_status(
    mut status: super::GroupSecureStatus,
    authoritative: &crate::groups::GroupReadResult,
) -> crate::ImResult<super::GroupSecureStatus> {
    let raw =
        authoritative
            .raw_response()
            .ok_or_else(|| crate::ImError::LocalStateUnavailable {
                detail: "authoritative group.get omitted its response".to_owned(),
            })?;
    if raw.get("group_did").and_then(serde_json::Value::as_str) != Some(status.group.as_str()) {
        return Err(crate::ImError::LocalStateUnavailable {
            detail: "authoritative group.get returned a conflicting group".to_owned(),
        });
    }
    let Some(maintenance) = raw.get("e2ee_maintenance") else {
        return Ok(status);
    };
    let maintenance =
        maintenance
            .as_object()
            .ok_or_else(|| crate::ImError::LocalStateUnavailable {
                detail: "authoritative group maintenance projection is malformed".to_owned(),
            })?;
    if maintenance.len() != 2
        || !maintenance.contains_key("send_paused")
        || !maintenance.contains_key("reason")
        || maintenance
            .get("send_paused")
            .and_then(serde_json::Value::as_bool)
            != Some(true)
        || maintenance
            .get("reason")
            .and_then(serde_json::Value::as_str)
            != Some("device_revocation_pending")
    {
        return Err(crate::ImError::LocalStateUnavailable {
            detail: "authoritative group maintenance projection is unsupported".to_owned(),
        });
    }
    let active_owner = authoritative.group.as_ref().is_some_and(|group| {
        group.did == status.group
            && group.membership_status.as_deref() == Some("active")
            && group.my_role.as_deref() == Some("owner")
    });
    status.can_send_secure = false;
    if !active_owner {
        status.state = super::GroupSecureState::WaitingForMembershipUpdate;
        status.problem = Some(super::SecureProblem {
            code: super::SecureProblemCode::GroupStateUnavailable,
            message: "group sending is paused for device revocation convergence".to_owned(),
            retryable: true,
        });
    } else if status.local_readiness.has_active_membership && status.local_readiness.has_local_state
    {
        status.state = super::GroupSecureState::NeedsRepair;
        status.problem = Some(super::SecureProblem {
            code: super::SecureProblemCode::SessionNeedsRepair,
            message: "group device membership requires repair before sending resumes".to_owned(),
            retryable: true,
        });
    } else {
        status.state = super::GroupSecureState::MissingLocalState;
        status.problem = Some(super::SecureProblem {
            code: super::SecureProblemCode::GroupStateUnavailable,
            message: "this device does not hold the group controller state required for repair"
                .to_owned(),
            retryable: true,
        });
    }
    status
        .warnings
        .push("group sending is paused until device revocation convergence completes".to_owned());
    Ok(status)
}

#[cfg(feature = "group-e2ee")]
fn group_status_can_downgrade_error(err: &crate::ImError) -> bool {
    matches!(
        err,
        crate::ImError::IdentityRequired
            | crate::ImError::IdentityNotFound { .. }
            | crate::ImError::DefaultIdentityMissing
            | crate::ImError::IdentityNotReady { .. }
            | crate::ImError::AuthRequired
            | crate::ImError::SessionExpired
            | crate::ImError::TransportUnavailable { .. }
            | crate::ImError::LocalStateUnavailable { .. }
            | crate::ImError::CursorInvalid
            | crate::ImError::CursorStale
            | crate::ImError::InventoryIncomplete
            | crate::ImError::InventoryTooLarge
            | crate::ImError::PathUnavailable { .. }
            | crate::ImError::CredentialFileUnreadable { .. }
    )
}

#[cfg(feature = "group-e2ee")]
fn group_status_problem_code(err: &crate::ImError) -> super::SecureProblemCode {
    match err {
        crate::ImError::TransportUnavailable { .. } => {
            super::SecureProblemCode::TransportUnavailable
        }
        crate::ImError::LocalStateUnavailable { .. } => {
            super::SecureProblemCode::LocalStateUnavailable
        }
        _ => super::SecureProblemCode::IdentityNotReady,
    }
}

pub(crate) struct SecureOutboxService<'a> {
    client: &'a crate::core::ImClient,
}

impl SecureOutboxService<'_> {
    pub(crate) fn list_failed(&self) -> crate::ImResult<Vec<super::SecureOutboxEntry>> {
        #[cfg(all(feature = "sqlite", feature = "blocking"))]
        {
            let connection = crate::internal::local_state::open_writable(
                &self.client.core_inner().sdk_paths().local_state.sqlite_path,
            )?;
            let scope =
                crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope::for_client(self.client);
            let entries = crate::internal::store::e2ee_outbox::list_e2ee_outbox(
                &connection,
                &scope,
                Some("failed"),
            )?
            .iter()
            .map(crate::internal::store::e2ee_outbox::secure_outbox_entry_from_record)
            .collect::<crate::ImResult<Vec<_>>>()?;
            Ok(entries)
        }
        #[cfg(all(feature = "sqlite", not(feature = "blocking")))]
        {
            let _ = self.client.current_identity();
            Err(crate::ImError::unsupported("sync-secure-outbox"))
        }
        #[cfg(not(feature = "sqlite"))]
        {
            let _ = self.client.current_identity();
            Ok(Vec::new())
        }
    }

    pub(crate) async fn list_failed_async(&self) -> crate::ImResult<Vec<super::SecureOutboxEntry>> {
        #[cfg(feature = "sqlite")]
        {
            let db = self.client.core_inner().local_state_db().await?;
            let scope =
                crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope::for_client(self.client);
            db.list_e2ee_outbox(scope, Some("failed".to_owned()))
                .await?
                .iter()
                .map(crate::internal::store::e2ee_outbox::secure_outbox_entry_from_record)
                .collect::<crate::ImResult<Vec<_>>>()
        }
        #[cfg(not(feature = "sqlite"))]
        {
            let _ = self.client.current_identity();
            Ok(Vec::new())
        }
    }

    pub(crate) fn retry(
        &self,
        outbox_id: super::SecureOutboxId,
    ) -> crate::ImResult<super::SecureOutboxResult> {
        #[cfg(all(feature = "sqlite", feature = "blocking"))]
        {
            let connection = crate::internal::local_state::open_writable(
                &self.client.core_inner().sdk_paths().local_state.sqlite_path,
            )?;
            let scope =
                crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope::for_client(self.client);
            let outbox_id_value = outbox_id.as_str().to_owned();
            outbox_result_from_record(
                outbox_id,
                crate::internal::store::e2ee_outbox::retry_e2ee_outbox(
                    &connection,
                    &scope,
                    &outbox_id_value,
                )?,
            )
        }
        #[cfg(all(feature = "sqlite", not(feature = "blocking")))]
        {
            let _ = outbox_id;
            Err(crate::ImError::unsupported("sync-secure-outbox"))
        }
        #[cfg(not(feature = "sqlite"))]
        {
            let _ = outbox_id;
            Err(crate::ImError::unsupported("secure-outbox"))
        }
    }

    pub(crate) async fn retry_async(
        &self,
        outbox_id: super::SecureOutboxId,
    ) -> crate::ImResult<super::SecureOutboxResult> {
        #[cfg(feature = "sqlite")]
        {
            let db = self.client.core_inner().local_state_db().await?;
            let scope =
                crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope::for_client(self.client);
            let outbox_id_value = outbox_id.as_str().to_owned();
            outbox_result_from_record(
                outbox_id,
                db.retry_e2ee_outbox(scope, outbox_id_value).await?,
            )
        }
        #[cfg(not(feature = "sqlite"))]
        {
            let _ = outbox_id;
            Err(crate::ImError::unsupported("secure-outbox"))
        }
    }

    pub(crate) fn drop(
        &self,
        outbox_id: super::SecureOutboxId,
    ) -> crate::ImResult<super::SecureOutboxResult> {
        #[cfg(all(feature = "sqlite", feature = "blocking"))]
        {
            let connection = crate::internal::local_state::open_writable(
                &self.client.core_inner().sdk_paths().local_state.sqlite_path,
            )?;
            let scope =
                crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope::for_client(self.client);
            let outbox_id_value = outbox_id.as_str().to_owned();
            outbox_result_from_record(
                outbox_id,
                crate::internal::store::e2ee_outbox::drop_e2ee_outbox(
                    &connection,
                    &scope,
                    &outbox_id_value,
                )?,
            )
        }
        #[cfg(all(feature = "sqlite", not(feature = "blocking")))]
        {
            let _ = outbox_id;
            Err(crate::ImError::unsupported("sync-secure-outbox"))
        }
        #[cfg(not(feature = "sqlite"))]
        {
            let _ = outbox_id;
            Err(crate::ImError::unsupported("secure-outbox"))
        }
    }

    pub(crate) async fn drop_async(
        &self,
        outbox_id: super::SecureOutboxId,
    ) -> crate::ImResult<super::SecureOutboxResult> {
        #[cfg(feature = "sqlite")]
        {
            let db = self.client.core_inner().local_state_db().await?;
            let scope =
                crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope::for_client(self.client);
            let outbox_id_value = outbox_id.as_str().to_owned();
            outbox_result_from_record(
                outbox_id,
                db.drop_e2ee_outbox(scope, outbox_id_value).await?,
            )
        }
        #[cfg(not(feature = "sqlite"))]
        {
            let _ = outbox_id;
            Err(crate::ImError::unsupported("secure-outbox"))
        }
    }
}

#[cfg(feature = "sqlite")]
fn outbox_result_from_record(
    id: super::SecureOutboxId,
    record: Option<crate::internal::store::e2ee_outbox::E2eeOutboxRecord>,
) -> crate::ImResult<super::SecureOutboxResult> {
    let Some(record) = record else {
        return Err(crate::ImError::MessageNotFound {
            message_id: id.as_str().to_owned(),
        });
    };
    Ok(super::SecureOutboxResult {
        id,
        status: match record.local_status.trim() {
            "sending" => super::SecureOutboxStatus::Sending,
            "failed" => super::SecureOutboxStatus::Failed,
            "sent" => super::SecureOutboxStatus::Sent,
            "dropped" => super::SecureOutboxStatus::Dropped,
            _ => super::SecureOutboxStatus::Queued,
        },
        delivery: None,
        warnings: Vec::new(),
    })
}

#[cfg(all(test, feature = "group-e2ee"))]
mod step4_tests {
    use super::*;

    fn local_status(
        has_active_membership: bool,
        has_local_state: bool,
    ) -> crate::secure::GroupSecureStatus {
        crate::secure::GroupSecureStatus {
            group: crate::ids::GroupRef::parse("did:example:group").unwrap(),
            state: crate::secure::GroupSecureState::Ready,
            can_send_secure: true,
            local_readiness: crate::secure::GroupSecureLocalReadiness {
                has_local_state,
                has_active_membership,
            },
            pending_work: Default::default(),
            problem: None,
            warnings: Vec::new(),
        }
    }

    fn authoritative(role: &str) -> crate::groups::GroupReadResult {
        crate::groups::GroupReadResult::from_raw_response(
            serde_json::json!({
                "group_did": "did:example:group",
                "group": {
                    "group_did": "did:example:group",
                    "my_role": role,
                    "membership_status": "active"
                },
                "e2ee_maintenance": {
                    "send_paused": true,
                    "reason": "device_revocation_pending"
                }
            }),
            Vec::new(),
        )
    }

    #[test]
    fn maintenance_gate_never_reports_ready() {
        let owner =
            overlay_group_maintenance_status(local_status(true, true), &authoritative("owner"))
                .unwrap();
        assert_eq!(owner.state, crate::secure::GroupSecureState::NeedsRepair);
        assert!(!owner.can_send_secure);

        let member =
            overlay_group_maintenance_status(local_status(true, true), &authoritative("member"))
                .unwrap();
        assert_eq!(
            member.state,
            crate::secure::GroupSecureState::WaitingForMembershipUpdate
        );

        let missing =
            overlay_group_maintenance_status(local_status(false, false), &authoritative("owner"))
                .unwrap();
        assert_eq!(
            missing.state,
            crate::secure::GroupSecureState::MissingLocalState
        );

        let owner_without_controller =
            overlay_group_maintenance_status(local_status(true, false), &authoritative("owner"))
                .unwrap();
        assert_eq!(
            owner_without_controller.state,
            crate::secure::GroupSecureState::MissingLocalState
        );
    }

    #[test]
    fn maintenance_gate_rejects_nonminimal_projection() {
        let authoritative = crate::groups::GroupReadResult::from_raw_response(
            serde_json::json!({
                "group_did": "did:example:group",
                "group": {
                    "group_did": "did:example:group",
                    "my_role": "owner",
                    "membership_status": "active"
                },
                "e2ee_maintenance": {
                    "send_paused": true,
                    "reason": "device_revocation_pending",
                    "target_device_id": "must-not-be-projected"
                }
            }),
            Vec::new(),
        );
        assert!(
            overlay_group_maintenance_status(local_status(true, true), &authoritative).is_err()
        );
    }
}
