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
            require_group_e2ee_v2(self.client)?;
            crate::internal::group_e2ee::v2_lifecycle::publish_current_key_package(
                self.client,
                request,
            )
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
            require_group_e2ee_v2(self.client)?;
            crate::internal::group_e2ee::v2_lifecycle::publish_current_key_package_async(
                self.client,
                request,
            )
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
        if secure_required {
            require_group_e2ee_v2(self.client)?;
        }
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
            crate::internal::group_e2ee::v2_lifecycle::initialize_created_group(
                self.client,
                crate::internal::group_e2ee::v2_lifecycle::required_created_group_state_ref(
                    &group,
                    group_state_ref,
                )?,
            )?;
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
            require_group_e2ee_v2(self.client)?;
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
            let group_state_ref =
                crate::internal::group_e2ee::v2_lifecycle::required_created_group_state_ref(
                    &group,
                    group_state_ref,
                )?;
            crate::internal::group_e2ee::v2_lifecycle::initialize_created_group_async(
                self.client,
                group_state_ref,
            )
            .await?;
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
        {
            let authoritative = self.get_authoritative_group(request.group.clone())?;
            let group_uses_e2ee = authoritative_group_e2ee_classification(&group, &authoritative)?;
            if group_uses_e2ee {
                require_group_e2ee_v2(self.client)?;
            }
            require_v2_leave_safe(&group, &authoritative, request.security.required())?;
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
        {
            let authoritative = self
                .get_authoritative_group_async(request.group.clone())
                .await?;
            let group_uses_e2ee = authoritative_group_e2ee_classification(&group, &authoritative)?;
            if group_uses_e2ee {
                require_group_e2ee_v2(self.client)?;
            }
            require_v2_leave_safe(&group, &authoritative, request.security.required())?;
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
        #[cfg(feature = "group-e2ee")]
        let requested_member_is_did = request.member.is_did();
        let resolved_member = resolve_group_member(self.client, &request.member)?;
        #[cfg(feature = "group-e2ee")]
        let expected_member_did =
            requested_member_is_did.then(|| resolved_member.did.as_str().to_owned());
        #[cfg(not(feature = "group-e2ee"))]
        if request.security.required() {
            return Err(crate::ImError::unsupported("group-e2ee"));
        }
        #[cfg(feature = "group-e2ee")]
        let v2_route = {
            let authoritative = self.get_authoritative_group(request.group.clone())?;
            v2_member_mutation_route(&group, &authoritative, request.security.required())?
        };
        #[cfg(feature = "group-e2ee")]
        let use_v2_p6 = v2_route == V2MemberMutationRoute::OwnerP6;
        #[cfg(feature = "group-e2ee")]
        if use_v2_p6 {
            require_group_e2ee_v2(self.client)?;
            crate::internal::group_e2ee::v2_lifecycle::preflight_current_controller(
                self.client,
                &group,
            )?;
        }
        #[allow(unused_mut)]
        let mut request = resolved_group_member_request(request, &resolved_member);
        #[cfg(feature = "group-e2ee")]
        if use_v2_p6 {
            request.security = super::GroupSecurityRequirement::Default;
        }
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
        }
        Ok(result)
    }

    pub async fn add_member_async(
        &self,
        request: super::GroupMemberMutationRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        #[cfg(feature = "group-e2ee")]
        let requested_member_is_did = request.member.is_did();
        let secure_required = request.security.required();
        let group = request.group.as_str().to_string();
        let resolved_member = resolve_group_member_async(self.client, &request.member).await?;
        #[cfg(feature = "group-e2ee")]
        let expected_member_did =
            requested_member_is_did.then(|| resolved_member.did.as_str().to_owned());
        #[cfg(not(feature = "group-e2ee"))]
        if secure_required {
            return Err(crate::ImError::unsupported("group-e2ee"));
        }
        #[cfg(feature = "group-e2ee")]
        let v2_route = {
            let authoritative = self
                .get_authoritative_group_async(request.group.clone())
                .await?;
            v2_member_mutation_route(&group, &authoritative, secure_required)?
        };
        #[cfg(feature = "group-e2ee")]
        let use_v2_p6 = v2_route == V2MemberMutationRoute::OwnerP6;
        #[cfg(feature = "group-e2ee")]
        if use_v2_p6 {
            require_group_e2ee_v2(self.client)?;
            crate::internal::group_e2ee::v2_lifecycle::preflight_current_controller_async(
                self.client,
                &group,
            )
            .await?;
        }
        #[allow(unused_mut)]
        let mut request = resolved_group_member_request(request, &resolved_member);
        #[cfg(feature = "group-e2ee")]
        if use_v2_p6 {
            request.security = super::GroupSecurityRequirement::Default;
        }
        let p4_result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .add_member_async(request, None)
        .await;
        #[cfg(feature = "group-e2ee")]
        let mut result = match p4_result {
            Ok(result) => result,
            Err(error) if use_v2_p6 && group_error_is_already_member(&error) => {
                let authoritative_member = self
                    .authoritative_active_member_async(
                        crate::ids::GroupRef::parse(&group)?,
                        &resolved_member,
                    )
                    .await?
                    .ok_or_else(|| crate::ImError::LocalStateUnavailable {
                        detail: "group.add reported an existing member but the authoritative roster did not identify it"
                            .to_owned(),
                    })?;
                let authoritative = self
                    .get_authoritative_group_async(crate::ids::GroupRef::parse(&group)?)
                    .await?;
                let state_ref =
                    group_state_ref_from_result(&group, &authoritative).ok_or_else(|| {
                        crate::ImError::LocalStateUnavailable {
                            detail: "authoritative group.get omitted group_state_ref".to_owned(),
                        }
                    })?;
                crate::internal::group_e2ee::v2_lifecycle::add_active_member_devices_async(
                    self.client,
                    crate::internal::group_e2ee::v2_lifecycle::v2_group_state_ref(state_ref),
                    authoritative_member.did.as_str(),
                )
                .await?;
                let mut result = authoritative;
                result.resolved_member = Some(authoritative_member);
                self.refresh_group_state_for_async(&mut result, &group, true)
                    .await;
                return Ok(result);
            }
            Err(error) => return Err(error),
        };
        #[cfg(not(feature = "group-e2ee"))]
        let mut result = p4_result?;
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
            crate::internal::group_e2ee::v2_lifecycle::add_active_member_devices_async(
                self.client,
                transition.group_state_ref,
                &transition.member_did,
            )
            .await?;
        }
        Ok(result)
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
        let v2_route = {
            let authoritative = self.get_authoritative_group(request.group.clone())?;
            v2_member_mutation_route(&group, &authoritative, secure_required)?
        };
        #[cfg(feature = "group-e2ee")]
        let use_v2_p6 = v2_route == V2MemberMutationRoute::OwnerP6;
        #[cfg(feature = "group-e2ee")]
        let resolved_member = if use_v2_p6 {
            require_group_e2ee_v2(self.client)?;
            self.authoritative_active_member(
                crate::ids::GroupRef::parse(&group)?,
                &resolved_member,
            )?
            .unwrap_or_else(|| resolved_member.clone())
        } else {
            resolved_member
        };
        #[cfg(feature = "group-e2ee")]
        if use_v2_p6 {
            crate::internal::group_e2ee::v2_lifecycle::preflight_current_controller(
                self.client,
                &group,
            )?;
        }
        #[allow(unused_mut)]
        let mut p4_request = resolved_group_member_request(request, &resolved_member);
        #[cfg(feature = "group-e2ee")]
        if use_v2_p6 {
            p4_request.security = super::GroupSecurityRequirement::Default;
        }
        let p4_result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .remove_member(p4_request, None);
        #[cfg(feature = "group-e2ee")]
        let mut result = match p4_result {
            Ok(result) => result,
            Err(error) if use_v2_p6 && group_error_is_not_member(&error) => {
                let authoritative =
                    self.get_authoritative_group(crate::ids::GroupRef::parse(&group)?)?;
                crate::internal::group_e2ee::v2_lifecycle::reconcile_group_device_roster(
                    self.client,
                    crate::ids::GroupRef::parse(&group)?,
                )?;
                let mut result = authoritative;
                result.resolved_member = requested_member_is_did.then_some(resolved_member);
                self.refresh_group_state_for(&mut result, &group, true);
                return Ok(result);
            }
            Err(error) => return Err(error),
        };
        #[cfg(not(feature = "group-e2ee"))]
        let mut result = p4_result?;
        result.resolved_member = Some(resolved_member.clone());
        crate::internal::group_runtime::projection::project_group_snapshot(self.client, &result);
        self.refresh_group_state_for(&mut result, &group, true);
        #[cfg(feature = "group-e2ee")]
        if use_v2_p6 {
            let transition = crate::internal::group_e2ee::v2_lifecycle::required_member_transition(
                &group,
                Some(resolved_member.did.as_str()),
                "removed",
                &result,
            )?;
            crate::internal::group_e2ee::v2_lifecycle::remove_inactive_member_devices(
                self.client,
                transition.group_state_ref,
                &transition.member_did,
            )?;
            result.resolved_member = Some(super::GroupMemberResolution {
                did: crate::ids::Did::parse(&transition.member_did)?,
                handle: resolved_member.handle,
            });
            self.refresh_group_state_for(&mut result, &group, true);
        }
        Ok(result)
    }

    pub async fn remove_member_async(
        &self,
        request: super::GroupMemberMutationRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let secure_required = request.security.required();
        let group = request.group.as_str().to_string();
        #[cfg(feature = "group-e2ee")]
        let requested_member_is_did = request.member.is_did();
        let resolved_member = resolve_group_member_async(self.client, &request.member).await?;
        #[cfg(not(feature = "group-e2ee"))]
        if secure_required {
            return Err(crate::ImError::unsupported("group-e2ee"));
        }
        #[cfg(feature = "group-e2ee")]
        let v2_route = {
            let authoritative = self
                .get_authoritative_group_async(request.group.clone())
                .await?;
            v2_member_mutation_route(&group, &authoritative, secure_required)?
        };
        #[cfg(feature = "group-e2ee")]
        let use_v2_p6 = v2_route == V2MemberMutationRoute::OwnerP6;
        #[cfg(feature = "group-e2ee")]
        let resolved_member = if use_v2_p6 {
            require_group_e2ee_v2(self.client)?;
            self.authoritative_active_member_async(
                crate::ids::GroupRef::parse(&group)?,
                &resolved_member,
            )
            .await?
            .unwrap_or_else(|| resolved_member.clone())
        } else {
            resolved_member
        };
        #[cfg(feature = "group-e2ee")]
        if use_v2_p6 {
            crate::internal::group_e2ee::v2_lifecycle::preflight_current_controller_async(
                self.client,
                &group,
            )
            .await?;
        }
        #[allow(unused_mut)]
        let mut p4_request = resolved_group_member_request(request, &resolved_member);
        #[cfg(feature = "group-e2ee")]
        if use_v2_p6 {
            p4_request.security = super::GroupSecurityRequirement::Default;
        }
        let p4_result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .remove_member_async(p4_request, None)
        .await;
        #[cfg(feature = "group-e2ee")]
        let mut result = match p4_result {
            Ok(result) => result,
            Err(error) if use_v2_p6 && group_error_is_not_member(&error) => {
                let authoritative = self
                    .get_authoritative_group_async(crate::ids::GroupRef::parse(&group)?)
                    .await?;
                crate::internal::group_e2ee::v2_lifecycle::reconcile_group_device_roster_async(
                    self.client,
                    crate::ids::GroupRef::parse(&group)?,
                )
                .await?;
                let mut result = authoritative;
                result.resolved_member = requested_member_is_did.then_some(resolved_member);
                self.refresh_group_state_for_async(&mut result, &group, true)
                    .await;
                return Ok(result);
            }
            Err(error) => return Err(error),
        };
        #[cfg(not(feature = "group-e2ee"))]
        let mut result = p4_result?;
        result.resolved_member = Some(resolved_member.clone());
        let _ = crate::internal::group_runtime::projection::project_group_snapshot_async(
            self.client,
            &result,
        )
        .await;
        self.refresh_group_state_for_async(&mut result, &group, true)
            .await;
        #[cfg(feature = "group-e2ee")]
        if use_v2_p6 {
            let transition = crate::internal::group_e2ee::v2_lifecycle::required_member_transition(
                &group,
                Some(resolved_member.did.as_str()),
                "removed",
                &result,
            )?;
            crate::internal::group_e2ee::v2_lifecycle::remove_inactive_member_devices_async(
                self.client,
                transition.group_state_ref,
                &transition.member_did,
            )
            .await?;
            result.resolved_member = Some(super::GroupMemberResolution {
                did: crate::ids::Did::parse(&transition.member_did)?,
                handle: resolved_member.handle,
            });
            self.refresh_group_state_for_async(&mut result, &group, true)
                .await;
        }
        Ok(result)
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

    #[cfg(feature = "group-e2ee")]
    async fn authoritative_active_member_async(
        &self,
        group: crate::ids::GroupRef,
        requested: &super::GroupMemberResolution,
    ) -> crate::ImResult<Option<super::GroupMemberResolution>> {
        const MAX_ATTEMPTS: usize = 4;

        for attempt in 0..MAX_ATTEMPTS {
            let authoritative = self.get_authoritative_group_async(group.clone()).await?;
            let raw = authoritative
                .raw_response()
                .ok_or(crate::ImError::InventoryIncomplete)?;
            let max_members =
                crate::internal::group_e2ee::member_collector::product_max_members(raw)?;
            let expected_version = raw
                .get("group_state_version")
                .and_then(serde_json::Value::as_str)
                .ok_or(crate::ImError::InventoryIncomplete)?;
            let roster = match crate::internal::group_e2ee::member_collector::collect_complete_group_members_async(
                self.client,
                group.clone(),
                Some(expected_version),
                max_members,
            )
            .await
            {
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
        let _ = crate::internal::group_rebind_recovery::repair_previous_group_message_directions(
            &self.client.core_inner().sdk_paths().local_state.sqlite_path,
            self.client.current_identity().id.as_str(),
            self.client.did().as_str(),
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
        let _ = crate::internal::group_rebind_recovery::repair_previous_group_message_directions(
            &self.client.core_inner().sdk_paths().local_state.sqlite_path,
            self.client.current_identity().id.as_str(),
            self.client.did().as_str(),
        );
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
fn require_group_e2ee_v2(client: &crate::core::ImClient) -> crate::ImResult<()> {
    if client.core_inner().group_e2ee_v2_enabled() {
        Ok(())
    } else {
        Err(crate::ImError::unsupported("group-e2ee-v2"))
    }
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
    let group_uses_e2ee = authoritative_group_e2ee_classification(group_did, authoritative)?;
    if !group_uses_e2ee && caller_requested_e2ee {
        return Err(crate::ImError::invalid_input(
            Some("security".to_owned()),
            "the authoritative group policy is transport-protected",
        ));
    }
    // P6 v2 has no leave-request method: P4 records `left` first, then an
    // owner device converges every affected Leaf through the existing repair.
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

#[cfg(all(test, feature = "group-e2ee"))]
mod tests;
