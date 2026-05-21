pub struct IdentityService<'a> {
    client: &'a crate::core::ImClient,
}

impl<'a> IdentityService<'a> {
    pub(crate) fn new(client: &'a crate::core::ImClient) -> Self {
        Self { client }
    }

    pub fn profile(&self) -> crate::ImResult<super::Profile> {
        Ok(super::profile::profile_from_summary(
            self.client.current_identity(),
        ))
    }

    pub fn update_profile(&self, patch: super::ProfilePatch) -> crate::ImResult<super::Profile> {
        super::profile::validate_profile_patch(&patch)?;
        Err(crate::ImError::unsupported("identity-profile-update"))
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
