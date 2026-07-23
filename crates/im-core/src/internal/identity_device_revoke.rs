//! AWiki-local permanent device revocation orchestration.
//!
//! The client prepares the exact SDK device removal, signs the resulting DID
//! Document with the Vault-backed root key, adds a fresh current-admin proof,
//! and invokes the first-party `device_revoke` RPC. Stable business state is
//! sealed before the RPC so transport retries keep one operation ID. Local
//! Document/checkpoint state advances only after a validated server result.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore as _;
use serde_json::Value;
use std::io::Write as _;
use time::{Duration, OffsetDateTime};
use zeroize::Zeroizing;

use crate::internal::identity_device_join_runtime::{
    DeviceJoinRemoteDeviceSummary, DeviceJoinRemoteRegistry,
};
use crate::internal::identity_device_revoke_pending::{
    PendingDeviceRevoke, PendingDeviceRevokeStore,
};
use crate::internal::identity_device_state::{
    DeviceAuthorizationProjection, DeviceAuthorizationRole, DeviceAuthorizationStatus,
    IdentityDeviceMode, IdentityDeviceState, IdentityInternalCheckpoint,
    IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
};
use crate::internal::identity_wire::device_revoke::{
    DeviceRevokeRemoteResult, PreparedDeviceRevoke,
};
use crate::internal::transport::{AsyncAuthenticatedRpcTransport, AsyncRawJsonTransport};

const USER_PRESENCE_MAX_AGE_SECONDS: i64 = 120;
const USER_PRESENCE_FUTURE_SKEW_SECONDS: i64 = 30;

pub(crate) async fn revoke(
    core: &crate::core::ImCore,
    identity: crate::identity::IdentitySelector,
    target_device_id: crate::ids::ProtocolDeviceId,
) -> crate::ImResult<crate::identity::DeviceRevokeResult> {
    // Serialize same-process management mutations so two callers cannot
    // replace the deterministic exact-retry record with different intents.
    let _guard = core.inner().device_revoke_lock.lock().await;
    // Require the authenticated Vault exact-retry boundary before identity or
    // network access. Public rollout and user-presence gates run even earlier.
    let store = PendingDeviceRevokeStore::from_core(core)?;
    let (client, authorizing_device_id, authorizing_signing_key_id) =
        crate::internal::identity_device_join::ready_admin_context(core, &identity, None)?;
    let now = OffsetDateTime::now_utc();
    let mut remote =
        DeviceRevokeHttpAdapter::new(crate::internal::transport::CoreHttpTransport::new(&client));
    let mut resolver = DeviceRevokeDidResolver::new(
        crate::internal::transport::CoreHttpTransport::new_signature_only(&client),
    );
    execute_with_runtime(
        core,
        &client,
        &store,
        &authorizing_device_id,
        &authorizing_signing_key_id,
        target_device_id.as_str(),
        now,
        now,
        &mut remote,
        &mut resolver,
    )
    .await
}

pub(crate) trait DeviceRevokeRemote {
    async fn registry(
        &mut self,
        did: &crate::ids::Did,
    ) -> crate::ImResult<DeviceJoinRemoteRegistry>;

    async fn revoke(
        &mut self,
        prepared: &PreparedDeviceRevoke,
        expected_auth_generation: u64,
        expected_checkpoint: &IdentityInternalCheckpoint,
    ) -> crate::ImResult<DeviceRevokeRemoteResult>;
}

pub(crate) trait DeviceRevokeDocumentResolver {
    async fn resolve(&mut self, did: &crate::ids::Did) -> crate::ImResult<Value>;
}

pub(crate) struct DeviceRevokeHttpAdapter<T> {
    transport: T,
}

impl<T> DeviceRevokeHttpAdapter<T> {
    pub(crate) fn new(transport: T) -> Self {
        Self { transport }
    }
}

impl<T> DeviceRevokeRemote for DeviceRevokeHttpAdapter<T>
where
    T: AsyncAuthenticatedRpcTransport,
{
    async fn registry(
        &mut self,
        did: &crate::ids::Did,
    ) -> crate::ImResult<DeviceJoinRemoteRegistry> {
        let call = crate::internal::identity_wire::device_join::build_registry_call(did, false);
        let raw = self
            .transport
            .authenticated_rpc(call.endpoint, call.method, call.params)
            .await?;
        crate::internal::identity_wire::device_join::parse_registry_result(raw, did, false)
    }

    async fn revoke(
        &mut self,
        prepared: &PreparedDeviceRevoke,
        expected_auth_generation: u64,
        expected_checkpoint: &IdentityInternalCheckpoint,
    ) -> crate::ImResult<DeviceRevokeRemoteResult> {
        let call = crate::internal::identity_wire::device_revoke::build_revoke_call(prepared)?;
        let raw = self
            .transport
            .authenticated_rpc(call.endpoint, call.method, call.params)
            .await?;
        crate::internal::identity_wire::device_revoke::parse_revoke_result(
            raw,
            &prepared.target_device_id,
            expected_auth_generation,
            expected_checkpoint,
        )
    }
}

pub(crate) struct DeviceRevokeDidResolver<T> {
    transport: T,
}

impl<T> DeviceRevokeDidResolver<T> {
    pub(crate) fn new(transport: T) -> Self {
        Self { transport }
    }
}

impl<T> DeviceRevokeDocumentResolver for DeviceRevokeDidResolver<T>
where
    T: AsyncRawJsonTransport,
{
    async fn resolve(&mut self, did: &crate::ids::Did) -> crate::ImResult<Value> {
        crate::internal::discovery::did_document::resolve_did_document_async(
            &mut self.transport,
            did.as_str(),
        )
        .await
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_with_runtime<R, D>(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    store: &PendingDeviceRevokeStore,
    authorizing_device_id: &str,
    authorizing_signing_key_id: &str,
    target_device_id: &str,
    user_presence_at: OffsetDateTime,
    now: OffsetDateTime,
    remote: &mut R,
    resolver: &mut D,
) -> crate::ImResult<crate::identity::DeviceRevokeResult>
where
    R: DeviceRevokeRemote,
    D: DeviceRevokeDocumentResolver,
{
    validate_user_presence(user_presence_at, now)?;
    crate::ids::ProtocolDeviceId::parse(target_device_id)?;
    if target_device_id == authorizing_device_id {
        return Err(crate::ImError::PermissionDenied);
    }
    let did = client.did().clone();
    let (secret_ref, mut pending) = match store.load(&did, target_device_id)? {
        Some((secret_ref, pending)) => {
            validate_pending_authorizer(
                &pending,
                &did,
                authorizing_device_id,
                authorizing_signing_key_id,
            )?;
            (secret_ref, pending)
        }
        None => {
            let registry = remote.registry(&did).await.map_err(redact_remote_error)?;
            let document = resolver.resolve(&did).await.map_err(redact_remote_error)?;
            let pending = prepare_initial_intent(
                client,
                target_device_id,
                authorizing_device_id,
                authorizing_signing_key_id,
                registry,
                document,
            )?;
            let secret_ref = store.save(&pending)?;
            (secret_ref, pending)
        }
    };

    if pending.remote_result.is_none() {
        let signing_pem = Zeroizing::new(
            client
                .runtime()
                .key_provider
                .device_request_signing_private_pem()?,
        );
        let signing_private = anp::PrivateKeyMaterial::from_pem(&signing_pem)
            .map_err(|_| crate::ImError::PermissionDenied)?;
        let prepared = crate::internal::identity_wire::device_revoke::prepare_revoke(
            pending.operation_id.clone(),
            pending.target_device_id.clone(),
            pending.expected_checkpoint.clone(),
            pending.new_document.clone(),
            pending.authorizing_device.device_id.clone(),
            &pending.authorizing_device.signing_key_id,
            &signing_private,
            now,
        )?;
        let expected_checkpoint = pending.expected_result_checkpoint()?;
        let expected_generation = pending
            .target_auth_generation
            .checked_add(1)
            .ok_or(crate::ImError::PermissionDenied)?;
        let remote_result = match remote
            .revoke(&prepared, expected_generation, &expected_checkpoint)
            .await
        {
            Ok(result) => result,
            Err(error) => {
                if stale_intent_error(&error) {
                    store.delete(&secret_ref)?;
                }
                return Err(redact_remote_error(error));
            }
        };
        if remote_result.target_device_id != pending.target_device_id
            || remote_result.auth_generation != expected_generation
            || remote_result.checkpoint != expected_checkpoint
        {
            return Err(crate::ImError::PermissionDenied);
        }
        pending.remote_result = Some(remote_result);
        if store.save(&pending)? != secret_ref {
            return Err(crate::ImError::PermissionDenied);
        }
    }

    let remote_result = pending
        .remote_result
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    converge_local_state(core, client, &pending, remote_result)?;
    converge_revoked_group_leaves(client, &pending.target_device_id).await?;
    store.delete(&secret_ref)?;
    Ok(crate::identity::DeviceRevokeResult {
        did,
        target_device_id: crate::ids::ProtocolDeviceId::parse(&pending.target_device_id)?,
        status: crate::identity::DeviceRevokeStatus::Revoked,
    })
}

#[cfg(feature = "group-e2ee")]
async fn converge_revoked_group_leaves(
    client: &crate::core::ImClient,
    target_device_id: &str,
) -> crate::ImResult<()> {
    if !client.core_inner().group_e2ee_v2_enabled() {
        return Ok(());
    }
    let client = client.clone();
    let target_device_id = target_device_id.to_owned();
    crate::internal::runtime::worker::run_blocking(move || {
        crate::internal::group_e2ee::v2_lifecycle::remove_revoked_device_from_owned_groups(
            &client,
            &target_device_id,
        )
    })
    .await
    .map_err(|error| crate::ImError::Internal {
        message: format!("revoked-device P6 convergence worker failed: {error}"),
    })??;
    Ok(())
}

#[cfg(not(feature = "group-e2ee"))]
async fn converge_revoked_group_leaves(
    _client: &crate::core::ImClient,
    _target_device_id: &str,
) -> crate::ImResult<()> {
    Ok(())
}

fn prepare_initial_intent(
    client: &crate::core::ImClient,
    target_device_id: &str,
    authorizing_device_id: &str,
    authorizing_signing_key_id: &str,
    registry: DeviceJoinRemoteRegistry,
    document: Value,
) -> crate::ImResult<PendingDeviceRevoke> {
    let did = client.did();
    if registry.did != *did
        || registry.checkpoint.document_hash
            != crate::internal::identity_wire::document::document_hash(&document)?
        || document.get("id").and_then(Value::as_str) != Some(did.as_str())
        || !anp::authentication::validate_did_document_binding(&document, true)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let mut seen = std::collections::BTreeSet::new();
    if registry
        .devices
        .iter()
        .any(|device| !seen.insert(device.device_id.as_str()))
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let authorizing = registry
        .devices
        .iter()
        .find(|device| device.device_id == authorizing_device_id)
        .cloned()
        .ok_or(crate::ImError::PermissionDenied)?;
    if authorizing.signing_key_id != authorizing_signing_key_id
        || authorizing.status != DeviceAuthorizationStatus::Active
        || authorizing.role != DeviceAuthorizationRole::Admin
        || !authorizing.management_ready
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let target = registry
        .devices
        .iter()
        .find(|device| device.device_id == target_device_id)
        .cloned()
        .ok_or_else(|| crate::ImError::IdentityNotFound {
            selector: target_device_id.to_owned(),
        })?;
    if target.status != DeviceAuthorizationStatus::Active {
        return Err(crate::ImError::PermissionDenied);
    }
    let ready_admin_count = registry
        .devices
        .iter()
        .filter(|device| {
            device.status == DeviceAuthorizationStatus::Active
                && device.role == DeviceAuthorizationRole::Admin
                && device.management_ready
        })
        .count();
    if target.role == DeviceAuthorizationRole::Admin
        && target.management_ready
        && ready_admin_count <= 1
    {
        return Err(crate::ImError::PermissionDenied);
    }
    validate_manifest_device(&document, &authorizing)?;
    validate_manifest_device(&document, &target)?;

    let root_key_id = format!("{}#key-1", did.as_str());
    let mut new_document = anp::authentication::remove_device_from_did_document(
        &document,
        &root_key_id,
        target_device_id,
    )
    .map_err(|_| crate::ImError::PermissionDenied)?;
    let root_private_pem = Zeroizing::new(
        client
            .runtime()
            .key_provider
            .did_document_root_private_pem()?,
    );
    crate::internal::identity_daemon_subkey::resign_did_document_with_key1(
        &mut new_document,
        did,
        &root_private_pem,
    )?;
    validate_manifest_device(&new_document, &authorizing)?;
    if anp::authentication::validate_device_manifest(&new_document)
        .map_err(|_| crate::ImError::PermissionDenied)?
        .ok_or(crate::ImError::PermissionDenied)?
        .devices
        .iter()
        .any(|device| device.device_id == target_device_id)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    PendingDeviceRevoke::new(
        did.clone(),
        random_operation_id()?,
        target.device_id,
        target.auth_generation,
        registry.checkpoint,
        new_document,
        authorizing,
    )
}

fn validate_manifest_device(
    document: &Value,
    expected: &DeviceJoinRemoteDeviceSummary,
) -> crate::ImResult<()> {
    let manifest = anp::authentication::validate_device_manifest(document)
        .map_err(|_| crate::ImError::PermissionDenied)?
        .ok_or(crate::ImError::PermissionDenied)?;
    let device = manifest
        .devices
        .iter()
        .find(|device| device.device_id == expected.device_id)
        .ok_or(crate::ImError::PermissionDenied)?;
    if device.signing_key_id != expected.signing_key_id
        || device.e2ee_key_id != expected.e2ee_key_id
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn validate_pending_authorizer(
    pending: &PendingDeviceRevoke,
    did: &crate::ids::Did,
    authorizing_device_id: &str,
    authorizing_signing_key_id: &str,
) -> crate::ImResult<()> {
    pending.validate()?;
    if pending.did != *did
        || pending.authorizing_device.device_id != authorizing_device_id
        || pending.authorizing_device.signing_key_id != authorizing_signing_key_id
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn converge_local_state(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    pending: &PendingDeviceRevoke,
    remote: &DeviceRevokeRemoteResult,
) -> crate::ImResult<()> {
    pending.validate()?;
    if remote.checkpoint != pending.expected_result_checkpoint()? {
        return Err(crate::ImError::PermissionDenied);
    }
    write_document_atomic(&client.runtime().did_document_path, &pending.new_document)?;
    let local_alias = client
        .current_identity()
        .local_alias
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or(crate::ImError::PermissionDenied)?;
    crate::internal::identity_store::IdentityStore::new(&core.inner().sdk_paths().identities)
        .save_device_state(
            local_alias,
            IdentityDeviceState {
                schema_version: IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
                mode: IdentityDeviceMode::VNext,
                authorization: Some(DeviceAuthorizationProjection {
                    protocol_device_id: crate::ids::ProtocolDeviceId::parse(
                        &pending.authorizing_device.device_id,
                    )?,
                    signing_key_id: pending.authorizing_device.signing_key_id.clone(),
                    e2ee_key_id: pending.authorizing_device.e2ee_key_id.clone(),
                    status: DeviceAuthorizationStatus::Active,
                    role: DeviceAuthorizationRole::Admin,
                    management_ready: true,
                    auth_generation: pending.authorizing_device.auth_generation,
                }),
                checkpoint: Some(remote.checkpoint.clone()),
            },
        )
}

fn write_document_atomic(path: &std::path::Path, document: &Value) -> crate::ImResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| crate::ImError::PathUnavailable {
            path_kind: "did_document".to_owned(),
            detail: "DID Document path has no parent".to_owned(),
        })?;
    std::fs::create_dir_all(parent)?;
    let raw = Zeroizing::new(serde_json::to_vec_pretty(document).map_err(|error| {
        crate::ImError::Serialization {
            detail: error.to_string(),
        }
    })?);
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let temporary = path.with_file_name(format!(
        ".{}.{}.{}.tmp",
        path.file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("did-document"),
        std::process::id(),
        nonce,
    ));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let result = (|| -> crate::ImResult<()> {
        let mut file = options.open(&temporary)?;
        file.write_all(raw.as_slice())?;
        file.sync_all()?;
        drop(file);
        crate::internal::atomic_file::replace(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    if let Ok(parent) = std::fs::File::open(parent) {
        let _ = parent.sync_all();
    }
    Ok(())
}

fn validate_user_presence(
    user_presence_at: OffsetDateTime,
    now: OffsetDateTime,
) -> crate::ImResult<()> {
    if user_presence_at > now + Duration::seconds(USER_PRESENCE_FUTURE_SKEW_SECONDS)
        || now - user_presence_at > Duration::seconds(USER_PRESENCE_MAX_AGE_SECONDS)
    {
        return Err(crate::ImError::SessionExpired);
    }
    Ok(())
}

fn random_operation_id() -> crate::ImResult<String> {
    let mut random = [0_u8; 24];
    rand::rngs::OsRng
        .try_fill_bytes(&mut random)
        .map_err(|_| crate::ImError::Internal {
            message: "generate device revoke operation id failed".to_owned(),
        })?;
    Ok(format!("device-revoke-{}", URL_SAFE_NO_PAD.encode(random)))
}

fn stale_intent_error(error: &crate::ImError) -> bool {
    matches!(
        error,
        crate::ImError::Service { code: Some(code), .. }
            if matches!(
                code.as_str(),
                "device.document_version_conflict"
                    | "device.document_hash_conflict"
                    | "device.registry_version_conflict"
                    | "device.inactive"
                    | "device.last_ready_admin"
            )
    )
}

fn redact_remote_error(error: crate::ImError) -> crate::ImError {
    match error {
        crate::ImError::Service {
            status_code, code, ..
        } => crate::ImError::Service {
            status_code,
            code,
            message: "device revoke request failed".to_owned(),
            data: None,
        },
        crate::ImError::TransportUnavailable { .. } => crate::ImError::TransportUnavailable {
            detail: "device revoke transport failed".to_owned(),
        },
        crate::ImError::Serialization { .. } => crate::ImError::Serialization {
            detail: "device revoke response was invalid".to_owned(),
        },
        crate::ImError::Internal { .. } => crate::ImError::Internal {
            message: "device revoke request failed".to_owned(),
        },
        other => other,
    }
}

#[cfg(test)]
mod tests;
