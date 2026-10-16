use super::*;
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub(super) struct Persistence {
    path: PathBuf,
    key: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    format_version: u32,
    key: String,
    snapshot: CapabilitySnapshot,
}

pub(crate) fn tenant_key(config: &crate::ImCoreConfig, paths: &crate::ImCorePaths) -> String {
    // Do not key on rotating tokens, product version, or runtime generations.
    hash(&serde_json::json!([
        config.service_base_url,
        config.user_service_endpoint,
        config.message_service_endpoint,
        config.did_domain,
        paths.local_state.sqlite_path,
        paths.identities.registry_path,
        CONTRACT_VERSION
    ]))
}

fn hash(value: &impl Serialize) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).expect("serializable capability key"))
    )
}

impl Persistence {
    pub(super) fn public(config: &crate::ImCoreConfig, paths: &crate::ImCorePaths) -> Self {
        let key = tenant_key(config, paths);
        Self {
            path: paths
                .runtime
                .cache_dir
                .join("backend-capabilities-v1")
                .join(&key)
                .join("public.json"),
            key,
        }
    }

    pub(super) fn identity(
        core: &crate::core::ImCoreInner,
        runtime: &crate::internal::identity_runtime::ClientIdentityRuntime,
    ) -> Self {
        let tenant = tenant_key(core.sdk_config(), core.sdk_paths());
        let owner = hash(&runtime.owner.identity_id);
        let authorization = runtime.owner.sync_account.as_ref().map(|s| {
            serde_json::json!([
                s.account_id,
                s.protocol_device_id,
                s.identity_generation.get(),
                s.device_auth_generation,
                s.device_signing_key_id,
                s.device_e2ee_key_id,
                s.role,
                s.management_ready
            ])
        });
        let key = hash(&serde_json::json!([
            tenant,
            runtime.owner.identity_id,
            runtime.owner.current_did,
            authorization
        ]));
        Self {
            path: core
                .sdk_paths()
                .runtime
                .cache_dir
                .join("backend-capabilities-v1")
                .join(tenant)
                .join(owner)
                .join(format!("{key}.json")),
            key,
        }
    }

    pub(super) fn read(&self) -> Option<CapabilitySnapshot> {
        let file = std::fs::File::open(&self.path).ok()?;
        if file.metadata().ok()?.len() > MAX_RESPONSE_BYTES as u64 {
            return None;
        }
        let mut bytes = Vec::new();
        file.take(MAX_RESPONSE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        let envelope: Envelope = serde_json::from_slice(&bytes).ok()?;
        let snapshot = envelope.snapshot;
        if envelope.format_version != 1
            || envelope.key != self.key
            || snapshot.capabilities_version != CONTRACT_VERSION
            || !matches!(
                snapshot.status,
                DiscoveryStatus::Confirmed | DiscoveryStatus::Incompatible
            )
            || snapshot.fetched_at.is_none()
        {
            return None;
        }
        // Validate the same closed projection used for network data. Disk content
        // must not introduce arbitrary strings into diagnostics or subscriptions.
        for (key, capability) in &snapshot.capabilities {
            if !MESSAGE_KEYS.contains(&key.as_str()) && !PUBLIC_KEYS.contains(&key.as_str()) {
                return None;
            }
            if capability
                .reason_code
                .as_deref()
                .is_some_and(|v| v != "not_supported")
            {
                return None;
            }
            let projected = Capability::parse(Some(&serde_json::json!({
                "enabled": capability.support == Support::Supported,
                "protocol_version": capability.protocol_version.unwrap_or(0),
                "limits": capability.limits
            })))
            .ok()?;
            if projected != *capability {
                return None;
            }
        }
        Some(CapabilitySnapshot {
            context: None,
            ..snapshot
        })
    }

    pub(super) fn write(&self, snapshot: &CapabilitySnapshot) -> std::io::Result<()> {
        let parent = self.path.parent().unwrap();
        private_directory(parent)?;
        let mut snapshot = snapshot.clone();
        snapshot.context = None;
        let bytes = serde_json::to_vec(&Envelope {
            format_version: 1,
            key: self.key.clone(),
            snapshot,
        })?;
        let temporary = parent.join(format!(".{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temporary)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            std::fs::rename(&temporary, &self.path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }
}

fn private_directory(path: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

pub(crate) fn remove_identity(core: &crate::ImCore, identity_id: &str) -> std::io::Result<()> {
    if let Some(sessions) = core
        .inner()
        .identity_capability_sessions
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(identity_id)
    {
        for cache in sessions.into_iter().filter_map(|cache| cache.upgrade()) {
            cache.retire();
        }
    }
    let path = core
        .inner()
        .sdk_paths()
        .runtime
        .cache_dir
        .join("backend-capabilities-v1")
        .join(tenant_key(
            core.inner().sdk_config(),
            core.inner().sdk_paths(),
        ))
        .join(hash(&identity_id));
    match std::fs::remove_dir_all(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}
