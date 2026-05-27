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

    pub fn outbox(&self) -> SecureOutboxService<'a> {
        SecureOutboxService {
            client: self.client,
        }
    }
}

pub struct DirectSecureConversation<'a> {
    client: &'a crate::core::ImClient,
    peer: crate::ids::PeerRef,
}

impl DirectSecureConversation<'_> {
    pub fn status(&self) -> crate::ImResult<super::DirectSecureStatus> {
        direct_status_to_dto(
            crate::internal::secure_direct::status::direct_status_for_client(
                self.client,
                self.peer.clone(),
            )?,
        )
    }

    pub fn prepare(&self) -> crate::ImResult<super::DirectSecurePrepareResult> {
        let plan = crate::internal::secure_direct::prepare::prepare_direct_for_client(
            self.client,
            self.peer.clone(),
        )?;
        let mut warnings = plan.status.warnings;
        if plan.prepared_local_send_state {
            warnings.push("direct E2EE local send state prepared".to_owned());
        }
        Ok(super::DirectSecurePrepareResult {
            peer: plan.status.peer,
            state: plan.status.state,
            can_send_secure: plan.status.can_send_secure,
            warnings,
        })
    }

    pub fn repair(&self) -> crate::ImResult<super::DirectSecureRepairResult> {
        let plan = crate::internal::secure_direct::status::repair_direct_for_client(
            self.client,
            self.peer.clone(),
        )?;
        let mut warnings = plan.status.warnings;
        let mut state = plan.status.state;
        let mut problem = plan.status.problem;
        let mut prepared_local_send_state = false;
        match crate::internal::secure_direct::prepare::prepare_direct_for_client(
            self.client,
            self.peer.clone(),
        ) {
            Ok(prepare_plan) => {
                warnings.extend(prepare_plan.status.warnings);
                prepared_local_send_state = prepare_plan.prepared_local_send_state;
                if prepared_local_send_state {
                    state = prepare_plan.status.state;
                    problem = prepare_plan.status.problem;
                    warnings.push("direct E2EE local send state prepared".to_owned());
                }
            }
            Err(err) => {
                warnings.push(format!("direct E2EE prekey preparation failed: {err}"));
            }
        }
        if plan.requeued_outbox_count > 0 {
            warnings.push(format!(
                "{} failed secure outbox item(s) were moved back to queued",
                plan.requeued_outbox_count
            ));
        }
        Ok(super::DirectSecureRepairResult {
            peer: plan.status.peer,
            state,
            repaired: plan.removed_session
                || plan.requeued_outbox_count > 0
                || prepared_local_send_state,
            problem,
            prepared_local_send_state,
            warnings,
        })
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
        #[cfg(feature = "group-e2ee")]
        {
            let provider =
                match crate::internal::group_e2ee::storage::native_provider_for_client(self.client)
                {
                    Ok(provider) => provider,
                    Err(err) => return Ok(group_status_unavailable(self.group.clone(), err)),
                };
            let status = crate::internal::group_e2ee::status::GroupE2eeStatusRuntime::new(
                self.client,
                crate::internal::auth::session::FileSessionProvider::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
                provider,
            )
            .status(crate::internal::group_e2ee::status::GroupE2eeStatusInput {
                group: self.group.clone(),
                credentials: None,
                notice_limit: 50,
            });
            match status {
                Ok(status) => group_status_to_dto(status),
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
                state: super::GroupSecureState::Unknown,
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

    pub fn repair(&self) -> crate::ImResult<super::GroupSecureRepairResult> {
        #[cfg(feature = "group-e2ee")]
        {
            let provider =
                crate::internal::group_e2ee::storage::native_provider_for_client(self.client)?;
            group_repair_to_dto(
                crate::internal::group_e2ee::repair::GroupE2eeRepairRuntime::new(
                    self.client,
                    crate::internal::auth::session::FileSessionProvider::new(self.client),
                    crate::internal::transport::CoreHttpTransport::new(self.client),
                    provider,
                )
                .repair(
                    crate::internal::group_e2ee::repair::GroupE2eeRepairInput {
                        group: self.group.clone(),
                        credentials: None,
                        notice_limit: 50,
                    },
                )?,
            )
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

#[cfg(feature = "group-e2ee")]
fn group_status_to_dto(
    status: crate::internal::group_e2ee::status::GroupE2eeStatusResult,
) -> crate::ImResult<super::GroupSecureStatus> {
    Ok(super::GroupSecureStatus {
        group: status.group,
        state: status.state,
        can_send_secure: status.can_send_secure,
        local_readiness: status.local_readiness,
        pending_work: status.pending_work,
        problem: status.problem,
        warnings: status.warnings,
    })
}

#[cfg(feature = "group-e2ee")]
fn group_repair_to_dto(
    result: crate::internal::group_e2ee::repair::GroupE2eeRepairResult,
) -> crate::ImResult<super::GroupSecureRepairResult> {
    Ok(super::GroupSecureRepairResult {
        group: result.group,
        state: result.state,
        repaired: result.repaired,
        problem: result.problem,
        warnings: result.warnings,
    })
}

pub struct SecureOutboxService<'a> {
    client: &'a crate::core::ImClient,
}

impl SecureOutboxService<'_> {
    pub fn list_failed(&self) -> crate::ImResult<Vec<super::SecureOutboxEntry>> {
        #[cfg(feature = "sqlite")]
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
        #[cfg(not(feature = "sqlite"))]
        {
            let _ = self.client.current_identity();
            Ok(Vec::new())
        }
    }

    pub fn retry(
        &self,
        outbox_id: super::SecureOutboxId,
    ) -> crate::ImResult<super::SecureOutboxResult> {
        #[cfg(feature = "sqlite")]
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
        #[cfg(not(feature = "sqlite"))]
        {
            let _ = outbox_id;
            Err(crate::ImError::unsupported("secure-outbox"))
        }
    }

    pub fn drop(
        &self,
        outbox_id: super::SecureOutboxId,
    ) -> crate::ImResult<super::SecureOutboxResult> {
        #[cfg(feature = "sqlite")]
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
