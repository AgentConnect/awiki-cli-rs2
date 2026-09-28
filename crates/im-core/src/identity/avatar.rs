//! Avatar writes own authentication and wire shape; products own image preparation and bytes.

/// Reuse the same request and bytes when a previous outcome is unknown (up to seven days).
#[derive(Clone)]
pub struct SetAvatarRequest {
    pub request_id: String,
    pub expected_profile_version: String,
    /// Prepared, square 512×512 JPEG, at most 512 KiB. Never persisted in Core state.
    pub image_jpeg: Vec<u8>,
}

impl std::fmt::Debug for SetAvatarRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SetAvatarRequest")
            .field("request_id", &self.request_id)
            .field("expected_profile_version", &self.expected_profile_version)
            .field("image_bytes", &self.image_jpeg.len())
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct ClearAvatarRequest {
    pub request_id: String,
    pub expected_profile_version: String,
}

impl super::IdentityService<'_> {
    pub async fn set_avatar_async(
        &self,
        request: SetAvatarRequest,
    ) -> crate::ImResult<super::Profile> {
        let call = crate::internal::identity_wire::profile::build_set_avatar_rpc_call(request)?;
        self.mutate_avatar_async(call).await
    }

    pub async fn clear_avatar_async(
        &self,
        request: ClearAvatarRequest,
    ) -> crate::ImResult<super::Profile> {
        let call = crate::internal::identity_wire::profile::build_clear_avatar_rpc_call(request)?;
        self.mutate_avatar_async(call).await
    }

    async fn mutate_avatar_async(
        &self,
        call: crate::internal::identity_wire::RpcCall,
    ) -> crate::ImResult<super::Profile> {
        crate::internal::profile_runtime::ProfileReader::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .mutate_avatar_async(call)
        .await
    }
}

#[cfg(test)]
#[path = "avatar_tests.rs"]
mod tests;
