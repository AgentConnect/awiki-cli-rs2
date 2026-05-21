pub struct IdentityService<'a> {
    client: &'a crate::core::ImClient,
}

impl<'a> IdentityService<'a> {
    pub(crate) fn new(client: &'a crate::core::ImClient) -> Self {
        Self { client }
    }

    pub fn profile(&self) -> crate::ImResult<super::Profile> {
        self.profile_with_runtime(
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::UnavailableTransport,
        )
        .map(|result| result.profile)
    }

    pub(crate) fn profile_with_runtime<P, T>(
        &self,
        session_provider: P,
        transport: T,
    ) -> crate::ImResult<crate::internal::profile_runtime::ProfileReadResult>
    where
        P: crate::internal::auth::session::SessionProvider,
        T: crate::internal::transport::AuthenticatedRpcTransport,
    {
        crate::internal::profile_runtime::ProfileReader::new(
            self.client,
            session_provider,
            transport,
        )
        .profile()
    }

    pub fn update_profile(&self, patch: super::ProfilePatch) -> crate::ImResult<super::Profile> {
        super::profile::validate_profile_patch(&patch)?;
        self.update_profile_with_runtime(
            patch,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::UnavailableTransport,
        )
        .map(|result| result.profile)
    }

    pub(crate) fn update_profile_with_runtime<P, T>(
        &self,
        patch: super::ProfilePatch,
        session_provider: P,
        transport: T,
    ) -> crate::ImResult<crate::internal::profile_runtime::ProfileUpdateResult>
    where
        P: crate::internal::auth::session::SessionProvider,
        T: crate::internal::transport::AuthenticatedRpcTransport,
    {
        super::profile::validate_profile_patch(&patch)?;
        crate::internal::profile_runtime::ProfileReader::new(
            self.client,
            session_provider,
            transport,
        )
        .update_profile(patch)
    }

    pub fn bind_contact(
        &self,
        request: super::ContactBindingRequest,
    ) -> crate::ImResult<super::ContactBindingResult> {
        crate::internal::identity_bind_runtime::validate_request(&request)?;
        self.bind_contact_with_runtime(
            request,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::UnavailableTransport,
        )
        .map(|result| result.sdk_result)
    }

    pub(crate) fn bind_contact_with_runtime<P, T>(
        &self,
        request: super::ContactBindingRequest,
        session_provider: P,
        transport: T,
    ) -> crate::ImResult<crate::internal::identity_bind_runtime::ContactBindingRuntimeResult>
    where
        P: crate::internal::auth::session::SessionProvider,
        T: crate::internal::transport::AuthenticatedRestTransport,
    {
        crate::internal::identity_bind_runtime::ContactBindingRuntime::new(
            self.client,
            session_provider,
            transport,
        )
        .bind_contact(request)
    }

    pub(crate) fn bind_email_status_with_runtime<P, T>(
        &self,
        email: String,
        session_provider: P,
        transport: T,
    ) -> crate::ImResult<crate::internal::identity_bind_runtime::ContactBindingRuntimeResult>
    where
        P: crate::internal::auth::session::SessionProvider,
        T: crate::internal::transport::AuthenticatedRestTransport,
    {
        crate::internal::identity_bind_runtime::ContactBindingRuntime::new(
            self.client,
            session_provider,
            transport,
        )
        .email_status(email)
    }

    pub fn replace_did_plan(
        &self,
        request: super::ReplaceDidPlanRequest,
    ) -> crate::ImResult<super::ReplaceDidPlan> {
        crate::internal::identity_replace_did_plan::plan_replace_did(request)
    }
}
