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
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .map(|result| result.profile)
    }

    pub async fn profile_async(&self) -> crate::ImResult<super::Profile> {
        self.profile_with_runtime_async(
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .await
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

    pub(crate) async fn profile_with_runtime_async<P, T>(
        &self,
        session_provider: P,
        transport: T,
    ) -> crate::ImResult<crate::internal::profile_runtime::ProfileReadResult>
    where
        P: crate::internal::auth::session::AsyncSessionProvider,
        T: crate::internal::transport::AsyncAuthenticatedRpcTransport,
    {
        crate::internal::profile_runtime::ProfileReader::new(
            self.client,
            session_provider,
            transport,
        )
        .profile_async()
        .await
    }

    pub fn update_profile(&self, patch: super::ProfilePatch) -> crate::ImResult<super::Profile> {
        super::profile::validate_profile_patch(&patch)?;
        self.update_profile_with_runtime(
            patch,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .map(|result| result.profile)
    }

    pub async fn update_profile_async(
        &self,
        patch: super::ProfilePatch,
    ) -> crate::ImResult<super::Profile> {
        super::profile::validate_profile_patch(&patch)?;
        self.update_profile_with_runtime_async(
            patch,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .await
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

    pub(crate) async fn update_profile_with_runtime_async<P, T>(
        &self,
        patch: super::ProfilePatch,
        session_provider: P,
        transport: T,
    ) -> crate::ImResult<crate::internal::profile_runtime::ProfileUpdateResult>
    where
        P: crate::internal::auth::session::AsyncSessionProvider,
        T: crate::internal::transport::AsyncAuthenticatedRpcTransport,
    {
        super::profile::validate_profile_patch(&patch)?;
        crate::internal::profile_runtime::ProfileReader::new(
            self.client,
            session_provider,
            transport,
        )
        .update_profile_async(patch)
        .await
    }

    pub fn bind_contact(
        &self,
        request: super::ContactBindingRequest,
    ) -> crate::ImResult<super::ContactBindingResult> {
        crate::internal::identity_bind_runtime::validate_request(&request)?;
        self.bind_contact_with_runtime(
            request,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .map(|result| result.sdk_result)
    }

    pub async fn bind_contact_async(
        &self,
        request: super::ContactBindingRequest,
    ) -> crate::ImResult<super::ContactBindingResult> {
        crate::internal::identity_bind_runtime::validate_request(&request)?;
        self.bind_contact_with_runtime_async(
            request,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .await
        .map(|result| result.sdk_result)
    }

    pub fn bind_email_status(&self, email: String) -> crate::ImResult<super::ContactBindingResult> {
        self.bind_email_status_with_runtime(
            email,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .map(|result| result.sdk_result)
    }

    pub async fn bind_email_status_async(
        &self,
        email: String,
    ) -> crate::ImResult<super::ContactBindingResult> {
        self.bind_email_status_with_runtime_async(
            email,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .await
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

    pub(crate) async fn bind_contact_with_runtime_async<P, T>(
        &self,
        request: super::ContactBindingRequest,
        session_provider: P,
        transport: T,
    ) -> crate::ImResult<crate::internal::identity_bind_runtime::ContactBindingRuntimeResult>
    where
        P: crate::internal::auth::session::AsyncSessionProvider,
        T: crate::internal::transport::AsyncAuthenticatedRestTransport,
    {
        crate::internal::identity_bind_runtime::ContactBindingRuntime::new(
            self.client,
            session_provider,
            transport,
        )
        .bind_contact_async(request)
        .await
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

    pub(crate) async fn bind_email_status_with_runtime_async<P, T>(
        &self,
        email: String,
        session_provider: P,
        transport: T,
    ) -> crate::ImResult<crate::internal::identity_bind_runtime::ContactBindingRuntimeResult>
    where
        P: crate::internal::auth::session::AsyncSessionProvider,
        T: crate::internal::transport::AsyncAuthenticatedRestTransport,
    {
        crate::internal::identity_bind_runtime::ContactBindingRuntime::new(
            self.client,
            session_provider,
            transport,
        )
        .email_status_async(email)
        .await
    }
}
