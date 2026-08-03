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
            if use_group_e2ee_v2_lifecycle(self.client) {
                crate::internal::group_e2ee::v2_lifecycle::publish_current_key_package(
                    self.client,
                    request,
                )
            } else {
                self.publish_key_package_with_group_e2ee(request)
            }
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
            if use_group_e2ee_v2_lifecycle(self.client) {
                let client = self.client.clone();
                crate::internal::runtime::worker::run_blocking(move || {
                    crate::internal::group_e2ee::v2_lifecycle::publish_current_key_package(
                        &client, request,
                    )
                })
                .await
                .map_err(|err| crate::ImError::Internal {
                    message: format!("P6 v2 KeyPackage publish worker failed: {err}"),
                })?
            } else {
                self.publish_key_package_with_group_e2ee_async(request)
                    .await
            }
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
        let v2_enabled = use_group_e2ee_v2_lifecycle(self.client);
        #[cfg(feature = "group-e2ee")]
        let secure_provider = if secure_required && !v2_enabled {
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
        crate::internal::group_runtime::projection::project_group_snapshot(self.client, &result);
        #[cfg(feature = "group-e2ee")]
        if secure_required {
            let group =
                group_did(&result).ok_or_else(|| crate::ImError::LocalStateUnavailable {
                    detail: "group E2EE create requires created group DID".to_owned(),
                })?;
            let group_state_ref = group_state_ref_from_result(&group, &result);
            if v2_enabled {
                crate::internal::group_e2ee::v2_lifecycle::initialize_created_group(
                    self.client,
                    crate::internal::group_e2ee::v2_lifecycle::required_created_group_state_ref(
                        &group,
                        group_state_ref,
                    )?,
                )?;
            } else {
                let secure =
                    crate::internal::group_e2ee::lifecycle::GroupE2eeLifecycleRuntime::new(
                        self.client,
                        crate::internal::auth::session::FileSessionProvider::new(self.client),
                        crate::internal::transport::CoreHttpTransport::new(self.client),
                        secure_provider.expect("legacy secure provider initialized"),
                    )
                    .create_secure_group(
                        crate::internal::group_e2ee::lifecycle::GroupE2eeCreateInput {
                            group: crate::ids::GroupRef::parse(&group)?,
                            credentials: None,
                            service_did: None,
                            group_state_ref,
                        },
                    )?;
                result.warnings.extend(secure.warnings);
            }
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
        let v2_enabled = use_group_e2ee_v2_lifecycle(self.client);
        #[cfg(feature = "group-e2ee")]
        if secure_required && !v2_enabled {
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
        let _ = crate::internal::group_runtime::projection::project_group_snapshot_async(
            self.client,
            &result,
        )
        .await;
        #[cfg(feature = "group-e2ee")]
        if secure_required {
            let group =
                group_did(&result).ok_or_else(|| crate::ImError::LocalStateUnavailable {
                    detail: "group E2EE create requires created group DID".to_owned(),
                })?;
            let group_state_ref = group_state_ref_from_result(&group, &result);
            if v2_enabled {
                let group_state_ref =
                    crate::internal::group_e2ee::v2_lifecycle::required_created_group_state_ref(
                        &group,
                        group_state_ref,
                    )?;
                let client = self.client.clone();
                crate::internal::runtime::worker::run_blocking(move || {
                    crate::internal::group_e2ee::v2_lifecycle::initialize_created_group(
                        &client,
                        group_state_ref,
                    )
                })
                .await
                .map_err(|err| crate::ImError::Internal {
                    message: format!("P6 v2 group create worker failed: {err}"),
                })??;
            } else {
                let secure =
                    crate::internal::group_e2ee::lifecycle::GroupE2eeLifecycleRuntime::new(
                        self.client,
                        crate::internal::auth::session::FileSessionProvider::new(self.client),
                        crate::internal::transport::CoreHttpTransport::new(self.client),
                        crate::internal::group_e2ee::storage::native_provider_for_client(
                            self.client,
                        )?,
                    )
                    .create_secure_group_async(
                        crate::internal::group_e2ee::lifecycle::GroupE2eeCreateInput {
                            group: crate::ids::GroupRef::parse(&group)?,
                            credentials: None,
                            service_did: None,
                            group_state_ref,
                        },
                    )
                    .await?;
                result.warnings.extend(secure.warnings);
            }
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
        let v2_enabled = use_group_e2ee_v2_lifecycle(self.client);
        #[cfg(feature = "group-e2ee")]
        if v2_enabled {
            let authoritative = self.get_authoritative_group(request.group.clone())?;
            require_v2_leave_safe(&group, &authoritative, request.security.required())?;
            let result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
                self.client,
                crate::internal::auth::session::FileSessionProvider::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
            )
            .leave(request, None)?;
            crate::internal::group_runtime::projection::project_group_left(self.client, &group);
            return Ok(result);
        }
        #[cfg(feature = "group-e2ee")]
        let secure_provider = if request.security.required() && !v2_enabled {
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
        let v2_enabled = use_group_e2ee_v2_lifecycle(self.client);
        #[cfg(feature = "group-e2ee")]
        if v2_enabled {
            let authoritative = self
                .get_authoritative_group_async(request.group.clone())
                .await?;
            require_v2_leave_safe(&group, &authoritative, request.security.required())?;
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
            return Ok(result);
        }
        #[cfg(feature = "group-e2ee")]
        if request.security.required() && !v2_enabled {
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
        #[cfg(feature = "group-e2ee")]
        let secure_required = request.security.required();
        let group = request.group.as_str().to_string();
        #[cfg(feature = "group-e2ee")]
        let requested_member_is_did = request.member.is_did();
        let resolved_member = resolve_group_member(self.client, &request.member)?;
        #[cfg(feature = "group-e2ee")]
        let member = resolved_member.did.as_str().to_owned();
        #[cfg(feature = "group-e2ee")]
        let expected_member_did = requested_member_is_did.then(|| member.clone());
        #[cfg(feature = "group-e2ee")]
        let reason_text = request.reason_text.clone();
        #[cfg(not(feature = "group-e2ee"))]
        if request.security.required() {
            return Err(crate::ImError::unsupported("group-e2ee"));
        }
        #[cfg(feature = "group-e2ee")]
        let v2_enabled = use_group_e2ee_v2_lifecycle(self.client);
        #[cfg(feature = "group-e2ee")]
        let v2_route = if v2_enabled {
            let authoritative = self.get_authoritative_group(request.group.clone())?;
            Some(v2_member_mutation_route(
                &group,
                &authoritative,
                secure_required,
            )?)
        } else {
            None
        };
        #[cfg(feature = "group-e2ee")]
        let use_v2_p6 = v2_route == Some(V2MemberMutationRoute::OwnerP6);
        #[cfg(feature = "group-e2ee")]
        let secure_provider = if secure_required && !v2_enabled {
            ensure_group_e2ee_service_available(self.client, true)?;
            Some(crate::internal::group_e2ee::storage::native_provider_for_client(self.client)?)
        } else {
            None
        };
        #[cfg(feature = "group-e2ee")]
        let use_legacy_cache_guard = !v2_enabled;
        #[cfg(not(feature = "group-e2ee"))]
        let use_legacy_cache_guard = true;
        if use_legacy_cache_guard {
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
        }
        #[cfg(feature = "group-e2ee")]
        if use_v2_p6 {
            crate::internal::group_e2ee::v2_lifecycle::preflight_current_controller(
                self.client,
                &group,
            )?;
        }
        #[cfg(feature = "group-e2ee")]
        let request = p4_member_mutation_request(request, use_v2_p6);
        let p4_result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .add_member(request, None);
        #[cfg(feature = "group-e2ee")]
        let mut result = match p4_result {
            Ok(result) => result,
            Err(error) if use_v2_p6 && group_error_is_already_member(&error) => {
                let authoritative_member = self.authoritative_active_member(
                    crate::ids::GroupRef::parse(&group)?,
                    &resolved_member,
                )?
                .ok_or_else(|| crate::ImError::LocalStateUnavailable {
                    detail: "group.add reported an existing member but the authoritative roster did not identify it"
                        .to_owned(),
                })?;
                let authoritative =
                    self.get_authoritative_group(crate::ids::GroupRef::parse(&group)?)?;
                let state_ref =
                    group_state_ref_from_result(&group, &authoritative).ok_or_else(|| {
                        crate::ImError::LocalStateUnavailable {
                            detail: "authoritative group.get omitted group_state_ref".to_owned(),
                        }
                    })?;
                crate::internal::group_e2ee::v2_lifecycle::add_active_member_devices(
                    self.client,
                    crate::internal::group_e2ee::v2_lifecycle::v2_group_state_ref(state_ref),
                    authoritative_member.did.as_str(),
                )?;
                let mut result = authoritative;
                result.resolved_member = Some(authoritative_member);
                self.refresh_group_state_for(&mut result, &group, true);
                return Ok(result);
            }
            Err(error) => return Err(error),
        };
        #[cfg(not(feature = "group-e2ee"))]
        let mut result = p4_result?;
        result.resolved_member = Some(resolved_member.clone());
        crate::internal::group_runtime::projection::project_group_snapshot(self.client, &result);
        project_group_system_event_best_effort(self.client, &group, &mut result);
        self.refresh_group_state_for(&mut result, &group, true);
        #[cfg(feature = "group-e2ee")]
        if use_v2_p6 {
            let transition = crate::internal::group_e2ee::v2_lifecycle::required_member_transition(
                &group,
                expected_member_did.as_deref(),
                "active",
                &result,
            )?;
            let authoritative_member = super::GroupMemberResolution {
                did: crate::ids::Did::parse(&transition.member_did)?,
                handle: resolved_member.handle.clone(),
            };
            result.resolved_member = Some(authoritative_member);
            crate::internal::group_e2ee::v2_lifecycle::add_active_member_devices(
                self.client,
                transition.group_state_ref,
                &transition.member_did,
            )?;
        } else if let Some(secure_provider) = secure_provider {
            let group_state_ref = group_state_ref_from_result(&group, &result);
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
                    group_state_ref,
                    operation_id: None,
                },
            )?;
            result = super::GroupReadResult::from_raw_response(secure.delivery, secure.warnings);
            result.resolved_member = Some(resolved_member.clone());
            project_group_system_event_best_effort(self.client, &group, &mut result);
            self.refresh_group_state_for(&mut result, &group, true);
        }
        Ok(result)
    }

    pub async fn add_member_async(
        &self,
        request: super::GroupMemberMutationRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        #[cfg(feature = "group-e2ee")]
        if use_group_e2ee_v2_lifecycle(self.client) {
            let client = self.client.clone();
            return crate::internal::runtime::worker::run_blocking(move || {
                GroupService::new(&client).add_member(request)
            })
            .await
            .map_err(|err| crate::ImError::Internal {
                message: format!("P6 v2 group add worker failed: {err}"),
            })?;
        }
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
        .add_member_async(request, None)
        .await?;
        result.resolved_member = Some(resolved_member.clone());
        let _ = crate::internal::group_runtime::projection::project_group_snapshot_async(
            self.client,
            &result,
        )
        .await;
        project_group_system_event_best_effort_async(self.client, &group, &mut result).await;
        self.refresh_group_state_for_async(&mut result, &group, true)
            .await;
        #[cfg(feature = "group-e2ee")]
        if secure_required {
            let group_state_ref = group_state_ref_from_result(&group, &result);
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
                    group_state_ref,
                    operation_id: None,
                },
            )
            .await?;
            result = super::GroupReadResult::from_raw_response(secure.delivery, secure.warnings);
            result.resolved_member = Some(resolved_member.clone());
            project_group_system_event_best_effort_async(self.client, &group, &mut result).await;
            self.refresh_group_state_for_async(&mut result, &group, true)
                .await;
        }
        Ok(result)
    }

    pub fn rebind_member(
        &self,
        request: super::GroupRebindMemberRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let group = request.group.as_str().to_owned();
        let mut result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .rebind_member(request, None)?;
        crate::internal::group_runtime::projection::project_group_snapshot(self.client, &result);
        project_group_system_event_best_effort(self.client, &group, &mut result);
        self.refresh_group_state_for(&mut result, &group, true);
        Ok(result)
    }

    pub async fn rebind_member_async(
        &self,
        request: super::GroupRebindMemberRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let group = request.group.as_str().to_owned();
        let mut result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .rebind_member_async(request, None)
        .await?;
        let _ = crate::internal::group_runtime::projection::project_group_snapshot_async(
            self.client,
            &result,
        )
        .await;
        project_group_system_event_best_effort_async(self.client, &group, &mut result).await;
        self.refresh_group_state_for_async(&mut result, &group, true)
            .await;
        Ok(result)
    }

    pub async fn resume_rebind_recovery_async(
        &self,
        limit: u32,
    ) -> crate::ImResult<super::GroupRebindRecoverySummary> {
        let limit = limit.clamp(1, 100);
        let sqlite_path = &self.client.core_inner().sdk_paths().local_state.sqlite_path;
        let owner_identity_id = self.client.current_identity().id.as_str();
        let mut summary = super::GroupRebindRecoverySummary::default();

        if let Some(handle) = self.client.handle().cloned() {
            let lookup =
                crate::internal::handle_discovery::resolve_authoritative_handle_binding_async(
                    self.client,
                    handle.as_str(),
                )
                .await;
            match lookup {
                Ok(lookup) => {
                    match crate::internal::group_rebind_recovery::reconcile_missing_recovery_jobs(
                        sqlite_path,
                        owner_identity_id,
                        handle.as_str(),
                        self.client.did().as_str(),
                        &lookup,
                    ) {
                        Ok(reconciled) if reconciled > 0 => summary.warnings.push(format!(
                            "reconciled {reconciled} missing group rebind job(s) from authoritative Handle binding"
                        )),
                        Ok(_) => {}
                        Err(_) => summary.warnings.push(
                            "group rebind reconcile skipped because authoritative Handle binding validation failed"
                                .to_owned(),
                        ),
                    }
                }
                Err(_) => summary.warnings.push(
                    "group rebind reconcile skipped because authoritative Handle lookup failed"
                        .to_owned(),
                ),
            }
        } else {
            summary.warnings.push(
                "group rebind reconcile skipped because the current identity has no full Handle"
                    .to_owned(),
            );
        }

        let mut transport_refresh_failures = 0_u32;
        for group_did in crate::internal::group_rebind_recovery::awaiting_p6_groups(
            sqlite_path,
            owner_identity_id,
        )?
        .into_iter()
        .take(limit as usize)
        {
            if summary.processed >= limit {
                break;
            }
            if crate::internal::group_rebind_recovery::group_security_classification(
                sqlite_path,
                owner_identity_id,
                &group_did,
            )?
            .is_none()
            {
                let Ok(group) = crate::ids::GroupRef::parse(&group_did) else {
                    transport_refresh_failures += 1;
                    continue;
                };
                match self.get_async(group).await {
                    Ok(snapshot) => {
                        let _ = crate::internal::group_runtime::projection::project_group_snapshot_async(
                            self.client,
                            &snapshot,
                        )
                        .await;
                    }
                    Err(_) => transport_refresh_failures += 1,
                }
            }
            let completed = crate::internal::group_rebind_recovery::complete_transport_p4_jobs(
                sqlite_path,
                owner_identity_id,
                &group_did,
                limit.saturating_sub(summary.processed),
            )?;
            summary.processed += completed;
            summary.completed += completed;
        }
        if transport_refresh_failures > 0 {
            summary.warnings.push(format!(
                "{transport_refresh_failures} group rebind security profile refresh(es) failed; recovery remains paused"
            ));
        }

        for _ in 0..limit.saturating_sub(summary.processed) {
            let Some(job) = crate::internal::group_rebind_recovery::next_p4_job(
                sqlite_path,
                owner_identity_id,
            )?
            else {
                break;
            };
            summary.processed += 1;
            if job.new_member_did != self.client.did().as_str() {
                crate::internal::group_rebind_recovery::update_p4_job(
                    sqlite_path,
                    &job.job_id,
                    "blocked",
                    None,
                    Some("current identity is not the recovered new DID"),
                )?;
                summary.blocked += 1;
                continue;
            }
            match self.authoritative_recovery_rebind_state(&job).await {
                Ok(RecoveryRebindAuthority::Eligible) => {}
                Ok(RecoveryRebindAuthority::Converged) => {
                    crate::internal::group_rebind_recovery::project_applied_p4_rebind(
                        sqlite_path,
                        &job,
                    )?;
                    crate::internal::group_rebind_recovery::update_p4_job(
                        sqlite_path,
                        &job.job_id,
                        "complete",
                        None,
                        None,
                    )?;
                    summary.completed += 1;
                    continue;
                }
                Ok(RecoveryRebindAuthority::Ineligible) => {
                    crate::internal::group_rebind_recovery::update_p4_job(
                        sqlite_path,
                        &job.job_id,
                        "blocked",
                        None,
                        Some("authoritative group policy or roster is not an exact transport-protected Handle rebind"),
                    )?;
                    summary.blocked += 1;
                    continue;
                }
                Err(error) => {
                    crate::internal::group_rebind_recovery::update_p4_job(
                        sqlite_path,
                        &job.job_id,
                        "pending",
                        None,
                        Some(&error.to_string()),
                    )?;
                    summary.pending += 1;
                    continue;
                }
            }
            let request = super::GroupRebindMemberRequest {
                group: crate::ids::GroupRef::parse(&job.group_did)?,
                member_handle: crate::ids::Handle::parse(&job.member_handle, "")?,
                previous_member_did: crate::ids::Did::parse(&job.previous_member_did)?,
                new_member_did: crate::ids::Did::parse(&job.new_member_did)?,
                handle_binding_generation: job.binding_generation.clone(),
            };
            let rebind = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
                self.client,
                crate::internal::auth::session::FileSessionProvider::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
            )
            .rebind_member_with_operation_id_async(request, &job.job_id, None)
            .await;
            match rebind {
                Ok(result) => {
                    let mut result = result;
                    let _ =
                        crate::internal::group_runtime::projection::project_group_snapshot_async(
                            self.client,
                            &result,
                        )
                        .await;
                    if let Err(error) =
                        crate::internal::group_rebind_recovery::project_applied_p4_rebind(
                            sqlite_path,
                            &job,
                        )
                    {
                        let detail = error.to_string();
                        crate::internal::group_rebind_recovery::update_p4_job(
                            sqlite_path,
                            &job.job_id,
                            "pending",
                            None,
                            Some(&detail),
                        )?;
                        summary.pending += 1;
                        summary.warnings.push(format!(
                            "group {} accepted P4 rebind but local member projection will retry: {}",
                            job.group_did, detail
                        ));
                        continue;
                    }
                    if crate::internal::group_rebind_recovery::group_security_classification(
                        sqlite_path,
                        owner_identity_id,
                        &job.group_did,
                    )?
                    .is_none()
                    {
                        self.refresh_group_state_for_async(&mut result, &job.group_did, false)
                            .await;
                    }
                    let state_ref = p4_rebind_state_ref(&job.group_did, &result)?;
                    // Handle Recovery v1 deliberately migrates only groups
                    // whose exact required profile is transport-protected.
                    // Its contract-stable operation id is therefore also a
                    // closed execution boundary: these jobs complete at P4
                    // and must never be promoted into the legacy P6 rebind
                    // pipeline, even if cached group metadata changes later.
                    let e2ee = p4_rebind_requires_p6(
                        &job.job_id,
                        crate::internal::group_rebind_recovery::group_uses_e2ee(
                            sqlite_path,
                            owner_identity_id,
                            &job.group_did,
                        )?,
                    );
                    crate::internal::group_rebind_recovery::update_p4_job(
                        sqlite_path,
                        &job.job_id,
                        if e2ee { "awaiting_p6" } else { "complete" },
                        Some(&state_ref),
                        None,
                    )?;
                    if e2ee {
                        summary.pending += 1;
                    } else {
                        summary.completed += 1;
                    }
                }
                Err(error) => {
                    let detail = error.to_string();
                    let convergence = if rebind_error_requires_authoritative_reread(&error) {
                        self.authoritative_recovery_rebind_state(&job).await.ok()
                    } else {
                        None
                    };
                    if convergence == Some(RecoveryRebindAuthority::Converged) {
                        crate::internal::group_rebind_recovery::project_applied_p4_rebind(
                            sqlite_path,
                            &job,
                        )?;
                        crate::internal::group_rebind_recovery::update_p4_job(
                            sqlite_path,
                            &job.job_id,
                            "complete",
                            None,
                            None,
                        )?;
                        summary.completed += 1;
                        continue;
                    }
                    let blocked = if rebind_error_requires_authoritative_reread(&error) {
                        convergence.is_some()
                    } else {
                        rebind_error_is_terminal(&error)
                    };
                    crate::internal::group_rebind_recovery::update_p4_job(
                        sqlite_path,
                        &job.job_id,
                        if blocked { "blocked" } else { "pending" },
                        None,
                        Some(&detail),
                    )?;
                    if blocked {
                        summary.blocked += 1;
                    } else {
                        summary.pending += 1;
                    }
                    summary.warnings.push(format!(
                        "group {} P4 rebind {}: {}",
                        job.group_did,
                        if blocked { "blocked" } else { "will retry" },
                        detail
                    ));
                }
            }
        }

        #[cfg(feature = "group-e2ee")]
        for _ in 0..limit.saturating_sub(summary.processed) {
            let Some(job) = crate::internal::group_rebind_recovery::next_p6_job(
                sqlite_path,
                owner_identity_id,
            )?
            else {
                break;
            };
            summary.processed += 1;
            let group = crate::ids::GroupRef::parse(&job.group_did)?;
            let state_ref: anp::group_e2ee::GroupStateRef =
                serde_json::from_str(&job.group_state_ref_json).map_err(|error| {
                    crate::ImError::LocalStateUnavailable {
                        detail: format!("invalid persisted P4 group_state_ref: {error}"),
                    }
                })?;
            let outcome = match job.phase.as_str() {
                "awaiting_add" => {
                    let result =
                        crate::internal::group_e2ee::lifecycle::GroupE2eeLifecycleRuntime::new(
                            self.client,
                            crate::internal::auth::session::FileSessionProvider::new(self.client),
                            crate::internal::transport::CoreHttpTransport::new(self.client),
                            crate::internal::group_e2ee::storage::native_provider_for_client(
                                self.client,
                            )?,
                        )
                        .add_secure_member_async(
                            crate::internal::group_e2ee::lifecycle::GroupE2eeMemberMutationInput {
                                group: group.clone(),
                                member: crate::ids::Did::parse(&job.new_member_did)?,
                                reason_text: Some("handle credential rebind".to_owned()),
                                leave_request_id: None,
                                credentials: None,
                                service_did: None,
                                group_state_ref: Some(state_ref.clone()),
                                operation_id: Some(format!("{}-add", job.job_id)),
                            },
                        )
                        .await;
                    result.map(|result| p6_phase_after_add(result.finalize_outcome))
                }
                "add_repair" => match self
                    .client
                    .secure()
                    .group(group.clone())
                    .repair_async()
                    .await
                {
                    Ok(_) => verify_rebind_repair_async(self.client, &job, true)
                        .await
                        .map(|_| "awaiting_remove"),
                    Err(error) => Err(error),
                },
                "awaiting_remove" => {
                    let result =
                        crate::internal::group_e2ee::lifecycle::GroupE2eeLifecycleRuntime::new(
                            self.client,
                            crate::internal::auth::session::FileSessionProvider::new(self.client),
                            crate::internal::transport::CoreHttpTransport::new(self.client),
                            crate::internal::group_e2ee::storage::native_provider_for_client(
                                self.client,
                            )?,
                        )
                        .remove_secure_member_async(
                            crate::internal::group_e2ee::lifecycle::GroupE2eeMemberMutationInput {
                                group: group.clone(),
                                member: crate::ids::Did::parse(&job.previous_member_did)?,
                                reason_text: Some("handle credential rebind".to_owned()),
                                leave_request_id: None,
                                credentials: None,
                                service_did: None,
                                group_state_ref: Some(state_ref),
                                operation_id: Some(format!("{}-remove", job.job_id)),
                            },
                        )
                        .await;
                    result.map(|result| p6_phase_after_remove(result.finalize_outcome))
                }
                "remove_repair" => match self
                    .client
                    .secure()
                    .group(group.clone())
                    .repair_async()
                    .await
                {
                    Ok(_) => verify_rebind_repair_async(self.client, &job, false)
                        .await
                        .map(|_| "complete"),
                    Err(error) => Err(error),
                },
                _ => continue,
            };
            match outcome {
                Ok(phase) => {
                    crate::internal::group_rebind_recovery::update_p6_job(
                        sqlite_path,
                        &job.job_id,
                        phase,
                        None,
                    )?;
                    if phase == "complete" {
                        summary.completed += 1;
                    } else {
                        summary.pending += 1;
                    }
                }
                Err(error) => {
                    let detail = error.to_string();
                    let blocked = rebind_error_is_terminal(&error);
                    crate::internal::group_rebind_recovery::update_p6_job(
                        sqlite_path,
                        &job.job_id,
                        if blocked {
                            "blocked"
                        } else {
                            job.phase.as_str()
                        },
                        Some(&detail),
                    )?;
                    if blocked {
                        summary.blocked += 1;
                    } else {
                        summary.pending += 1;
                    }
                    summary.warnings.push(format!(
                        "group {} P6 phase {} {}: {}",
                        job.group_did,
                        job.phase,
                        if blocked { "blocked" } else { "will retry" },
                        detail
                    ));
                }
            }
        }

        summary.send_paused_groups = paused_rebind_groups(sqlite_path, owner_identity_id)?;
        summary.items =
            crate::internal::group_rebind_recovery::recovery_items(sqlite_path, owner_identity_id)?;
        Ok(summary)
    }

    async fn authoritative_recovery_rebind_state(
        &self,
        job: &crate::internal::group_rebind_recovery::P4RebindJob,
    ) -> crate::ImResult<RecoveryRebindAuthority> {
        let group = crate::ids::GroupRef::parse(&job.group_did)?;
        let authoritative = self.get_authoritative_group_async(group.clone()).await?;
        if authoritative_group_e2ee_classification(&job.group_did, &authoritative)? {
            return Ok(RecoveryRebindAuthority::Ineligible);
        }
        let expected_version = authoritative
            .raw_response()
            .and_then(|raw| raw.get("group_state_version"))
            .and_then(|value| match value {
                serde_json::Value::String(value) => Some(value.clone()),
                serde_json::Value::Number(value) => Some(value.to_string()),
                _ => None,
            })
            .filter(|value| !value.trim().is_empty())
            .ok_or(crate::ImError::InventoryIncomplete)?;
        let mut cursor = None;
        let mut members = Vec::new();
        for _ in 0..10 {
            let page = self
                .members_async(super::GroupMembersRequest {
                    group: group.clone(),
                    limit: crate::ids::PageLimit::new(100)?,
                    cursor,
                })
                .await?;
            if page.page_group.as_ref() != Some(&group)
                || page.group_state_version.as_deref() != Some(expected_version.as_str())
            {
                return Err(crate::ImError::CursorStale);
            }
            members.extend(page.members);
            if !page.has_more {
                return classify_recovery_rebind_authority(job, &members);
            }
            cursor = Some(
                page.next_cursor
                    .ok_or(crate::ImError::InventoryIncomplete)?,
            );
        }
        Err(crate::ImError::InventoryIncomplete)
    }

    pub fn remove_member(
        &self,
        request: super::GroupMemberMutationRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        #[cfg(feature = "group-e2ee")]
        let secure_required = request.security.required();
        let group = request.group.as_str().to_string();
        #[cfg(feature = "group-e2ee")]
        let requested_member_is_did = request.member.is_did();
        let resolved_member = resolve_group_member(self.client, &request.member)?;
        #[cfg(not(feature = "group-e2ee"))]
        if request.security.required() {
            return Err(crate::ImError::unsupported("group-e2ee"));
        }
        #[cfg(feature = "group-e2ee")]
        let v2_enabled = use_group_e2ee_v2_lifecycle(self.client);
        #[cfg(feature = "group-e2ee")]
        let v2_route = if v2_enabled {
            let authoritative = self.get_authoritative_group(request.group.clone())?;
            Some(v2_member_mutation_route(
                &group,
                &authoritative,
                secure_required,
            )?)
        } else {
            None
        };
        #[cfg(feature = "group-e2ee")]
        let use_v2_p6 = v2_route == Some(V2MemberMutationRoute::OwnerP6);
        #[cfg(feature = "group-e2ee")]
        let secure_provider = if secure_required && !v2_enabled {
            ensure_group_e2ee_service_available(self.client, false)?;
            Some(crate::internal::group_e2ee::storage::native_provider_for_client(self.client)?)
        } else {
            None
        };
        #[cfg(feature = "group-e2ee")]
        let use_legacy_cache_guard = !v2_enabled;
        #[cfg(not(feature = "group-e2ee"))]
        let use_legacy_cache_guard = true;
        if use_legacy_cache_guard {
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
        }
        #[cfg(feature = "group-e2ee")]
        if secure_required || use_v2_p6 {
            #[cfg(feature = "group-e2ee")]
            {
                let resolved_member = if use_v2_p6 {
                    self.authoritative_active_member(
                        crate::ids::GroupRef::parse(&group)?,
                        &resolved_member,
                    )?
                    .unwrap_or_else(|| resolved_member.clone())
                } else {
                    resolved_member.clone()
                };
                if use_v2_p6 {
                    crate::internal::group_e2ee::v2_lifecycle::preflight_current_controller(
                        self.client,
                        &group,
                    )?;
                }
                let mut p4_request =
                    resolved_group_member_request(request.clone(), &resolved_member);
                p4_request.security = super::GroupSecurityRequirement::Default;
                let p4_result =
                    crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
                        self.client,
                        crate::internal::auth::session::FileSessionProvider::new(self.client),
                        crate::internal::transport::CoreHttpTransport::new(self.client),
                    )
                    .remove_member(p4_request, None);
                let p4_result = match p4_result {
                    Ok(result) => result,
                    Err(error) if use_v2_p6 && group_error_is_not_member(&error) => {
                        let authoritative =
                            self.get_authoritative_group(crate::ids::GroupRef::parse(&group)?)?;
                        // The original Handle may now resolve to a different
                        // DID, so a retry cannot safely infer the removed P4
                        // subject from today's binding. Reconcile the whole
                        // accepted tree against the authoritative P4 roster;
                        // that removes the old DID without persisting another
                        // membership state chain in Core.
                        crate::internal::group_e2ee::v2_lifecycle::reconcile_group_device_roster(
                            self.client,
                            crate::ids::GroupRef::parse(&group)?,
                        )?;
                        let mut result = authoritative;
                        // A Handle may have rebound since the original P4
                        // removal. Do not report today's DID as the removed
                        // subject when this retry only proves whole-roster
                        // convergence.
                        result.resolved_member = requested_member_is_did.then_some(resolved_member);
                        self.refresh_group_state_for(&mut result, &group, true);
                        return Ok(result);
                    }
                    Err(error) => return Err(error),
                };
                if use_v2_p6 {
                    let transition =
                        crate::internal::group_e2ee::v2_lifecycle::required_member_transition(
                            &group,
                            Some(resolved_member.did.as_str()),
                            "removed",
                            &p4_result,
                        )?;
                    crate::internal::group_e2ee::v2_lifecycle::remove_inactive_member_devices(
                        self.client,
                        transition.group_state_ref,
                        &transition.member_did,
                    )?;
                    let mut result = p4_result;
                    result.resolved_member = Some(super::GroupMemberResolution {
                        did: crate::ids::Did::parse(&transition.member_did)?,
                        handle: resolved_member.handle,
                    });
                    self.refresh_group_state_for(&mut result, &group, true);
                    return Ok(result);
                }
                let group_state_ref =
                    group_state_ref_from_result(&group, &p4_result).ok_or_else(|| {
                        crate::ImError::LocalStateUnavailable {
                            detail:
                                "group.remove response omitted the pending-removal group_state_ref"
                                    .to_owned(),
                        }
                    })?;
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
                            group_state_ref: Some(group_state_ref),
                            operation_id: None,
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
        #[cfg(feature = "group-e2ee")]
        if use_group_e2ee_v2_lifecycle(self.client) {
            let client = self.client.clone();
            return crate::internal::runtime::worker::run_blocking(move || {
                GroupService::new(&client).remove_member(request)
            })
            .await
            .map_err(|err| crate::ImError::Internal {
                message: format!("P6 v2 group remove worker failed: {err}"),
            })?;
        }
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
            let mut p4_request = resolved_group_member_request(request.clone(), &resolved_member);
            p4_request.security = super::GroupSecurityRequirement::Default;
            let p4_result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
                self.client,
                crate::internal::auth::session::FileSessionProvider::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
            )
            .remove_member_async(p4_request, None)
            .await?;
            let group_state_ref =
                group_state_ref_from_result(&group, &p4_result).ok_or_else(|| {
                    crate::ImError::LocalStateUnavailable {
                        detail: "group.remove response omitted the pending-removal group_state_ref"
                            .to_owned(),
                    }
                })?;
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
                    group_state_ref: Some(group_state_ref),
                    operation_id: None,
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
            Err(crate::ImError::unsupported("group-e2ee"))
        }
        #[cfg(feature = "group-e2ee")]
        {
            ensure_group_e2ee_service_available(self.client, false)?;
            let resolved_member = resolve_group_member(self.client, &request.member)?;
            crate::internal::group_e2ee::lifecycle::GroupE2eeLifecycleRuntime::new(
                self.client,
                crate::internal::auth::session::FileSessionProvider::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
                crate::internal::group_e2ee::storage::native_provider_for_client(self.client)?,
            )
            .acknowledge_leave_request(
                crate::internal::group_e2ee::lifecycle::GroupE2eeMemberMutationInput {
                    group: request.group.clone(),
                    member: resolved_member.did.clone(),
                    reason_text: request.reason_text.clone(),
                    leave_request_id: Some(request.leave_request_id.clone()),
                    credentials: None,
                    service_did: None,
                    group_state_ref: None,
                    operation_id: None,
                },
            )?;
            self.remove_member(super::GroupMemberMutationRequest {
                group: request.group,
                member: super::GroupMemberRef::from(resolved_member.did),
                role: None,
                reason_text: request.reason_text,
                leave_request_id: Some(request.leave_request_id),
                security: super::GroupSecurityRequirement::Required,
            })
        }
    }

    pub async fn process_e2ee_leave_request_async(
        &self,
        request: super::GroupE2eeProcessLeaveRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        #[cfg(not(feature = "group-e2ee"))]
        {
            let _ = request;
            Err(crate::ImError::unsupported("group-e2ee"))
        }
        #[cfg(feature = "group-e2ee")]
        {
            ensure_group_e2ee_service_available_async(self.client, false).await?;
            let resolved_member = resolve_group_member_async(self.client, &request.member).await?;
            crate::internal::group_e2ee::lifecycle::GroupE2eeLifecycleRuntime::new(
                self.client,
                crate::internal::auth::session::FileSessionProvider::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
                crate::internal::group_e2ee::storage::native_provider_for_client(self.client)?,
            )
            .acknowledge_leave_request_async(
                crate::internal::group_e2ee::lifecycle::GroupE2eeMemberMutationInput {
                    group: request.group.clone(),
                    member: resolved_member.did.clone(),
                    reason_text: request.reason_text.clone(),
                    leave_request_id: Some(request.leave_request_id.clone()),
                    credentials: None,
                    service_did: None,
                    group_state_ref: None,
                    operation_id: None,
                },
            )
            .await?;
            self.remove_member_async(super::GroupMemberMutationRequest {
                group: request.group,
                member: super::GroupMemberRef::from(resolved_member.did),
                role: None,
                reason_text: request.reason_text,
                leave_request_id: Some(request.leave_request_id),
                security: super::GroupSecurityRequirement::Required,
            })
            .await
        }
    }

    pub fn update_member_key(
        &self,
        request: super::GroupE2eeUpdateKeyRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        #[cfg(not(feature = "group-e2ee"))]
        {
            let _ = request;
            Err(crate::ImError::unsupported("group-e2ee"))
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
            Err(crate::ImError::unsupported("group-e2ee"))
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
            Err(crate::ImError::unsupported("group-e2ee"))
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
            Err(crate::ImError::unsupported("group-e2ee"))
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

    #[cfg(feature = "group-e2ee")]
    fn get_authoritative_group(
        &self,
        group: crate::ids::GroupRef,
    ) -> crate::ImResult<super::GroupReadResult> {
        let result = crate::internal::group_runtime::read::GroupReadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .get_with_policy(group)?;
        crate::internal::group_runtime::projection::project_group_snapshot(self.client, &result);
        Ok(result)
    }

    async fn get_authoritative_group_async(
        &self,
        group: crate::ids::GroupRef,
    ) -> crate::ImResult<super::GroupReadResult> {
        let result = crate::internal::group_runtime::read::GroupReadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .get_with_policy_async(group)
        .await?;
        let _ = crate::internal::group_runtime::projection::project_group_snapshot_async(
            self.client,
            &result,
        )
        .await;
        Ok(result)
    }

    #[cfg(feature = "group-e2ee")]
    fn authoritative_active_member(
        &self,
        group: crate::ids::GroupRef,
        requested: &super::GroupMemberResolution,
    ) -> crate::ImResult<Option<super::GroupMemberResolution>> {
        const MAX_ATTEMPTS: usize = 4;

        for attempt in 0..MAX_ATTEMPTS {
            let authoritative = self.get_authoritative_group(group.clone())?;
            let raw = authoritative
                .raw_response()
                .ok_or(crate::ImError::InventoryIncomplete)?;
            let max_members =
                crate::internal::group_e2ee::member_collector::product_max_members(raw)?;
            let expected_version = raw
                .get("group_state_version")
                .and_then(serde_json::Value::as_str)
                .ok_or(crate::ImError::InventoryIncomplete)?;
            let roster =
                match crate::internal::group_e2ee::member_collector::collect_complete_group_members(
                    self.client,
                    group.clone(),
                    Some(expected_version),
                    max_members,
                ) {
                    Err(crate::ImError::CursorStale) if attempt + 1 < MAX_ATTEMPTS => continue,
                    result => result?,
                };
            return Ok(roster
                .members
                .into_iter()
                .find(|member| active_member_matches_resolution(member, requested))
                .and_then(|member| {
                    member.did.map(|did| super::GroupMemberResolution {
                        did,
                        handle: member.handle.or_else(|| requested.handle.clone()),
                    })
                }));
        }
        Err(crate::ImError::CursorStale)
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
        let limit = request.limit;
        let group = request.group.as_str().to_string();
        let mut result = crate::internal::group_runtime::read::GroupReadRuntime::new(
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
        result.replace_messages(
            crate::internal::message_runtime::read::merge_group_local_projection_best_effort(
                self.client,
                result.messages.clone(),
                &crate::ids::GroupRef::parse(&group)?,
                limit,
            ),
        );
        Ok(result)
    }

    pub async fn messages_async(
        &self,
        request: super::GroupMessagesRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let group = request.group.as_str().to_string();
        let limit = request.limit;
        let mut result = crate::internal::group_runtime::read::GroupReadRuntime::new(
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
        let merged =
            crate::internal::message_runtime::read::merge_group_local_projection_best_effort_async(
                self.client,
                result.messages.clone(),
                &crate::ids::GroupRef::parse(&group)?,
                limit,
            )
            .await;
        result.replace_messages(merged);
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
            Ok(snapshot) => {
                crate::internal::group_runtime::projection::project_group_snapshot(
                    self.client,
                    &snapshot,
                );
                result.merge_group_snapshot_from(&snapshot);
            }
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
            cursor: None,
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
            Ok(snapshot) => {
                let _ = crate::internal::group_runtime::projection::project_group_snapshot_async(
                    self.client,
                    &snapshot,
                )
                .await;
                result.merge_group_snapshot_from(&snapshot);
            }
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
                cursor: None,
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

#[cfg(feature = "group-e2ee")]
fn group_state_ref_from_result(
    group_did: &str,
    result: &super::GroupReadResult,
) -> Option<anp::group_e2ee::GroupStateRef> {
    result.raw_response().and_then(|raw| {
        crate::internal::group_e2ee::state_ref::group_state_ref_from_group_response(group_did, raw)
    })
}

#[cfg(feature = "group-e2ee")]
fn group_state_ref_from_current_group(
    service: &GroupService<'_>,
    group_did: &str,
) -> Option<anp::group_e2ee::GroupStateRef> {
    let group = crate::ids::GroupRef::parse(group_did).ok()?;
    service
        .get(group)
        .ok()
        .and_then(|result| group_state_ref_from_result(group_did, &result))
}

#[cfg(feature = "group-e2ee")]
async fn group_state_ref_from_current_group_async(
    service: &GroupService<'_>,
    group_did: &str,
) -> Option<anp::group_e2ee::GroupStateRef> {
    let group = crate::ids::GroupRef::parse(group_did).ok()?;
    service
        .get_async(group)
        .await
        .ok()
        .and_then(|result| group_state_ref_from_result(group_did, &result))
}

fn p4_rebind_state_ref(
    group_did: &str,
    result: &super::GroupReadResult,
) -> crate::ImResult<anp::group_e2ee::GroupStateRef> {
    let version = result
        .raw_response()
        .and_then(|raw| raw.get("group_state_version"))
        .and_then(|value| match value {
            serde_json::Value::String(value) => Some(value.clone()),
            serde_json::Value::Number(value) => Some(value.to_string()),
            _ => None,
        })
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| crate::ImError::LocalStateUnavailable {
            detail: "group.rebind_member response omitted group_state_version".to_owned(),
        })?;
    serde_json::from_value(serde_json::json!({
        "group_did": group_did,
        "group_state_version": version,
    }))
    .map_err(|error| crate::ImError::LocalStateUnavailable {
        detail: format!("failed to build P4 rebind group_state_ref: {error}"),
    })
}

fn rebind_error_is_terminal(error: &crate::ImError) -> bool {
    match error {
        crate::ImError::Service {
            code: Some(code), ..
        } => matches!(
            code.trim().to_ascii_lowercase().as_str(),
            "3011"
                | "3012"
                | "3013"
                | "3014"
                | "handle_binding_stale"
                | "rebind_not_allowed"
                | "rebind_conflict"
                | "invalid_handle_binding"
                | "owner_required"
                | "not_member"
                | "not-member"
        ),
        crate::ImError::InvalidInput { .. }
        | crate::ImError::IdentityNotFound { .. }
        | crate::ImError::PermissionDenied
        | crate::ImError::GroupNotFound { .. }
        | crate::ImError::UnsupportedCapability { .. } => true,
        _ => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecoveryRebindAuthority {
    Eligible,
    Converged,
    Ineligible,
}

fn classify_recovery_rebind_authority(
    job: &crate::internal::group_rebind_recovery::P4RebindJob,
    members: &[super::GroupMember],
) -> crate::ImResult<RecoveryRebindAuthority> {
    let previous_generation = job
        .binding_generation
        .parse::<u128>()
        .ok()
        .and_then(|value| value.checked_sub(1))
        .filter(|value| *value > 0)
        .map(|value| value.to_string())
        .ok_or(crate::ImError::PermissionDenied)?;
    let active = members
        .iter()
        .filter(|member| member.status.as_deref() == Some("active"))
        .filter(|member| {
            member.handle.as_ref().map(crate::ids::Handle::as_str)
                == Some(job.member_handle.as_str())
        })
        .collect::<Vec<_>>();
    if active.len() != 1 || active[0].subject_type.as_deref() != Some("user") {
        return Ok(RecoveryRebindAuthority::Ineligible);
    }
    let member = active[0];
    let did = member.did.as_ref().map(crate::ids::Did::as_str);
    let generation = member.handle_binding_generation.as_deref();
    if did == Some(job.new_member_did.as_str())
        && generation == Some(job.binding_generation.as_str())
    {
        return Ok(RecoveryRebindAuthority::Converged);
    }
    if did == Some(job.previous_member_did.as_str())
        && generation == Some(previous_generation.as_str())
    {
        return Ok(RecoveryRebindAuthority::Eligible);
    }
    Ok(RecoveryRebindAuthority::Ineligible)
}

fn rebind_error_requires_authoritative_reread(error: &crate::ImError) -> bool {
    matches!(
        error,
        crate::ImError::Service { code: Some(code), .. }
            if matches!(
                code.trim().to_ascii_lowercase().as_str(),
                "handle_binding_stale"
                    | "group_handle_binding_stale"
                    | "rebind_not_allowed"
                    | "group_rebind_not_allowed"
            )
    )
}

#[cfg(feature = "group-e2ee")]
fn p6_phase_after_add(
    outcome: crate::internal::group_e2ee::lifecycle::GroupE2eeFinalizeOutcome,
) -> &'static str {
    match outcome {
        crate::internal::group_e2ee::lifecycle::GroupE2eeFinalizeOutcome::AcceptedNeedsRepair => {
            "add_repair"
        }
        crate::internal::group_e2ee::lifecycle::GroupE2eeFinalizeOutcome::Finalized => {
            "awaiting_remove"
        }
    }
}

fn p4_rebind_requires_p6(job_id: &str, group_uses_e2ee: bool) -> bool {
    group_uses_e2ee
        && !crate::internal::group_rebind_recovery::is_handle_recovery_operation_id(job_id)
}

#[cfg(feature = "group-e2ee")]
fn p6_phase_after_remove(
    outcome: crate::internal::group_e2ee::lifecycle::GroupE2eeFinalizeOutcome,
) -> &'static str {
    match outcome {
        crate::internal::group_e2ee::lifecycle::GroupE2eeFinalizeOutcome::AcceptedNeedsRepair => {
            "remove_repair"
        }
        crate::internal::group_e2ee::lifecycle::GroupE2eeFinalizeOutcome::Finalized => "complete",
    }
}

#[cfg(feature = "group-e2ee")]
async fn verify_rebind_repair_async(
    client: &crate::core::ImClient,
    job: &crate::internal::group_rebind_recovery::P6RebindJob,
    add_phase: bool,
) -> crate::ImResult<()> {
    use crate::internal::group_e2ee::provider::GroupMlsProvider as _;
    let provider = crate::internal::group_e2ee::storage::native_provider_for_client(client)?;
    let operation_id = format!(
        "{}-{}",
        job.job_id,
        if add_phase { "add" } else { "remove" }
    );
    let input = anp::group_e2ee::operations::StatusInput {
        request_id: format!(
            "group-rebind-repair-status-{}",
            crate::internal::wire::common::generate_operation_id()
        ),
        device_id: client
            .current_identity()
            .device_id
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| crate::internal::group_e2ee::DEFAULT_GROUP_MLS_DEVICE_ID.to_owned()),
        agent_did: Some(client.did().as_str().to_owned()),
        group_did: Some(job.group_did.clone()),
    };
    let status = crate::internal::runtime::worker::run_blocking(move || provider.status(input))
        .await
        .map_err(|error| crate::ImError::Internal {
            message: error.to_string(),
        })??;
    rebind_repair_status_is_finalized(
        &status,
        &operation_id,
        &job.previous_member_did,
        &job.new_member_did,
        add_phase,
    )
}

#[cfg(feature = "group-e2ee")]
fn rebind_repair_status_is_finalized(
    status: &anp::group_e2ee::operations::StatusOutput,
    operation_id: &str,
    old_did: &str,
    new_did: &str,
    add_phase: bool,
) -> crate::ImResult<()> {
    if status
        .pending_commits
        .iter()
        .any(|pending| pending.operation_id == operation_id)
    {
        return Err(crate::ImError::LocalStateUnavailable {
            detail: format!("rebind repair still has pending operation {operation_id}"),
        });
    }
    let roster = status
        .member_dids
        .iter()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    let old_present = roster.contains(old_did);
    let applied = roster.contains(new_did) && if add_phase { old_present } else { !old_present };
    if !applied {
        return Err(crate::ImError::LocalStateUnavailable {
            detail: "rebind repair roster does not prove the expected commit finalized".to_owned(),
        });
    }
    Ok(())
}

fn paused_rebind_groups(
    sqlite_path: &std::path::Path,
    owner_identity_id: &str,
) -> crate::ImResult<Vec<crate::ids::GroupRef>> {
    crate::internal::group_rebind_recovery::paused_groups(sqlite_path, owner_identity_id)?
        .into_iter()
        .map(crate::ids::GroupRef::parse)
        .collect()
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

fn project_group_system_event_best_effort(
    client: &crate::core::ImClient,
    group: &str,
    result: &mut super::GroupReadResult,
) {
    if let Err(error) =
        crate::internal::group_system_events::persist_group_read_result(client, group, result)
    {
        result.push_warning(format!("group_system_event_projection_failed:{error}"));
    }
}

async fn project_group_system_event_best_effort_async(
    client: &crate::core::ImClient,
    group: &str,
    result: &mut super::GroupReadResult,
) {
    if let Err(error) =
        crate::internal::group_system_events::persist_group_read_result_async(client, group, result)
            .await
    {
        result.push_warning(format!("group_system_event_projection_failed:{error}"));
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
fn group_error_is_already_member(error: &crate::ImError) -> bool {
    matches!(
        error,
        crate::ImError::Service { code: Some(code), .. }
            if matches!(
                code.trim().to_ascii_lowercase().as_str(),
                "group.already_member"
                    | "group_already_member"
                    | "already_member"
                    | "already-member"
            )
    )
}

#[cfg(feature = "group-e2ee")]
fn active_member_matches_resolution(
    member: &super::GroupMember,
    requested: &super::GroupMemberResolution,
) -> bool {
    // P4's authoritative roster currently exposes the member DID but does not
    // have to repeat its Handle. The Handle was already freshly resolved by
    // Core, so use that DID as the stable comparison anchor.
    member.status.as_deref().unwrap_or("active") == "active"
        && member.did.as_ref() == Some(&requested.did)
}

#[cfg(feature = "group-e2ee")]
fn group_error_is_not_member(error: &crate::ImError) -> bool {
    matches!(
        error,
        crate::ImError::Service { code: Some(code), .. }
            if matches!(
                code.trim().to_ascii_lowercase().as_str(),
                "group.not_member" | "group_not_member" | "not_member" | "not-member"
            )
    )
}

#[cfg(feature = "group-e2ee")]
fn use_group_e2ee_v2_lifecycle(client: &crate::core::ImClient) -> bool {
    client.core_inner().group_e2ee_v2_enabled()
}

#[cfg(feature = "group-e2ee")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum V2MemberMutationRoute {
    BaseOnly,
    OwnerP6,
}

#[cfg(feature = "group-e2ee")]
fn v2_member_mutation_route(
    group_did: &str,
    authoritative: &super::GroupReadResult,
    caller_requested_e2ee: bool,
) -> crate::ImResult<V2MemberMutationRoute> {
    match authoritative_group_e2ee_classification(group_did, authoritative)? {
        false => {
            if caller_requested_e2ee {
                return Err(crate::ImError::invalid_input(
                    Some("security".to_owned()),
                    "the authoritative group policy is transport-protected",
                ));
            }
            Ok(V2MemberMutationRoute::BaseOnly)
        }
        true => {
            let group = authoritative
                .group
                .as_ref()
                .filter(|group| group.did.as_str() == group_did)
                .ok_or_else(|| crate::ImError::LocalStateUnavailable {
                    detail: "authoritative group.get omitted the caller membership".to_owned(),
                })?;
            if group.membership_status.as_deref() != Some("active") {
                return Err(crate::ImError::PermissionDenied);
            }
            if group.my_role.as_deref() != Some("owner") {
                // This does not reinterpret the P4 role as unauthorized. The
                // combined API stops before P4 because ordinary membership
                // mutations do not yet have a durable owner-handoff job.
                return Err(crate::ImError::LocalStateUnavailable {
                    detail: "P4 may authorize this member mutation, but P6 v2 requires an active owner device; refusing to split P4 and P6 without durable owner orchestration"
                        .to_owned(),
                });
            }
            Ok(V2MemberMutationRoute::OwnerP6)
        }
    }
}

pub(crate) fn authoritative_group_e2ee_classification(
    group_did: &str,
    authoritative: &super::GroupReadResult,
) -> crate::ImResult<bool> {
    if authoritative
        .group
        .as_ref()
        .filter(|group| group.did.as_str() == group_did)
        .is_none()
    {
        return Err(crate::ImError::LocalStateUnavailable {
            detail: "authoritative group.get returned a different or missing group".to_owned(),
        });
    }
    let raw =
        authoritative
            .raw_response()
            .ok_or_else(|| crate::ImError::LocalStateUnavailable {
                detail: "authoritative group.get omitted its raw response".to_owned(),
            })?;
    let group_ids = [
        "/group_did",
        "/group/group_did",
        "/group_snapshot/group_did",
    ]
    .into_iter()
    .filter_map(|pointer| raw.pointer(pointer))
    .collect::<Vec<_>>();
    if group_ids.is_empty()
        || group_ids
            .iter()
            .any(|value| value.as_str() != Some(group_did))
    {
        return Err(crate::ImError::LocalStateUnavailable {
            detail: "authoritative group.get returned a conflicting or missing group_did"
                .to_owned(),
        });
    }
    if let Some(value) = raw.get("group_policy") {
        let Some(policy) = value.as_object() else {
            return Err(crate::ImError::LocalStateUnavailable {
                detail: "authoritative group.get returned a malformed group policy".to_owned(),
            });
        };
        let Some(profile) = policy
            .get("message_security_profile")
            .and_then(serde_json::Value::as_str)
        else {
            return Err(crate::ImError::LocalStateUnavailable {
                detail: "authoritative group.get returned a malformed group policy".to_owned(),
            });
        };
        return match profile {
            "group-e2ee" => Ok(true),
            "transport-protected" => Ok(false),
            _ => Err(crate::ImError::LocalStateUnavailable {
                detail: "authoritative group.get did not classify the group security profile"
                    .to_owned(),
            }),
        };
    }

    let mut policy_profiles = Vec::new();
    for pointer in ["/group/group_policy", "/group_snapshot/group_policy"] {
        let Some(value) = raw.pointer(pointer) else {
            continue;
        };
        let Some(policy) = value.as_object() else {
            return Err(crate::ImError::LocalStateUnavailable {
                detail: "authoritative group.get returned a malformed group policy".to_owned(),
            });
        };
        let Some(profile) = policy
            .get("message_security_profile")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|profile| !profile.is_empty())
        else {
            return Err(crate::ImError::LocalStateUnavailable {
                detail: "authoritative group.get returned a malformed group policy".to_owned(),
            });
        };
        policy_profiles.push(profile.to_ascii_lowercase());
    }
    if !policy_profiles.is_empty() {
        return classify_group_security_profiles(&policy_profiles);
    }

    let mut projected_profiles = Vec::new();
    for pointer in [
        "/message_security_profile",
        "/required_security_profile",
        "/group/message_security_profile",
        "/group/required_security_profile",
        "/group_snapshot/message_security_profile",
        "/group_snapshot/required_security_profile",
    ] {
        let Some(value) = raw.pointer(pointer) else {
            continue;
        };
        let Some(profile) = value
            .as_str()
            .map(str::trim)
            .filter(|profile| !profile.is_empty())
        else {
            return Err(crate::ImError::LocalStateUnavailable {
                detail: "authoritative group.get returned a malformed security profile".to_owned(),
            });
        };
        projected_profiles.push(profile.to_ascii_lowercase());
    }
    classify_group_security_profiles(&projected_profiles)
}

fn classify_group_security_profiles(profiles: &[String]) -> crate::ImResult<bool> {
    let mut classification = None;
    for profile in profiles {
        let current = match profile.as_str() {
            "group-e2ee" => true,
            "transport-protected" | "transport" => false,
            _ => {
                return Err(crate::ImError::LocalStateUnavailable {
                    detail: "authoritative group.get did not classify the group security profile"
                        .to_owned(),
                });
            }
        };
        if classification.is_some_and(|previous| previous != current) {
            return Err(crate::ImError::LocalStateUnavailable {
                detail: "authoritative group.get returned conflicting security profiles".to_owned(),
            });
        }
        classification = Some(current);
    }
    classification.ok_or_else(|| crate::ImError::LocalStateUnavailable {
        detail: "authoritative group.get did not classify the group security profile".to_owned(),
    })
}

#[cfg(feature = "group-e2ee")]
fn require_v2_leave_safe(
    group_did: &str,
    authoritative: &super::GroupReadResult,
    caller_requested_e2ee: bool,
) -> crate::ImResult<()> {
    if authoritative_group_e2ee_classification(group_did, authoritative)? {
        return Err(crate::ImError::LocalStateUnavailable {
            detail: "P6 v2 leave requires a subsequent owner-controlled device Remove; refusing P4 leave until durable owner orchestration is available"
                .to_owned(),
        });
    }
    if caller_requested_e2ee {
        return Err(crate::ImError::invalid_input(
            Some("security".to_owned()),
            "the authoritative group policy is transport-protected",
        ));
    }
    if authoritative.group.as_ref().is_some_and(|group| {
        group.did.as_str() == group_did
            && group.my_role.as_deref() == Some("owner")
            && group.membership_status.as_deref() == Some("active")
    }) {
        return Err(crate::ImError::invalid_input(
            Some("group".to_owned()),
            "group owner cannot leave the group",
        ));
    }
    Ok(())
}

#[cfg(feature = "group-e2ee")]
fn p4_member_mutation_request(
    mut request: super::GroupMemberMutationRequest,
    use_v2_p6: bool,
) -> super::GroupMemberMutationRequest {
    if use_v2_p6 {
        request.security = super::GroupSecurityRequirement::Default;
    }
    request
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

#[cfg(all(test, feature = "group-e2ee"))]
fn verify_group_key_package_binding_contract(
    binding: &serde_json::Value,
    did_document: &serde_json::Value,
) -> crate::ImResult<()> {
    anp::proof::verify_did_wba_binding(
        binding,
        did_document,
        anp::proof::DidWbaBindingVerificationOptions {
            expected_leaf_signature_key_b64u: binding
                .get("leaf_signature_key_b64u")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
            expected_credential_identity: binding
                .get("agent_did")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
            ..Default::default()
        },
    )
    .map_err(|err| crate::ImError::Serialization {
        detail: format!("verify group KeyPackage DID WBA binding proof: {err}"),
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
mod tests;
