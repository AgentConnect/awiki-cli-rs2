impl super::MessageService<'_> {
    /// Bounded, persistent device preparation shared by Host activation and sync.
    pub async fn maintain_encrypted_messaging_async(&self) {
        #[cfg(feature = "sqlite")]
        maintain(self.client).await;
    }

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
        #[cfg(feature = "sqlite")]
        let lock = self.client.core_inner().group_lifecycle_lock(
            self.client.current_identity().id.as_str(),
            "device-preparation",
        );
        #[cfg(feature = "sqlite")]
        let _guard = lock.lock().await;
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
                    &format!(
                        "{operation_id}-{}",
                        chrono::Utc::now().timestamp().div_euclid(86400)
                    ),
                    &format!(
                        "{key_package_id}-{}",
                        chrono::Utc::now().timestamp().div_euclid(86400)
                    ),
                )
                .await?;
                for (key, value) in crate::internal::group_runtime::journal::entries(
                    &client,
                    "key-package-replenish:%",
                )?
                .into_iter()
                .filter(|(_, v)| v["completed"] != true)
                .take(8)
                {
                    let Some(notice_id) = value["notice_id"].as_str() else {
                        continue;
                    };
                    crate::internal::group_e2ee::v2_lifecycle::publish_stable_key_package(
                        &client,
                        &device_id,
                        &format!("{operation_id}-{notice_id}"),
                        &format!("{key_package_id}-{notice_id}"),
                    )
                    .await?;
                    crate::internal::group_runtime::journal::save(
                        &client,
                        &key,
                        serde_json::json!({ "notice_id": notice_id, "completed": true }),
                    )?;
                }
            }
            #[cfg(not(feature = "group-e2ee"))]
            return Err(crate::ImError::unsupported("group-e2ee-v2"));
        }
        Ok(())
    }
}

/// Sync-owned bounded preparation. A failure cannot prevent ordinary receive.
#[cfg(feature = "sqlite")]
pub(crate) async fn maintain(client: &crate::core::ImClient) {
    if !client.core_inner().direct_e2ee_v2_enabled() && !client.core_inner().group_e2ee_v2_enabled()
    {
        return;
    }
    let lock = client
        .core_inner()
        .group_lifecycle_lock(client.current_identity().id.as_str(), "device-maintenance");
    let Ok(_guard) = lock.try_lock() else {
        return;
    };
    let key = "device-preparation-status";
    let now = chrono::Utc::now().timestamp();
    let previous = crate::internal::group_runtime::journal::load(client, key)
        .ok()
        .flatten();
    if previous
        .as_ref()
        .and_then(|v| v["next_attempt_at"].as_i64())
        .is_some_and(|at| at > now)
    {
        return;
    }
    let attempts = previous
        .as_ref()
        .and_then(|v| v["attempts"].as_u64())
        .unwrap_or(0)
        .saturating_add(1);
    if crate::internal::group_runtime::journal::save(
        client,
        key,
        serde_json::json!({
            "phase": "preparing", "reason": null, "attempts": attempts,
            "next_attempt_at": now + (5i64 << attempts.min(6)).min(300),
        }),
    )
    .is_err()
    {
        return;
    }
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(8),
        client.messages().prepare_encrypted_messaging_async(),
    )
    .await;
    let (ready, reason) = match result {
        Ok(Ok(())) => (true, None),
        Ok(Err(_)) => (false, Some("device_preparation_failed")),
        Err(_) => (false, Some("device_preparation_timeout")),
    };
    let delay = if ready {
        300
    } else {
        (5i64 << attempts.min(6)).min(300)
    };
    let _ = crate::internal::group_runtime::journal::save(
        client,
        key,
        serde_json::json!({
            "phase": if ready { "ready" } else { "retry" }, "reason": reason,
            "attempts": if ready { 0 } else { attempts }, "next_attempt_at": now + delay,
        }),
    );
}
