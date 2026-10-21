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
        let capabilities = self.client.backend_capabilities_async(false).await?;
        let direct_enabled =
            capabilities.support("direct_e2ee") == crate::backend_capabilities::Support::Supported;
        let group_enabled =
            capabilities.support("group_e2ee") == crate::backend_capabilities::Support::Supported;
        if !direct_enabled && !group_enabled {
            return Err(crate::ImError::unsupported(
                "backend.encrypted_messaging.unavailable",
            ));
        }
        #[cfg(feature = "sqlite")]
        let lock = self.client.core_inner().group_lifecycle_lock(
            self.client.current_identity().id.as_str(),
            "device-preparation",
        );
        #[cfg(feature = "sqlite")]
        let _guard = lock.lock().await;
        #[cfg(feature = "sqlite")]
        preparation_stage(self.client, "authorization")?;
        let (client, _) = self.client.reload_local_authorization_async().await?;
        let core = client.core_handle();
        if core.inner().direct_e2ee_v2_enabled() && direct_enabled {
            #[cfg(feature = "sqlite")]
            preparation_stage(&client, "direct_prekey")?;
            crate::internal::secure_direct::v2_prekey_runtime::ensure_local_prekey_published(
                &core, &client,
            )
            .await?;
        }
        if core.inner().group_e2ee_v2_enabled() && group_enabled {
            #[cfg(feature = "group-e2ee")]
            {
                let device_id = client.exact_protocol_device_id()?;
                preparation_stage(&client, "group_key_package")?;
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
    let Ok(capabilities) = client.backend_capabilities_async(false).await else {
        return;
    };
    if capabilities.support("direct_e2ee") != crate::backend_capabilities::Support::Supported
        && capabilities.support("group_e2ee") != crate::backend_capabilities::Support::Supported
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
        Ok(Err(error)) => (false, Some(preparation_failure(&error))),
        Err(_) => (false, Some("device_preparation_timeout")),
    };
    let delay = if ready {
        300
    } else {
        (5i64 << attempts.min(6)).min(300)
    };
    let stage = crate::internal::group_runtime::journal::load(client, key)
        .ok()
        .flatten()
        .and_then(|v| v["stage"].as_str().map(str::to_owned));
    let _ = crate::internal::group_runtime::journal::save(
        client,
        key,
        serde_json::json!({
            "phase": if ready { "ready" } else { "retry" }, "reason": reason,
            "stage": if ready { Some("complete".to_owned()) } else { stage },
            "attempts": if ready { 0 } else { attempts }, "next_attempt_at": now + delay,
        }),
    );
}

#[cfg(feature = "sqlite")]
fn preparation_stage(client: &crate::core::ImClient, stage: &str) -> crate::ImResult<()> {
    let key = "device-preparation-status";
    if let Some(mut value) = crate::internal::group_runtime::journal::load(client, key)? {
        value["stage"] = serde_json::json!(stage);
        crate::internal::group_runtime::journal::save(client, key, value)?;
    }
    Ok(())
}

#[cfg(feature = "sqlite")]
fn preparation_failure(error: &crate::ImError) -> &'static str {
    match error {
        crate::ImError::PermissionDenied => "device_authorization_denied",
        crate::ImError::UnsupportedCapability { .. } => "device_preparation_unsupported",
        crate::ImError::TransportUnavailable { .. } => "device_preparation_transport_failed",
        crate::ImError::Service { .. } => "device_preparation_service_rejected",
        crate::ImError::IdentityVault { .. } => "device_preparation_vault_unavailable",
        crate::ImError::LocalStateUnavailable { .. } => "device_preparation_local_state_failed",
        _ => "device_preparation_failed",
    }
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use super::*;
    #[test]
    fn device_preparation_failure_records_category_without_host_payload() {
        let error = crate::ImError::Service {
            status_code: Some(503),
            code: Some("private-host-code".into()),
            message: "private-host-payload".into(),
            data: Some(serde_json::json!({"private": true})),
        };
        assert_eq!(
            preparation_failure(&error),
            "device_preparation_service_rejected"
        );
        assert_eq!(
            preparation_failure(&crate::ImError::PermissionDenied),
            "device_authorization_denied"
        );
        assert_eq!(
            preparation_failure(&crate::ImError::unsupported("private capability")),
            "device_preparation_unsupported"
        );
    }
}
