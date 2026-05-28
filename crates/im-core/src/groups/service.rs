#[cfg(feature = "group-e2ee")]
use crate::internal::auth::session::{AsyncSessionProvider, SessionProvider};
#[cfg(feature = "group-e2ee")]
use crate::internal::group_e2ee::provider::GroupMlsProvider;
#[cfg(feature = "group-e2ee")]
use crate::internal::transport::{AsyncAuthenticatedRpcTransport, AuthenticatedRpcTransport};

pub struct GroupService<'a> {
    client: &'a crate::core::ImClient,
}

impl<'a> GroupService<'a> {
    pub(crate) fn new(client: &'a crate::core::ImClient) -> Self {
        Self { client }
    }

    pub fn publish_key_package(
        &self,
        request: super::GroupKeyPackagePublishRequest,
    ) -> crate::ImResult<super::GroupKeyPackagePublishResult> {
        #[cfg(feature = "group-e2ee")]
        {
            self.publish_key_package_with_group_e2ee(request)
        }
        #[cfg(not(feature = "group-e2ee"))]
        {
            let _ = request;
            Err(crate::ImError::unsupported("group-e2ee"))
        }
    }

    pub async fn publish_key_package_async(
        &self,
        request: super::GroupKeyPackagePublishRequest,
    ) -> crate::ImResult<super::GroupKeyPackagePublishResult> {
        #[cfg(feature = "group-e2ee")]
        {
            self.publish_key_package_with_group_e2ee_async(request)
                .await
        }
        #[cfg(not(feature = "group-e2ee"))]
        {
            let _ = request;
            Err(crate::ImError::unsupported("group-e2ee"))
        }
    }

    pub fn create(
        &self,
        request: super::GroupCreateRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let secure_required = group_create_uses_e2ee(&request);
        #[cfg(not(feature = "group-e2ee"))]
        if secure_required {
            return Err(crate::ImError::unsupported("group-e2ee"));
        }
        #[cfg(feature = "group-e2ee")]
        let secure_provider = if secure_required {
            ensure_group_e2ee_service_available(self.client, false)?;
            Some(crate::internal::group_e2ee::storage::native_provider_for_client(self.client)?)
        } else {
            None
        };
        let mut result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .create(request, None)?;
        crate::internal::group_runtime::projection::project_group_snapshot(self.client, &result);
        self.refresh_group_state(&mut result, true);
        #[cfg(feature = "group-e2ee")]
        if secure_required {
            let group =
                group_did(&result).ok_or_else(|| crate::ImError::LocalStateUnavailable {
                    detail: "group E2EE create requires created group DID".to_owned(),
                })?;
            let secure = crate::internal::group_e2ee::lifecycle::GroupE2eeLifecycleRuntime::new(
                self.client,
                crate::internal::auth::session::FileSessionProvider::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
                secure_provider.expect("secure provider initialized when secure_required"),
            )
            .create_secure_group(
                crate::internal::group_e2ee::lifecycle::GroupE2eeCreateInput {
                    group: crate::ids::GroupRef::parse(&group)?,
                    credentials: None,
                    service_did: None,
                },
            )?;
            result.warnings.extend(secure.warnings);
        }
        Ok(result)
    }

    pub async fn create_async(
        &self,
        request: super::GroupCreateRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let secure_required = group_create_uses_e2ee(&request);
        #[cfg(not(feature = "group-e2ee"))]
        if secure_required {
            return Err(crate::ImError::unsupported("group-e2ee"));
        }
        #[cfg(feature = "group-e2ee")]
        if secure_required {
            ensure_group_e2ee_service_available_async(self.client, false).await?;
        }
        let mut result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .create_async(request, None)
        .await?;
        let _ = crate::internal::group_runtime::projection::project_group_snapshot_async(
            self.client,
            &result,
        )
        .await;
        self.refresh_group_state_async(&mut result, true).await;
        #[cfg(feature = "group-e2ee")]
        if secure_required {
            let group =
                group_did(&result).ok_or_else(|| crate::ImError::LocalStateUnavailable {
                    detail: "group E2EE create requires created group DID".to_owned(),
                })?;
            let secure = crate::internal::group_e2ee::lifecycle::GroupE2eeLifecycleRuntime::new(
                self.client,
                crate::internal::auth::session::FileSessionProvider::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
                crate::internal::group_e2ee::storage::native_provider_for_client(self.client)?,
            )
            .create_secure_group_async(
                crate::internal::group_e2ee::lifecycle::GroupE2eeCreateInput {
                    group: crate::ids::GroupRef::parse(&group)?,
                    credentials: None,
                    service_did: None,
                },
            )
            .await?;
            result.warnings.extend(secure.warnings);
        }
        Ok(result)
    }

    pub fn join(
        &self,
        request: super::GroupJoinRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let group = request.group.as_str().to_string();
        let mut result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .join(request, None)?;
        crate::internal::group_runtime::projection::project_group_snapshot(self.client, &result);
        self.refresh_group_state_for(&mut result, &group, true);
        Ok(result)
    }

    pub async fn join_async(
        &self,
        request: super::GroupJoinRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let group = request.group.as_str().to_string();
        let mut result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .join_async(request, None)
        .await?;
        let _ = crate::internal::group_runtime::projection::project_group_snapshot_async(
            self.client,
            &result,
        )
        .await;
        self.refresh_group_state_for_async(&mut result, &group, true)
            .await;
        Ok(result)
    }

    pub fn leave(
        &self,
        request: super::GroupLeaveRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let group = request.group.as_str().to_string();
        #[cfg(not(feature = "group-e2ee"))]
        if request.security.required() {
            return Err(crate::ImError::unsupported("group-e2ee"));
        }
        #[cfg(feature = "group-e2ee")]
        let secure_provider = if request.security.required() {
            ensure_group_e2ee_service_available(self.client, false)?;
            Some(crate::internal::group_e2ee::storage::native_provider_for_client(self.client)?)
        } else {
            None
        };
        if let Ok(Some(snapshot)) =
            crate::internal::group_runtime::cache::cached_group_snapshot(self.client, &group)
        {
            if crate::internal::group_runtime::cache::is_active_group_owner(&snapshot) {
                return Err(crate::ImError::invalid_input(
                    Some("group".to_string()),
                    "group owner cannot leave the group",
                ));
            }
            if crate::internal::group_runtime::cache::group_snapshot_uses_e2ee(&snapshot)
                && !request.security.required()
            {
                return Err(crate::ImError::unsupported("group-e2ee"));
            }
        }
        if request.security.required() {
            #[cfg(feature = "group-e2ee")]
            {
                let secure =
                    crate::internal::group_e2ee::lifecycle::GroupE2eeLifecycleRuntime::new(
                        self.client,
                        crate::internal::auth::session::FileSessionProvider::new(self.client),
                        crate::internal::transport::CoreHttpTransport::new(self.client),
                        secure_provider.expect("secure provider initialized when secure_required"),
                    )
                    .leave_secure_group(
                        crate::internal::group_e2ee::lifecycle::GroupE2eeLeaveInput {
                            group: request.group,
                            reason_text: request.reason_text,
                            owner_leave_commit: false,
                            credentials: None,
                        },
                    )?;
                return Ok(super::GroupReadResult::from_raw_response(
                    secure.delivery,
                    secure.warnings,
                ));
            }
        }
        let result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .leave(request, None)?;
        crate::internal::group_runtime::projection::project_group_left(self.client, &group);
        Ok(result)
    }

    pub async fn leave_async(
        &self,
        request: super::GroupLeaveRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let group = request.group.as_str().to_string();
        #[cfg(not(feature = "group-e2ee"))]
        if request.security.required() {
            return Err(crate::ImError::unsupported("group-e2ee"));
        }
        #[cfg(feature = "group-e2ee")]
        if request.security.required() {
            ensure_group_e2ee_service_available_async(self.client, false).await?;
            if let Ok(Some(snapshot)) =
                crate::internal::group_runtime::cache::cached_group_snapshot_async(
                    self.client,
                    &group,
                )
                .await
            {
                if crate::internal::group_runtime::cache::is_active_group_owner(&snapshot) {
                    return Err(crate::ImError::invalid_input(
                        Some("group".to_string()),
                        "group owner cannot leave the group",
                    ));
                }
            }
            let session_provider =
                crate::internal::auth::session::FileSessionProvider::new(self.client);
            let mut transport = crate::internal::transport::CoreHttpTransport::new(self.client);
            let secure = crate::internal::group_e2ee::lifecycle::leave_secure_group_request_async(
                self.client,
                &session_provider,
                &mut transport,
                crate::internal::group_e2ee::lifecycle::GroupE2eeLeaveInput {
                    group: request.group,
                    reason_text: request.reason_text,
                    owner_leave_commit: false,
                    credentials: None,
                },
            )
            .await?;
            return Ok(super::GroupReadResult::from_raw_response(
                secure.delivery,
                secure.warnings,
            ));
        }
        if let Ok(Some(snapshot)) =
            crate::internal::group_runtime::cache::cached_group_snapshot_async(self.client, &group)
                .await
        {
            if crate::internal::group_runtime::cache::is_active_group_owner(&snapshot) {
                return Err(crate::ImError::invalid_input(
                    Some("group".to_string()),
                    "group owner cannot leave the group",
                ));
            }
            if crate::internal::group_runtime::cache::group_snapshot_uses_e2ee(&snapshot)
                && !request.security.required()
            {
                return Err(crate::ImError::unsupported("group-e2ee"));
            }
        }
        let result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .leave_async(request, None)
        .await?;
        let _ = crate::internal::group_runtime::projection::project_group_left_async(
            self.client,
            &group,
        )
        .await;
        Ok(result)
    }

    pub fn add_member(
        &self,
        request: super::GroupMemberMutationRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let group = request.group.as_str().to_string();
        let resolved_member = resolve_group_member(self.client, &request.member)?;
        #[cfg(feature = "group-e2ee")]
        let member = resolved_member.did.as_str().to_string();
        #[cfg(feature = "group-e2ee")]
        let reason_text = request.reason_text.clone();
        #[cfg(not(feature = "group-e2ee"))]
        if request.security.required() {
            return Err(crate::ImError::unsupported("group-e2ee"));
        }
        #[cfg(feature = "group-e2ee")]
        let secure_provider = if request.security.required() {
            ensure_group_e2ee_service_available(self.client, true)?;
            Some(crate::internal::group_e2ee::storage::native_provider_for_client(self.client)?)
        } else {
            None
        };
        if let Ok(Some(snapshot)) = crate::internal::group_runtime::cache::cached_group_snapshot(
            self.client,
            request.group.as_str(),
        ) {
            if crate::internal::group_runtime::cache::group_snapshot_uses_e2ee(&snapshot)
                && !request.security.required()
            {
                return Err(crate::ImError::unsupported("group-e2ee"));
            }
        }
        let mut result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .add_member(
            resolved_group_member_request(request, &resolved_member),
            None,
        )?;
        result.resolved_member = Some(resolved_member.clone());
        crate::internal::group_runtime::projection::project_group_snapshot(self.client, &result);
        self.refresh_group_state_for(&mut result, &group, true);
        #[cfg(feature = "group-e2ee")]
        if let Some(secure_provider) = secure_provider {
            let secure = crate::internal::group_e2ee::lifecycle::GroupE2eeLifecycleRuntime::new(
                self.client,
                crate::internal::auth::session::FileSessionProvider::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
                secure_provider,
            )
            .add_secure_member(
                crate::internal::group_e2ee::lifecycle::GroupE2eeMemberMutationInput {
                    group: crate::ids::GroupRef::parse(&group)?,
                    member: crate::ids::Did::parse(&member)?,
                    reason_text,
                    leave_request_id: None,
                    credentials: None,
                    service_did: None,
                },
            )?;
            result = super::GroupReadResult::from_raw_response(secure.delivery, secure.warnings);
            result.resolved_member = Some(resolved_member.clone());
            self.refresh_group_state_for(&mut result, &group, true);
        }
        Ok(result)
    }

    pub async fn add_member_async(
        &self,
        request: super::GroupMemberMutationRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let secure_required = request.security.required();
        let group = request.group.as_str().to_string();
        let resolved_member = resolve_group_member_async(self.client, &request.member).await?;
        #[cfg(feature = "group-e2ee")]
        let member = resolved_member.did.as_str().to_string();
        #[cfg(feature = "group-e2ee")]
        let reason_text = request.reason_text.clone();
        #[cfg(not(feature = "group-e2ee"))]
        if secure_required {
            return Err(crate::ImError::unsupported("group-e2ee"));
        }
        #[cfg(feature = "group-e2ee")]
        if secure_required {
            ensure_group_e2ee_service_available_async(self.client, true).await?;
        }
        if let Ok(Some(snapshot)) =
            crate::internal::group_runtime::cache::cached_group_snapshot_async(
                self.client,
                request.group.as_str(),
            )
            .await
        {
            if crate::internal::group_runtime::cache::group_snapshot_uses_e2ee(&snapshot)
                && !secure_required
            {
                return Err(crate::ImError::unsupported("group-e2ee"));
            }
        }
        let mut result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .add_member_async(
            resolved_group_member_request(request, &resolved_member),
            None,
        )
        .await?;
        result.resolved_member = Some(resolved_member.clone());
        let _ = crate::internal::group_runtime::projection::project_group_snapshot_async(
            self.client,
            &result,
        )
        .await;
        self.refresh_group_state_for_async(&mut result, &group, true)
            .await;
        #[cfg(feature = "group-e2ee")]
        if secure_required {
            let secure = crate::internal::group_e2ee::lifecycle::GroupE2eeLifecycleRuntime::new(
                self.client,
                crate::internal::auth::session::FileSessionProvider::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
                crate::internal::group_e2ee::storage::native_provider_for_client(self.client)?,
            )
            .add_secure_member_async(
                crate::internal::group_e2ee::lifecycle::GroupE2eeMemberMutationInput {
                    group: crate::ids::GroupRef::parse(&group)?,
                    member: crate::ids::Did::parse(&member)?,
                    reason_text,
                    leave_request_id: None,
                    credentials: None,
                    service_did: None,
                },
            )
            .await?;
            result = super::GroupReadResult::from_raw_response(secure.delivery, secure.warnings);
            result.resolved_member = Some(resolved_member.clone());
            self.refresh_group_state_for_async(&mut result, &group, true)
                .await;
        }
        Ok(result)
    }

    pub fn remove_member(
        &self,
        request: super::GroupMemberMutationRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let group = request.group.as_str().to_string();
        let resolved_member = resolve_group_member(self.client, &request.member)?;
        #[cfg(not(feature = "group-e2ee"))]
        if request.security.required() {
            return Err(crate::ImError::unsupported("group-e2ee"));
        }
        #[cfg(feature = "group-e2ee")]
        let secure_provider = if request.security.required() {
            ensure_group_e2ee_service_available(self.client, false)?;
            Some(crate::internal::group_e2ee::storage::native_provider_for_client(self.client)?)
        } else {
            None
        };
        if let Ok(Some(snapshot)) = crate::internal::group_runtime::cache::cached_group_snapshot(
            self.client,
            request.group.as_str(),
        ) {
            if crate::internal::group_runtime::cache::group_snapshot_uses_e2ee(&snapshot)
                && !request.security.required()
            {
                return Err(crate::ImError::unsupported("group-e2ee"));
            }
        }
        if request.security.required() {
            #[cfg(feature = "group-e2ee")]
            {
                let secure =
                    crate::internal::group_e2ee::lifecycle::GroupE2eeLifecycleRuntime::new(
                        self.client,
                        crate::internal::auth::session::FileSessionProvider::new(self.client),
                        crate::internal::transport::CoreHttpTransport::new(self.client),
                        secure_provider.expect("secure provider initialized when secure_required"),
                    )
                    .remove_secure_member(
                        crate::internal::group_e2ee::lifecycle::GroupE2eeMemberMutationInput {
                            group: request.group,
                            member: resolved_member.did.clone(),
                            reason_text: request.reason_text,
                            leave_request_id: request.leave_request_id,
                            credentials: None,
                            service_did: None,
                        },
                    )?;
                let mut result =
                    super::GroupReadResult::from_raw_response(secure.delivery, secure.warnings);
                result.resolved_member = Some(resolved_member);
                self.refresh_group_state_for(&mut result, &group, true);
                return Ok(result);
            }
        }
        let mut result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .remove_member(
            resolved_group_member_request(request, &resolved_member),
            None,
        )?;
        result.resolved_member = Some(resolved_member.clone());
        crate::internal::group_runtime::projection::project_group_snapshot(self.client, &result);
        self.refresh_group_state_for(&mut result, &group, true);
        Ok(result)
    }

    pub async fn remove_member_async(
        &self,
        request: super::GroupMemberMutationRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let secure_required = request.security.required();
        let group = request.group.as_str().to_string();
        let resolved_member = resolve_group_member_async(self.client, &request.member).await?;
        #[cfg(not(feature = "group-e2ee"))]
        if secure_required {
            return Err(crate::ImError::unsupported("group-e2ee"));
        }
        #[cfg(feature = "group-e2ee")]
        if secure_required {
            ensure_group_e2ee_service_available_async(self.client, false).await?;
        }
        if let Ok(Some(snapshot)) =
            crate::internal::group_runtime::cache::cached_group_snapshot_async(
                self.client,
                request.group.as_str(),
            )
            .await
        {
            if crate::internal::group_runtime::cache::group_snapshot_uses_e2ee(&snapshot)
                && !secure_required
            {
                return Err(crate::ImError::unsupported("group-e2ee"));
            }
        }
        #[cfg(feature = "group-e2ee")]
        if secure_required {
            let secure = crate::internal::group_e2ee::lifecycle::GroupE2eeLifecycleRuntime::new(
                self.client,
                crate::internal::auth::session::FileSessionProvider::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
                crate::internal::group_e2ee::storage::native_provider_for_client(self.client)?,
            )
            .remove_secure_member_async(
                crate::internal::group_e2ee::lifecycle::GroupE2eeMemberMutationInput {
                    group: request.group,
                    member: resolved_member.did.clone(),
                    reason_text: request.reason_text,
                    leave_request_id: request.leave_request_id,
                    credentials: None,
                    service_did: None,
                },
            )
            .await?;
            let mut result =
                super::GroupReadResult::from_raw_response(secure.delivery, secure.warnings);
            result.resolved_member = Some(resolved_member);
            self.refresh_group_state_for_async(&mut result, &group, true)
                .await;
            return Ok(result);
        }
        let mut result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .remove_member_async(
            resolved_group_member_request(request, &resolved_member),
            None,
        )
        .await?;
        result.resolved_member = Some(resolved_member.clone());
        let _ = crate::internal::group_runtime::projection::project_group_snapshot_async(
            self.client,
            &result,
        )
        .await;
        self.refresh_group_state_for_async(&mut result, &group, true)
            .await;
        Ok(result)
    }

    pub fn process_e2ee_leave_request(
        &self,
        request: super::GroupE2eeProcessLeaveRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        #[cfg(not(feature = "group-e2ee"))]
        {
            let _ = request;
            return Err(crate::ImError::unsupported("group-e2ee"));
        }
        #[cfg(feature = "group-e2ee")]
        {
            ensure_group_e2ee_service_available(self.client, false)?;
            let group = request.group.as_str().to_owned();
            let resolved_member = resolve_group_member(self.client, &request.member)?;
            let secure = crate::internal::group_e2ee::lifecycle::GroupE2eeLifecycleRuntime::new(
                self.client,
                crate::internal::auth::session::FileSessionProvider::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
                crate::internal::group_e2ee::storage::native_provider_for_client(self.client)?,
            )
            .process_leave_request(
                crate::internal::group_e2ee::lifecycle::GroupE2eeMemberMutationInput {
                    group: request.group,
                    member: resolved_member.did.clone(),
                    reason_text: request.reason_text,
                    leave_request_id: Some(request.leave_request_id),
                    credentials: None,
                    service_did: None,
                },
            )?;
            let mut result =
                super::GroupReadResult::from_raw_response(secure.delivery, secure.warnings);
            result.resolved_member = Some(resolved_member);
            self.refresh_group_state_for(&mut result, &group, true);
            Ok(result)
        }
    }

    pub async fn process_e2ee_leave_request_async(
        &self,
        request: super::GroupE2eeProcessLeaveRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        #[cfg(not(feature = "group-e2ee"))]
        {
            let _ = request;
            return Err(crate::ImError::unsupported("group-e2ee"));
        }
        #[cfg(feature = "group-e2ee")]
        {
            ensure_group_e2ee_service_available_async(self.client, false).await?;
            let group = request.group.as_str().to_owned();
            let resolved_member = resolve_group_member_async(self.client, &request.member).await?;
            let secure = crate::internal::group_e2ee::lifecycle::GroupE2eeLifecycleRuntime::new(
                self.client,
                crate::internal::auth::session::FileSessionProvider::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
                crate::internal::group_e2ee::storage::native_provider_for_client(self.client)?,
            )
            .process_leave_request_async(
                crate::internal::group_e2ee::lifecycle::GroupE2eeMemberMutationInput {
                    group: request.group,
                    member: resolved_member.did.clone(),
                    reason_text: request.reason_text,
                    leave_request_id: Some(request.leave_request_id),
                    credentials: None,
                    service_did: None,
                },
            )
            .await?;
            let mut result =
                super::GroupReadResult::from_raw_response(secure.delivery, secure.warnings);
            result.resolved_member = Some(resolved_member);
            self.refresh_group_state_for_async(&mut result, &group, true)
                .await;
            Ok(result)
        }
    }

    pub fn update_member_key(
        &self,
        request: super::GroupE2eeUpdateKeyRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        #[cfg(not(feature = "group-e2ee"))]
        {
            let _ = request;
            return Err(crate::ImError::unsupported("group-e2ee"));
        }
        #[cfg(feature = "group-e2ee")]
        {
            ensure_group_e2ee_service_available(self.client, true)?;
            let group = request.group.as_str().to_owned();
            let resolved_member = resolve_group_member(self.client, &request.member)?;
            let secure = crate::internal::group_e2ee::lifecycle::GroupE2eeLifecycleRuntime::new(
                self.client,
                crate::internal::auth::session::FileSessionProvider::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
                crate::internal::group_e2ee::storage::native_provider_for_client(self.client)?,
            )
            .update_member_key(
                crate::internal::group_e2ee::lifecycle::GroupE2eeKeyReplacementInput {
                    group: request.group,
                    member: resolved_member.did.clone(),
                    device_id: request.device_id.unwrap_or_default(),
                    credentials: None,
                    service_did: None,
                },
            )?;
            let mut result =
                super::GroupReadResult::from_raw_response(secure.delivery, secure.warnings);
            result.resolved_member = Some(resolved_member);
            self.refresh_group_state_for(&mut result, &group, true);
            Ok(result)
        }
    }

    pub async fn update_member_key_async(
        &self,
        request: super::GroupE2eeUpdateKeyRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        #[cfg(not(feature = "group-e2ee"))]
        {
            let _ = request;
            return Err(crate::ImError::unsupported("group-e2ee"));
        }
        #[cfg(feature = "group-e2ee")]
        {
            ensure_group_e2ee_service_available_async(self.client, true).await?;
            let group = request.group.as_str().to_owned();
            let resolved_member = resolve_group_member_async(self.client, &request.member).await?;
            let secure = crate::internal::group_e2ee::lifecycle::GroupE2eeLifecycleRuntime::new(
                self.client,
                crate::internal::auth::session::FileSessionProvider::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
                crate::internal::group_e2ee::storage::native_provider_for_client(self.client)?,
            )
            .update_member_key_async(
                crate::internal::group_e2ee::lifecycle::GroupE2eeKeyReplacementInput {
                    group: request.group,
                    member: resolved_member.did.clone(),
                    device_id: request.device_id.unwrap_or_default(),
                    credentials: None,
                    service_did: None,
                },
            )
            .await?;
            let mut result =
                super::GroupReadResult::from_raw_response(secure.delivery, secure.warnings);
            result.resolved_member = Some(resolved_member);
            self.refresh_group_state_for_async(&mut result, &group, true)
                .await;
            Ok(result)
        }
    }

    pub fn recover_member(
        &self,
        request: super::GroupE2eeRecoverMemberRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        #[cfg(not(feature = "group-e2ee"))]
        {
            let _ = request;
            return Err(crate::ImError::unsupported("group-e2ee"));
        }
        #[cfg(feature = "group-e2ee")]
        {
            ensure_group_e2ee_service_available(self.client, true)?;
            let group = request.group.as_str().to_owned();
            let resolved_member = resolve_group_member(self.client, &request.member)?;
            let secure = crate::internal::group_e2ee::lifecycle::GroupE2eeLifecycleRuntime::new(
                self.client,
                crate::internal::auth::session::FileSessionProvider::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
                crate::internal::group_e2ee::storage::native_provider_for_client(self.client)?,
            )
            .recover_member(
                crate::internal::group_e2ee::lifecycle::GroupE2eeKeyReplacementInput {
                    group: request.group,
                    member: resolved_member.did.clone(),
                    device_id: request.device_id.unwrap_or_default(),
                    credentials: None,
                    service_did: None,
                },
            )?;
            let mut result =
                super::GroupReadResult::from_raw_response(secure.delivery, secure.warnings);
            result.resolved_member = Some(resolved_member);
            self.refresh_group_state_for(&mut result, &group, true);
            Ok(result)
        }
    }

    pub async fn recover_member_async(
        &self,
        request: super::GroupE2eeRecoverMemberRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        #[cfg(not(feature = "group-e2ee"))]
        {
            let _ = request;
            return Err(crate::ImError::unsupported("group-e2ee"));
        }
        #[cfg(feature = "group-e2ee")]
        {
            ensure_group_e2ee_service_available_async(self.client, true).await?;
            let group = request.group.as_str().to_owned();
            let resolved_member = resolve_group_member_async(self.client, &request.member).await?;
            let secure = crate::internal::group_e2ee::lifecycle::GroupE2eeLifecycleRuntime::new(
                self.client,
                crate::internal::auth::session::FileSessionProvider::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
                crate::internal::group_e2ee::storage::native_provider_for_client(self.client)?,
            )
            .recover_member_async(
                crate::internal::group_e2ee::lifecycle::GroupE2eeKeyReplacementInput {
                    group: request.group,
                    member: resolved_member.did.clone(),
                    device_id: request.device_id.unwrap_or_default(),
                    credentials: None,
                    service_did: None,
                },
            )
            .await?;
            let mut result =
                super::GroupReadResult::from_raw_response(secure.delivery, secure.warnings);
            result.resolved_member = Some(resolved_member);
            self.refresh_group_state_for_async(&mut result, &group, true)
                .await;
            Ok(result)
        }
    }

    pub fn update_profile(
        &self,
        request: super::GroupUpdateProfileRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let group = request.group.as_str().to_string();
        if let Ok(Some(snapshot)) = crate::internal::group_runtime::cache::cached_group_snapshot(
            self.client,
            request.group.as_str(),
        ) {
            if crate::internal::group_runtime::cache::group_snapshot_uses_e2ee(&snapshot) {
                return Err(crate::ImError::unsupported("group-e2ee"));
            }
        }
        let mut result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .update_profile(request, None)?;
        crate::internal::group_runtime::projection::project_group_snapshot(self.client, &result);
        self.refresh_group_state_for(&mut result, &group, false);
        Ok(result)
    }

    pub async fn update_profile_async(
        &self,
        request: super::GroupUpdateProfileRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let group = request.group.as_str().to_string();
        if let Ok(Some(snapshot)) =
            crate::internal::group_runtime::cache::cached_group_snapshot_async(
                self.client,
                request.group.as_str(),
            )
            .await
        {
            if crate::internal::group_runtime::cache::group_snapshot_uses_e2ee(&snapshot) {
                return Err(crate::ImError::unsupported("group-e2ee"));
            }
        }
        let mut result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .update_profile_async(request, None)
        .await?;
        let _ = crate::internal::group_runtime::projection::project_group_snapshot_async(
            self.client,
            &result,
        )
        .await;
        self.refresh_group_state_for_async(&mut result, &group, false)
            .await;
        Ok(result)
    }

    pub fn update_policy(
        &self,
        request: super::GroupUpdatePolicyRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let group = request.group.as_str().to_string();
        if let Ok(Some(snapshot)) = crate::internal::group_runtime::cache::cached_group_snapshot(
            self.client,
            request.group.as_str(),
        ) {
            if crate::internal::group_runtime::cache::group_snapshot_uses_e2ee(&snapshot) {
                return Err(crate::ImError::unsupported("group-e2ee"));
            }
        }
        let mut result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .update_policy(request, None)?;
        crate::internal::group_runtime::projection::project_group_snapshot(self.client, &result);
        self.refresh_group_state_for(&mut result, &group, false);
        Ok(result)
    }

    pub async fn update_policy_async(
        &self,
        request: super::GroupUpdatePolicyRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let group = request.group.as_str().to_string();
        if let Ok(Some(snapshot)) =
            crate::internal::group_runtime::cache::cached_group_snapshot_async(
                self.client,
                request.group.as_str(),
            )
            .await
        {
            if crate::internal::group_runtime::cache::group_snapshot_uses_e2ee(&snapshot) {
                return Err(crate::ImError::unsupported("group-e2ee"));
            }
        }
        let mut result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .update_policy_async(request, None)
        .await?;
        let _ = crate::internal::group_runtime::projection::project_group_snapshot_async(
            self.client,
            &result,
        )
        .await;
        self.refresh_group_state_for_async(&mut result, &group, false)
            .await;
        Ok(result)
    }

    pub fn update(
        &self,
        request: super::GroupUpdateRequest,
    ) -> crate::ImResult<super::GroupUpdateResult> {
        if request.profile_patch == super::GroupProfilePatch::default()
            && request.policy_patch == super::GroupPolicyPatch::default()
        {
            return Err(crate::ImError::invalid_input(
                Some("group_update".to_string()),
                "group update requires at least one mutable field",
            ));
        }
        if let Ok(Some(snapshot)) = crate::internal::group_runtime::cache::cached_group_snapshot(
            self.client,
            request.group.as_str(),
        ) {
            if crate::internal::group_runtime::cache::group_snapshot_uses_e2ee(&snapshot) {
                return Err(crate::ImError::unsupported("group-e2ee"));
            }
        }
        let group = request.group;
        let mut deliveries = Vec::new();
        let mut warnings = Vec::new();
        if request.profile_patch != super::GroupProfilePatch::default() {
            let result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
                self.client,
                crate::internal::auth::session::FileSessionProvider::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
            )
            .update_profile(
                super::GroupUpdateProfileRequest {
                    group: group.clone(),
                    patch: request.profile_patch,
                },
                None,
            )?;
            crate::internal::group_runtime::projection::project_group_snapshot(
                self.client,
                &result,
            );
            warnings.extend(result.warnings.iter().cloned());
            deliveries.push(result);
        }
        if request.policy_patch != super::GroupPolicyPatch::default() {
            let result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
                self.client,
                crate::internal::auth::session::FileSessionProvider::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
            )
            .update_policy(
                super::GroupUpdatePolicyRequest {
                    group: group.clone(),
                    patch: request.policy_patch,
                },
                None,
            )?;
            crate::internal::group_runtime::projection::project_group_snapshot(
                self.client,
                &result,
            );
            warnings.extend(result.warnings.iter().cloned());
            deliveries.push(result);
        }
        let mut refreshed = match self.get(group.clone()) {
            Ok(result) => Some(result),
            Err(err) => {
                warnings.push(format!("Failed to refresh group snapshot: {err}"));
                None
            }
        };
        if let Some(ref mut refreshed) = refreshed {
            warnings.extend(refreshed.warnings.iter().cloned());
        }
        Ok(super::GroupUpdateResult {
            deliveries,
            refreshed,
            warnings,
        })
    }

    pub async fn update_async(
        &self,
        request: super::GroupUpdateRequest,
    ) -> crate::ImResult<super::GroupUpdateResult> {
        if request.profile_patch == super::GroupProfilePatch::default()
            && request.policy_patch == super::GroupPolicyPatch::default()
        {
            return Err(crate::ImError::invalid_input(
                Some("group_update".to_string()),
                "group update requires at least one mutable field",
            ));
        }
        if let Ok(Some(snapshot)) =
            crate::internal::group_runtime::cache::cached_group_snapshot_async(
                self.client,
                request.group.as_str(),
            )
            .await
        {
            if crate::internal::group_runtime::cache::group_snapshot_uses_e2ee(&snapshot) {
                return Err(crate::ImError::unsupported("group-e2ee"));
            }
        }
        let group = request.group;
        let mut deliveries = Vec::new();
        let mut warnings = Vec::new();
        if request.profile_patch != super::GroupProfilePatch::default() {
            let result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
                self.client,
                crate::internal::auth::session::FileSessionProvider::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
            )
            .update_profile_async(
                super::GroupUpdateProfileRequest {
                    group: group.clone(),
                    patch: request.profile_patch,
                },
                None,
            )
            .await?;
            let _ = crate::internal::group_runtime::projection::project_group_snapshot_async(
                self.client,
                &result,
            )
            .await;
            warnings.extend(result.warnings.iter().cloned());
            deliveries.push(result);
        }
        if request.policy_patch != super::GroupPolicyPatch::default() {
            let result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
                self.client,
                crate::internal::auth::session::FileSessionProvider::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
            )
            .update_policy_async(
                super::GroupUpdatePolicyRequest {
                    group: group.clone(),
                    patch: request.policy_patch,
                },
                None,
            )
            .await?;
            let _ = crate::internal::group_runtime::projection::project_group_snapshot_async(
                self.client,
                &result,
            )
            .await;
            warnings.extend(result.warnings.iter().cloned());
            deliveries.push(result);
        }
        let mut refreshed = match self.get_async(group.clone()).await {
            Ok(result) => Some(result),
            Err(err) => {
                warnings.push(format!("Failed to refresh group snapshot: {err}"));
                None
            }
        };
        if let Some(ref mut refreshed) = refreshed {
            warnings.extend(refreshed.warnings.iter().cloned());
        }
        Ok(super::GroupUpdateResult {
            deliveries,
            refreshed,
            warnings,
        })
    }

    pub fn get(&self, group: crate::ids::GroupRef) -> crate::ImResult<super::GroupReadResult> {
        let result = crate::internal::group_runtime::read::GroupReadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .get(group)?;
        crate::internal::group_runtime::projection::project_group_snapshot(self.client, &result);
        Ok(result)
    }

    pub async fn get_async(
        &self,
        group: crate::ids::GroupRef,
    ) -> crate::ImResult<super::GroupReadResult> {
        let result = crate::internal::group_runtime::read::GroupReadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .get_async(group)
        .await?;
        let _ = crate::internal::group_runtime::projection::project_group_snapshot_async(
            self.client,
            &result,
        )
        .await;
        Ok(result)
    }

    pub fn list(
        &self,
        request: super::GroupListRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let result = crate::internal::group_runtime::read::GroupReadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .list(request)?;
        crate::internal::group_runtime::projection::project_group_summaries(self.client, &result);
        Ok(result)
    }

    pub async fn list_async(
        &self,
        request: super::GroupListRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let result = crate::internal::group_runtime::read::GroupReadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .list_async(request)
        .await?;
        let _ = crate::internal::group_runtime::projection::project_group_summaries_async(
            self.client,
            &result,
        )
        .await;
        Ok(result)
    }

    pub fn members(
        &self,
        request: super::GroupMembersRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let group = request.group.as_str().to_string();
        let result = crate::internal::group_runtime::read::GroupReadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .members(request)?;
        crate::internal::group_runtime::projection::project_group_members(
            self.client,
            &group,
            &result,
        );
        Ok(result)
    }

    pub async fn members_async(
        &self,
        request: super::GroupMembersRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let group = request.group.as_str().to_string();
        let result = crate::internal::group_runtime::read::GroupReadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .members_async(request)
        .await?;
        let _ = crate::internal::group_runtime::projection::project_group_members_async(
            self.client,
            &group,
            &result,
        )
        .await;
        Ok(result)
    }

    pub fn messages(
        &self,
        request: super::GroupMessagesRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let group = request.group.as_str().to_string();
        let result = crate::internal::group_runtime::read::GroupReadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .messages(request)?;
        crate::internal::group_runtime::projection::project_group_messages(
            self.client,
            &group,
            &result,
        );
        let _ = crate::internal::message_runtime::local_projection::persist_messages(
            self.client,
            &result.messages.items,
        );
        Ok(result)
    }

    pub async fn messages_async(
        &self,
        request: super::GroupMessagesRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let group = request.group.as_str().to_string();
        let result = crate::internal::group_runtime::read::GroupReadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .messages_async(request)
        .await?;
        let _ = crate::internal::group_runtime::projection::project_group_messages_async(
            self.client,
            &group,
            &result,
        )
        .await;
        let _ = crate::internal::message_runtime::local_projection::persist_messages_async(
            self.client,
            &result.messages.items,
        )
        .await;
        Ok(result)
    }

    fn refresh_group_state(&self, result: &mut super::GroupReadResult, include_members: bool) {
        let Some(group) = group_did(result) else {
            return;
        };
        self.refresh_group_state_for(result, &group, include_members);
    }

    fn refresh_group_state_for(
        &self,
        result: &mut super::GroupReadResult,
        group: &str,
        include_members: bool,
    ) {
        let group_ref = match crate::ids::GroupRef::parse(group) {
            Ok(group_ref) => group_ref,
            Err(err) => {
                result.push_warning(format!("Failed to refresh group snapshot: {err}"));
                return;
            }
        };
        match self.get(group_ref.clone()) {
            Ok(snapshot) => result.merge_group_snapshot_from(&snapshot),
            Err(err) => {
                result.push_warning(format!("Failed to refresh group snapshot: {err}"));
                return;
            }
        }
        if !include_members {
            return;
        }
        match self.members(super::GroupMembersRequest {
            group: group_ref,
            limit: crate::ids::PageLimit(100),
        }) {
            Ok(members) => result.merge_group_members_from(&members),
            Err(err) => result.push_warning(format!("Failed to refresh group members: {err}")),
        }
    }

    async fn refresh_group_state_async(
        &self,
        result: &mut super::GroupReadResult,
        include_members: bool,
    ) {
        let Some(group) = group_did(result) else {
            return;
        };
        self.refresh_group_state_for_async(result, &group, include_members)
            .await;
    }

    async fn refresh_group_state_for_async(
        &self,
        result: &mut super::GroupReadResult,
        group: &str,
        include_members: bool,
    ) {
        let group_ref = match crate::ids::GroupRef::parse(group) {
            Ok(group_ref) => group_ref,
            Err(err) => {
                result.push_warning(format!("Failed to refresh group snapshot: {err}"));
                return;
            }
        };
        match self.get_async(group_ref.clone()).await {
            Ok(snapshot) => result.merge_group_snapshot_from(&snapshot),
            Err(err) => {
                result.push_warning(format!("Failed to refresh group snapshot: {err}"));
                return;
            }
        }
        if !include_members {
            return;
        }
        match self
            .members_async(super::GroupMembersRequest {
                group: group_ref,
                limit: crate::ids::PageLimit(100),
            })
            .await
        {
            Ok(members) => result.merge_group_members_from(&members),
            Err(err) => result.push_warning(format!("Failed to refresh group members: {err}")),
        }
    }

    #[cfg(feature = "group-e2ee")]
    fn publish_key_package_with_group_e2ee(
        &self,
        request: super::GroupKeyPackagePublishRequest,
    ) -> crate::ImResult<super::GroupKeyPackagePublishResult> {
        let session_provider =
            crate::internal::auth::session::FileSessionProvider::new(self.client);
        <crate::internal::auth::session::FileSessionProvider<'_> as SessionProvider>::ensure_session(
            &session_provider,
            crate::auth::AuthScope::GroupMessaging,
        )?;
        let credentials = crate::internal::message_runtime::group::load_credentials(self.client)?;
        let service_did = group_e2ee_service_did(self.client)?;
        let provider =
            crate::internal::group_e2ee::storage::native_provider_for_client(self.client)?;
        let prepared = prepare_group_key_package_publish(
            self.client,
            &request,
            &credentials,
            &service_did,
            &provider,
        )?;
        let mut transport = crate::internal::transport::CoreHttpTransport::new(self.client);
        let raw_response =
            <crate::internal::transport::CoreHttpTransport<'_> as AuthenticatedRpcTransport>::authenticated_rpc(
                &mut transport,
                crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT,
                "group.e2ee.publish_key_package",
                prepared.params.clone(),
            )?;
        Ok(group_key_package_publish_result(
            self.client,
            request,
            prepared,
            raw_response,
        ))
    }

    #[cfg(feature = "group-e2ee")]
    async fn publish_key_package_with_group_e2ee_async(
        &self,
        request: super::GroupKeyPackagePublishRequest,
    ) -> crate::ImResult<super::GroupKeyPackagePublishResult> {
        let session_provider =
            crate::internal::auth::session::FileSessionProvider::new(self.client);
        <crate::internal::auth::session::FileSessionProvider<'_> as AsyncSessionProvider>::ensure_session(
            &session_provider,
            crate::auth::AuthScope::GroupMessaging,
        )
            .await?;
        let credentials =
            crate::internal::message_runtime::group::load_credentials_async(self.client).await?;
        let service_did = group_e2ee_service_did(self.client)?;
        let provider =
            crate::internal::group_e2ee::storage::native_provider_for_client(self.client)?;
        let transport = crate::internal::transport::CoreHttpTransport::new(self.client);
        publish_group_key_package_with_components(
            self.client,
            request,
            credentials,
            service_did,
            provider,
            transport,
        )
        .await
    }
}

#[cfg(feature = "group-e2ee")]
struct PreparedGroupKeyPackagePublish {
    device_id: String,
    key_package_id: String,
    params: serde_json::Value,
}

#[cfg(feature = "group-e2ee")]
fn prepare_group_key_package_publish<P>(
    client: &crate::core::ImClient,
    request: &super::GroupKeyPackagePublishRequest,
    credentials: &crate::internal::message_runtime::group::GroupTextCredentials,
    service_did: &crate::ids::Did,
    provider: &P,
) -> crate::ImResult<PreparedGroupKeyPackagePublish>
where
    P: GroupMlsProvider + ?Sized,
{
    let operation_id = format!(
        "op-{}",
        crate::internal::wire::common::generate_operation_id()
    );
    let device_id = request
        .device_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(crate::internal::group_e2ee::DEFAULT_GROUP_MLS_DEVICE_ID)
        .to_owned();
    let mut group_key_package = provider
        .generate_key_package(anp::group_e2ee::operations::GenerateKeyPackageInput {
            owner_did: client.did().as_str().to_owned(),
            device_id: device_id.clone(),
            operation_id: operation_id.clone(),
            request_id: format!("group-e2ee-key-package-{operation_id}"),
            key_package_id: request
                .key_package_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned),
            purpose: Some(request.purpose.as_str().to_owned()),
            group_did: request
                .group
                .as_ref()
                .map(|group| group.as_str().to_owned()),
        })?
        .group_key_package;
    group_key_package.did_wba_binding = sign_group_key_package_binding(
        credentials,
        client.did().as_str(),
        group_key_package.did_wba_binding,
    )?;
    let key_package_id = group_key_package.key_package_id.clone();
    let params =
        crate::internal::group_e2ee::wire::build_group_e2ee_publish_key_package_rpc_params(
            credentials,
            client.did().as_str(),
            service_did.as_str(),
            &group_key_package,
            &operation_id,
        )?;
    Ok(PreparedGroupKeyPackagePublish {
        device_id,
        key_package_id,
        params,
    })
}

#[cfg(feature = "group-e2ee")]
fn group_key_package_publish_result(
    client: &crate::core::ImClient,
    request: super::GroupKeyPackagePublishRequest,
    prepared: PreparedGroupKeyPackagePublish,
    raw_response: serde_json::Value,
) -> super::GroupKeyPackagePublishResult {
    super::GroupKeyPackagePublishResult {
        owner_did: client.did().clone(),
        device_id: prepared.device_id,
        key_package_id: prepared.key_package_id,
        purpose: request.purpose,
        group: request.group,
        raw_response,
        warnings: Vec::new(),
    }
}

#[cfg(feature = "group-e2ee")]
async fn publish_group_key_package_with_components<P, T>(
    client: &crate::core::ImClient,
    request: super::GroupKeyPackagePublishRequest,
    credentials: crate::internal::message_runtime::group::GroupTextCredentials,
    service_did: crate::ids::Did,
    provider: P,
    mut transport: T,
) -> crate::ImResult<super::GroupKeyPackagePublishResult>
where
    P: GroupMlsProvider + Send + 'static,
    T: AsyncAuthenticatedRpcTransport,
{
    let client_did = client.did().as_str().to_owned();
    let prepared = prepare_group_key_package_publish_async(
        client_did.as_str(),
        &request,
        &credentials,
        &service_did,
        provider,
    )
    .await?;
    let raw_response = transport
        .authenticated_rpc(
            crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT,
            "group.e2ee.publish_key_package",
            prepared.params.clone(),
        )
        .await?;
    Ok(group_key_package_publish_result(
        client,
        request,
        prepared,
        raw_response,
    ))
}

#[cfg(feature = "group-e2ee")]
async fn prepare_group_key_package_publish_async<P>(
    owner_did: &str,
    request: &super::GroupKeyPackagePublishRequest,
    credentials: &crate::internal::message_runtime::group::GroupTextCredentials,
    service_did: &crate::ids::Did,
    provider: P,
) -> crate::ImResult<PreparedGroupKeyPackagePublish>
where
    P: GroupMlsProvider + Send + 'static,
{
    let operation_id = format!(
        "op-{}",
        crate::internal::wire::common::generate_operation_id()
    );
    let device_id = request
        .device_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(crate::internal::group_e2ee::DEFAULT_GROUP_MLS_DEVICE_ID)
        .to_owned();
    let input = anp::group_e2ee::operations::GenerateKeyPackageInput {
        owner_did: owner_did.to_owned(),
        device_id: device_id.clone(),
        operation_id: operation_id.clone(),
        request_id: format!("group-e2ee-key-package-{operation_id}"),
        key_package_id: request
            .key_package_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned),
        purpose: Some(request.purpose.as_str().to_owned()),
        group_did: request
            .group
            .as_ref()
            .map(|group| group.as_str().to_owned()),
    };
    let mut group_key_package = crate::internal::runtime::worker::run_blocking(move || {
        provider
            .generate_key_package(input)
            .map(|output| output.group_key_package)
    })
    .await
    .map_err(|err| crate::ImError::Internal {
        message: format!("group E2EE key package generation worker failed: {err}"),
    })??;
    group_key_package.did_wba_binding =
        sign_group_key_package_binding(credentials, owner_did, group_key_package.did_wba_binding)?;
    let key_package_id = group_key_package.key_package_id.clone();
    let params =
        crate::internal::group_e2ee::wire::build_group_e2ee_publish_key_package_rpc_params(
            credentials,
            owner_did,
            service_did.as_str(),
            &group_key_package,
            &operation_id,
        )?;
    Ok(PreparedGroupKeyPackagePublish {
        device_id,
        key_package_id,
        params,
    })
}

fn group_did(result: &super::GroupReadResult) -> Option<String> {
    result
        .group
        .as_ref()
        .map(|group| group.did.as_str().to_string())
        .or_else(|| {
            result
                .response_json()
                .and_then(|raw| raw.get("group_did").or_else(|| raw.get("did")))
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
}

fn resolve_group_member(
    client: &crate::core::ImClient,
    member: &super::GroupMemberRef,
) -> crate::ImResult<super::GroupMemberResolution> {
    if member.is_did() {
        return Ok(super::GroupMemberResolution {
            did: member.as_did()?,
            handle: None,
        });
    }
    let handle = crate::ids::Handle::parse(member.as_str(), "")?;
    let lookup = client.directory().lookup_handle(handle)?;
    Ok(super::GroupMemberResolution {
        did: lookup.did,
        handle: Some(lookup.handle),
    })
}

async fn resolve_group_member_async(
    client: &crate::core::ImClient,
    member: &super::GroupMemberRef,
) -> crate::ImResult<super::GroupMemberResolution> {
    if member.is_did() {
        return Ok(super::GroupMemberResolution {
            did: member.as_did()?,
            handle: None,
        });
    }
    let handle = crate::ids::Handle::parse(member.as_str(), "")?;
    let lookup = client.directory().lookup_handle_async(handle).await?;
    Ok(super::GroupMemberResolution {
        did: lookup.did,
        handle: Some(lookup.handle),
    })
}

fn resolved_group_member_request(
    request: super::GroupMemberMutationRequest,
    member: &super::GroupMemberResolution,
) -> super::GroupMemberMutationRequest {
    super::GroupMemberMutationRequest {
        group: request.group,
        member: super::GroupMemberRef::from(member.did.clone()),
        role: request.role,
        reason_text: request.reason_text,
        leave_request_id: request.leave_request_id,
        security: request.security,
    }
}

fn group_create_uses_e2ee(request: &super::GroupCreateRequest) -> bool {
    request.security.required()
        || request.e2ee
        || matches!(
            request.message_security_profile,
            Some(super::GroupMessageSecurityProfile::GroupE2ee)
        )
        || matches!(
            request.message_security_profile.as_ref(),
            Some(super::GroupMessageSecurityProfile::Custom(value))
                if value.trim() == "group-e2ee"
        )
}

#[cfg(feature = "group-e2ee")]
fn sign_group_key_package_binding(
    credentials: &crate::internal::message_runtime::group::GroupTextCredentials,
    owner_did: &str,
    binding: serde_json::Value,
) -> crate::ImResult<serde_json::Value> {
    let leaf_signature_key_b64u = binding
        .get("leaf_signature_key_b64u")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| crate::ImError::Serialization {
            detail: "group KeyPackage binding is missing leaf_signature_key_b64u".to_owned(),
        })?;
    let issued_at = binding
        .get("issued_at")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| crate::ImError::Serialization {
            detail: "group KeyPackage binding is missing issued_at".to_owned(),
        })?;
    let expires_at = binding
        .get("expires_at")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| crate::ImError::Serialization {
            detail: "group KeyPackage binding is missing expires_at".to_owned(),
        })?;
    let did_document =
        credentials
            .did_document
            .as_ref()
            .ok_or_else(|| crate::ImError::Serialization {
                detail: "identity is missing a DID document for group KeyPackage binding"
                    .to_owned(),
            })?;
    let verification_method = assertion_verification_method_id_from_document(did_document)
        .ok_or_else(|| crate::ImError::Serialization {
            detail: "identity is missing an assertion verification method".to_owned(),
        })?;
    let private_key =
        crate::internal::proof::origin::load_private_key_material(&credentials.key1_private_pem)?;
    anp::proof::generate_did_wba_binding(
        owner_did,
        &verification_method,
        leaf_signature_key_b64u,
        &private_key,
        issued_at,
        expires_at,
        Some(issued_at.to_owned()),
    )
    .map_err(|err| crate::ImError::Serialization {
        detail: format!("generate group KeyPackage DID WBA binding proof: {err}"),
    })
}

#[cfg(feature = "group-e2ee")]
fn assertion_verification_method_id_from_document(
    did_document: &serde_json::Value,
) -> Option<String> {
    did_document
        .get("assertionMethod")
        .and_then(serde_json::Value::as_array)
        .and_then(|methods| methods.first())
        .and_then(|method| {
            method
                .as_str()
                .or_else(|| method.get("id").and_then(serde_json::Value::as_str))
        })
        .map(str::to_owned)
        .or_else(|| {
            crate::internal::proof::origin::verification_method_id_from_document(did_document)
        })
}

#[cfg(feature = "group-e2ee")]
fn ensure_group_e2ee_service_available(
    client: &crate::core::ImClient,
    check_key_package: bool,
) -> crate::ImResult<()> {
    let session_provider = crate::internal::auth::session::FileSessionProvider::new(client);
    let mut transport = crate::internal::transport::CoreHttpTransport::new(client);
    crate::internal::group_e2ee::lifecycle::ensure_group_e2ee_service_available(
        client,
        &session_provider,
        &mut transport,
        crate::internal::group_e2ee::lifecycle::GroupE2eeServiceAvailabilityInput {
            credentials: None,
            service_did: None,
            check_key_package,
        },
    )
}

#[cfg(feature = "group-e2ee")]
async fn ensure_group_e2ee_service_available_async(
    client: &crate::core::ImClient,
    check_key_package: bool,
) -> crate::ImResult<()> {
    let session_provider = crate::internal::auth::session::FileSessionProvider::new(client);
    let mut transport = crate::internal::transport::CoreHttpTransport::new(client);
    crate::internal::group_e2ee::lifecycle::ensure_group_e2ee_service_available_async(
        client,
        &session_provider,
        &mut transport,
        crate::internal::group_e2ee::lifecycle::GroupE2eeServiceAvailabilityInput {
            credentials: None,
            service_did: None,
            check_key_package,
        },
    )
    .await
}

#[cfg(feature = "group-e2ee")]
fn group_e2ee_service_did(client: &crate::core::ImClient) -> crate::ImResult<crate::ids::Did> {
    client
        .core_inner()
        .sdk_config()
        .anp_service_did
        .clone()
        .ok_or_else(|| {
            crate::ImError::invalid_input(
                Some("anp_service_did".to_owned()),
                "group E2EE key package publish requires ImCoreConfig.anp_service_did",
            )
        })
}

#[cfg(all(test, feature = "group-e2ee"))]
mod tests {
    use super::*;
    use crate::internal::group_e2ee::provider::GroupMlsProvider;
    use crate::internal::transport::AsyncAuthenticatedRpcTransport;
    use anp::group_e2ee::operations::{
        AbortCommitInput, AbortCommitOutput, AddMemberInput, CreateGroupInput, DecryptInput,
        DecryptOutput, EncryptInput, EncryptOutput, FinalizeCommitInput, FinalizeCommitOutput,
        GenerateKeyPackageInput, GroupKeyPackageOutput, LeaveGroupInput, PreparedMlsCommitOutput,
        ProcessNoticeInput, ProcessNoticeOutput, ProcessWelcomeInput, ProcessWelcomeOutput,
        RecoverMemberInput, RemoveMemberInput, StatusInput, StatusOutput, UpdateMemberInput,
    };
    use serde_json::{json, Value};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn publish_key_package_async_helper_uses_async_transport() {
        let fixture = Fixture::new();
        let owner_did = fixture.did_bundle.did().unwrap().to_owned();
        let client = fixture.client();
        let credentials = fixture.credentials();
        let provider = RecordingKeyPackageProvider::default();
        let generated = Arc::clone(&provider.generated);
        let calls = Arc::new(Mutex::new(Vec::new()));
        let result = publish_group_key_package_with_components(
            &client,
            crate::groups::GroupKeyPackagePublishRequest {
                purpose: crate::groups::GroupKeyPackagePurpose::Recovery,
                group: Some(crate::ids::GroupRef::parse("did:example:groups:secure").unwrap()),
                device_id: Some(" device-a ".to_owned()),
                key_package_id: Some(" kp-explicit ".to_owned()),
            },
            credentials,
            crate::ids::Did::parse("did:example:service").unwrap(),
            provider,
            RecordingAsyncTransport {
                calls: Arc::clone(&calls),
                response: json!({"accepted": true}),
            },
        )
        .await
        .unwrap();

        assert_eq!(result.owner_did.as_str(), owner_did);
        assert_eq!(result.device_id, "device-a");
        assert_eq!(result.key_package_id, "kp-explicit");
        assert_eq!(
            result.purpose,
            crate::groups::GroupKeyPackagePurpose::Recovery
        );
        assert_eq!(
            result.group.as_ref().map(|group| group.as_str()),
            Some("did:example:groups:secure")
        );
        assert_eq!(result.raw_response, json!({"accepted": true}));

        let generated = generated.lock().unwrap();
        assert_eq!(generated.len(), 1);
        assert_eq!(generated[0].owner_did, owner_did);
        assert_eq!(generated[0].device_id, "device-a");
        assert_eq!(generated[0].key_package_id.as_deref(), Some("kp-explicit"));
        assert_eq!(generated[0].purpose.as_deref(), Some("recovery"));
        assert_eq!(
            generated[0].group_did.as_deref(),
            Some("did:example:groups:secure")
        );

        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].endpoint,
            crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT
        );
        assert_eq!(calls[0].method, "group.e2ee.publish_key_package");
        assert_eq!(
            calls[0].params["meta"]["target"],
            json!({"kind": "service", "did": "did:example:service"})
        );
        assert_eq!(
            calls[0].params["body"]["group_key_package"]["key_package_id"],
            "kp-explicit"
        );
        assert_eq!(
            calls[0].params["body"]["group_key_package"]["device_id"],
            "device-a"
        );
        assert_eq!(
            calls[0].params["body"]["group_key_package"]["purpose"],
            "recovery"
        );
        assert_eq!(
            calls[0].params["body"]["group_key_package"]["group_did"],
            "did:example:groups:secure"
        );
        assert!(
            calls[0].params["body"]["group_key_package"]["did_wba_binding"]["proof"].is_object()
        );
        assert!(calls[0].params["auth"]["origin_proof"].is_object());
    }

    #[derive(Default)]
    struct RecordingKeyPackageProvider {
        generated: Arc<Mutex<Vec<GenerateKeyPackageInput>>>,
    }

    impl GroupMlsProvider for RecordingKeyPackageProvider {
        fn generate_key_package(
            &self,
            input: GenerateKeyPackageInput,
        ) -> crate::ImResult<GroupKeyPackageOutput> {
            self.generated.lock().unwrap().push(input.clone());
            let owner_did = input.owner_did.clone();
            Ok(GroupKeyPackageOutput {
                group_key_package: anp::group_e2ee::GroupKeyPackage {
                    key_package_id: input
                        .key_package_id
                        .clone()
                        .unwrap_or_else(|| "kp-generated".to_owned()),
                    owner_did: owner_did.clone(),
                    device_id: Some(input.device_id),
                    purpose: input.purpose,
                    group_did: input.group_did,
                    suite: anp::group_e2ee::MTI_SUITE.to_owned(),
                    mls_key_package_b64u: "mls-key-package".to_owned(),
                    did_wba_binding: json!({
                        "agent_did": owner_did,
                        "verification_method": format!("{owner_did}#key-1"),
                        "leaf_signature_key_b64u": "leaf-key",
                        "issued_at": "2026-01-01T00:00:00Z",
                        "expires_at": "2099-01-01T00:00:00Z"
                    }),
                    expires_at: None,
                    non_cryptographic: false,
                    artifact_mode: None,
                },
            })
        }

        fn create_group_prepare(
            &self,
            _input: CreateGroupInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("publish key package should not create groups")
        }

        fn add_member_prepare(
            &self,
            _input: AddMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("publish key package should not add members")
        }

        fn remove_member_prepare(
            &self,
            _input: RemoveMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("publish key package should not remove members")
        }

        fn leave_prepare(
            &self,
            _input: LeaveGroupInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("publish key package should not leave groups")
        }

        fn update_member_prepare(
            &self,
            _input: UpdateMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("publish key package should not update members")
        }

        fn recover_member_prepare(
            &self,
            _input: RecoverMemberInput,
        ) -> crate::ImResult<PreparedMlsCommitOutput> {
            unreachable!("publish key package should not recover members")
        }

        fn finalize_commit(
            &self,
            _input: FinalizeCommitInput,
        ) -> crate::ImResult<FinalizeCommitOutput> {
            unreachable!("publish key package should not finalize commits")
        }

        fn abort_commit(&self, _input: AbortCommitInput) -> crate::ImResult<AbortCommitOutput> {
            unreachable!("publish key package should not abort commits")
        }

        fn process_welcome(
            &self,
            _input: ProcessWelcomeInput,
        ) -> crate::ImResult<ProcessWelcomeOutput> {
            unreachable!("publish key package should not process welcomes")
        }

        fn process_notice(
            &self,
            _input: ProcessNoticeInput,
        ) -> crate::ImResult<ProcessNoticeOutput> {
            unreachable!("publish key package should not process notices")
        }

        fn encrypt(&self, _input: EncryptInput) -> crate::ImResult<EncryptOutput> {
            unreachable!("publish key package should not encrypt")
        }

        fn decrypt(&self, _input: DecryptInput) -> crate::ImResult<DecryptOutput> {
            unreachable!("publish key package should not decrypt")
        }

        fn status(&self, _input: StatusInput) -> crate::ImResult<StatusOutput> {
            unreachable!("publish key package should not read status")
        }
    }

    struct RecordingAsyncTransport {
        calls: Arc<Mutex<Vec<RecordedCall>>>,
        response: Value,
    }

    impl AsyncAuthenticatedRpcTransport for RecordingAsyncTransport {
        async fn authenticated_rpc(
            &mut self,
            endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            self.calls.lock().unwrap().push(RecordedCall {
                endpoint: endpoint.to_owned(),
                method: method.to_owned(),
                params,
            });
            Ok(self.response.clone())
        }
    }

    struct RecordedCall {
        endpoint: String,
        method: String,
        params: Value,
    }

    struct Fixture {
        root: PathBuf,
        did_bundle: anp::authentication::DidDocumentBundle,
    }

    impl Fixture {
        fn new() -> Self {
            let root = unique_temp_root();
            std::fs::create_dir_all(root.join("identities").join("alice")).unwrap();
            Self {
                root,
                did_bundle: test_did_bundle(),
            }
        }

        fn client(&self) -> crate::core::ImClient {
            crate::core::ImCore::new(
                crate::ImCoreConfig {
                    service_base_url: crate::ServiceEndpoint::parse("https://example.test")
                        .unwrap(),
                    did_domain: "example.test".to_owned(),
                    user_service_endpoint: None,
                    message_service_endpoint: None,
                    mail_service_endpoint: None,
                    anp_service_endpoint: None,
                    anp_service_did: Some(crate::ids::Did::parse("did:example:service").unwrap()),
                    ca_bundle: None,
                    transport_policy: crate::MessageTransportPolicy::HttpOnly,
                },
                crate::ImCorePaths {
                    identities: crate::paths::IdentityRegistryPaths {
                        identity_root_dir: self.root.join("identities"),
                        registry_path: self.root.join("identities").join("registry.json"),
                        default_identity_path: Some(self.root.join("identities").join("default")),
                    },
                    local_state: crate::paths::LocalStatePaths {
                        sqlite_path: self.root.join("local").join("im.sqlite"),
                    },
                    runtime: crate::paths::RuntimePaths {
                        cache_dir: self.root.join("cache"),
                        temp_dir: self.root.join("tmp"),
                    },
                },
            )
            .unwrap()
            .client(crate::identity::IdentitySelector::Did(
                crate::ids::Did::parse(self.did_bundle.did().unwrap()).unwrap(),
            ))
            .unwrap()
        }

        fn credentials(&self) -> crate::internal::message_runtime::group::GroupTextCredentials {
            let key1_private_pem = self.did_bundle.private_key_pem("key-1").unwrap().to_owned();
            crate::internal::message_runtime::group::GroupTextCredentials {
                identity_name: "alice".to_owned(),
                did_document: Some(self.did_bundle.did_document.clone()),
                key1_private_pem,
            }
        }
    }

    fn test_did_bundle() -> anp::authentication::DidDocumentBundle {
        anp::authentication::create_did_wba_document(
            "example.test",
            anp::authentication::DidDocumentOptions {
                path_segments: vec!["alice".to_owned()],
                domain: Some("example.test".to_owned()),
                challenge: Some("group-key-package-publish-test".to_owned()),
                ..anp::authentication::DidDocumentOptions::default()
            },
        )
        .unwrap()
    }

    fn unique_temp_root() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "im-core-group-key-package-publish-{}-{nanos}",
            std::process::id()
        ))
    }
}
