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
        if request.peer.as_str().trim().is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("peer".to_string()),
                "peer must not be empty",
            ));
        }
        Err(crate::ImError::unsupported("identity-bind-contact"))
    }
}
