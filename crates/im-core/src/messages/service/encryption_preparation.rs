impl super::MessageService<'_> {
    /// Prepare this exact device to receive enabled encrypted message types.
    ///
    /// Hosts that register identities outside Core must await this before
    /// declaring their message receiver ready. Retrying reuses Core-owned
    /// publication material; no private keys or protocol objects leave Core.
    pub async fn prepare_encrypted_messaging_async(&self) -> crate::ImResult<()> {
        if !self.client.core_inner().direct_e2ee_v2_enabled()
            && !self.client.core_inner().group_e2ee_v2_enabled()
        {
            return Ok(());
        }
        let (client, _) = self.client.reload_local_authorization_async().await?;
        let core = client.core_handle();
        if core.inner().direct_e2ee_v2_enabled() {
            crate::internal::secure_direct::v2_prekey_runtime::ensure_local_prekey_published(
                &core, &client,
            )
            .await?;
        }
        if core.inner().group_e2ee_v2_enabled() {
            #[cfg(feature = "group-e2ee")]
            {
                let device_id = client.exact_protocol_device_id()?;
                let (operation_id, key_package_id) = crate::internal::identity_registration_runtime::deterministic_registration_group_key_package_ids(client.did(), &device_id);
                crate::internal::group_e2ee::v2_lifecycle::publish_stable_key_package(
                    &client,
                    &device_id,
                    &operation_id,
                    &key_package_id,
                )
                .await?;
            }
            #[cfg(not(feature = "group-e2ee"))]
            return Err(crate::ImError::unsupported("group-e2ee-v2"));
        }
        Ok(())
    }
}
