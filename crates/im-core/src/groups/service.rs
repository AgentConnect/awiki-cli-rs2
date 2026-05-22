pub struct GroupService<'a> {
    client: &'a crate::core::ImClient,
}

impl<'a> GroupService<'a> {
    pub(crate) fn new(client: &'a crate::core::ImClient) -> Self {
        Self { client }
    }

    pub fn create(
        &self,
        request: super::GroupCreateRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .create(request, None)?;
        crate::internal::group_runtime::projection::project_group_snapshot(self.client, &result);
        Ok(result)
    }

    pub fn join(
        &self,
        request: super::GroupJoinRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .join(request, None)?;
        crate::internal::group_runtime::projection::project_group_snapshot(self.client, &result);
        Ok(result)
    }

    pub fn leave(
        &self,
        request: super::GroupLeaveRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let group = request.group.as_str().to_string();
        let result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .leave(request, None)?;
        crate::internal::group_runtime::projection::project_group_left(self.client, &group);
        Ok(result)
    }

    pub fn add_member(
        &self,
        request: super::GroupMemberMutationRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .add_member(request, None)?;
        crate::internal::group_runtime::projection::project_group_snapshot(self.client, &result);
        Ok(result)
    }

    pub fn remove_member(
        &self,
        request: super::GroupMemberMutationRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .remove_member(request, None)?;
        crate::internal::group_runtime::projection::project_group_snapshot(self.client, &result);
        Ok(result)
    }

    pub fn update_profile(
        &self,
        request: super::GroupUpdateProfileRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .update_profile(request, None)?;
        crate::internal::group_runtime::projection::project_group_snapshot(self.client, &result);
        Ok(result)
    }

    pub fn update_policy(
        &self,
        request: super::GroupUpdatePolicyRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .update_policy(request, None)?;
        crate::internal::group_runtime::projection::project_group_snapshot(self.client, &result);
        Ok(result)
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

    pub fn list(
        &self,
        request: super::GroupListRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        crate::internal::group_runtime::read::GroupReadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .list(request)
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

    pub fn messages(
        &self,
        request: super::GroupMessagesRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        crate::internal::group_runtime::read::GroupReadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .messages(request)
    }
}
