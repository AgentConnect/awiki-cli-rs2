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
        crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::UnavailableTransport,
        )
        .create(request, None)
    }

    pub fn join(
        &self,
        request: super::GroupJoinRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::UnavailableTransport,
        )
        .join(request, None)
    }

    pub fn leave(
        &self,
        request: super::GroupLeaveRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::UnavailableTransport,
        )
        .leave(request, None)
    }

    pub fn get(&self, group: crate::ids::GroupRef) -> crate::ImResult<super::GroupReadResult> {
        crate::internal::group_runtime::read::GroupReadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::UnavailableTransport,
        )
        .get(group)
    }

    pub fn list(
        &self,
        request: super::GroupListRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        crate::internal::group_runtime::read::GroupReadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::UnavailableTransport,
        )
        .list(request)
    }

    pub fn members(
        &self,
        request: super::GroupMembersRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        crate::internal::group_runtime::read::GroupReadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::UnavailableTransport,
        )
        .members(request)
    }

    pub fn messages(
        &self,
        request: super::GroupMessagesRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        crate::internal::group_runtime::read::GroupReadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::UnavailableTransport,
        )
        .messages(request)
    }
}
