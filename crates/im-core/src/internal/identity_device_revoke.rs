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
    let store = PendingDeviceRevokeStore::from_core(core).map_err(rejected_before_commit)?;
    let (client, authorizing_device_id, authorizing_signing_key_id) =
        crate::internal::identity_device_join::ready_admin_context(core, &identity, None)
            .map_err(rejected_before_commit)?;
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

pub(crate) async fn recover_pending_for_client(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
) -> crate::ImResult<usize> {
    let _guard = core.inner().device_revoke_lock.lock().await;
    let store = PendingDeviceRevokeStore::from_core(core)?;
    let mut remote =
        DeviceRevokeHttpAdapter::new(crate::internal::transport::CoreHttpTransport::new(client));
    let mut resolver = DeviceRevokeDidResolver::new(
        crate::internal::transport::CoreHttpTransport::new_signature_only(client),
    );
    recover_pending_for_client_with_runtime(core, client, &store, &mut remote, &mut resolver).await
}

pub(crate) async fn recover_pending_with_registry(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    registry: &DeviceJoinRemoteRegistry,
) -> crate::ImResult<usize> {
    let _guard = core.inner().device_revoke_lock.lock().await;
    let store = PendingDeviceRevokeStore::from_core(core)?;
    let mut resolver = DeviceRevokeDidResolver::new(
        crate::internal::transport::CoreHttpTransport::new_signature_only(client),
    );
    recover_pending_locked(core, client, &store, registry, &mut resolver).await
}

async fn recover_pending_for_client_with_runtime<R, D>(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    store: &PendingDeviceRevokeStore,
    remote: &mut R,
    resolver: &mut D,
) -> crate::ImResult<usize>
where
    R: DeviceRevokeRemote,
    D: DeviceRevokeDocumentResolver,
{
    let (completed, pending_records) =
        converge_committed_pending(core, client, store, store.list_for_identity(client.did())?)?;
    if pending_records.is_empty() {
        return Ok(completed);
    }
    let registry = remote.registry(client.did()).await?;
    let recovered =
        recover_uncommitted_pending(core, client, store, &registry, pending_records, resolver)
            .await?;
    Ok(completed.saturating_add(recovered))
}

async fn recover_pending_locked<D>(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    store: &PendingDeviceRevokeStore,
    registry: &DeviceJoinRemoteRegistry,
    resolver: &mut D,
) -> crate::ImResult<usize>
where
    D: DeviceRevokeDocumentResolver,
{
    let (completed, pending_records) =
        converge_committed_pending(core, client, store, store.list_for_identity(client.did())?)?;
    if pending_records.is_empty() {
        return Ok(completed);
    }
    let recovered =
        recover_uncommitted_pending(core, client, store, registry, pending_records, resolver)
            .await?;
    Ok(completed.saturating_add(recovered))
}

fn converge_committed_pending(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    store: &PendingDeviceRevokeStore,
    pending_records: Vec<(
        crate::internal::secret_vault::record::SecretRef,
        PendingDeviceRevoke,
    )>,
) -> crate::ImResult<(
    usize,
    Vec<(
        crate::internal::secret_vault::record::SecretRef,
        PendingDeviceRevoke,
    )>,
)> {
    let mut completed = 0usize;
    let mut uncommitted = Vec::new();
    for (secret_ref, pending) in pending_records {
        let Some(remote_result) = pending.remote_result.as_ref() else {
            uncommitted.push((secret_ref, pending));
            continue;
        };
        pending.validate()?;
        converge_local_state(core, client, &pending, remote_result)?;
        store.delete(&secret_ref)?;
        completed = completed.saturating_add(1);
    }
    Ok((completed, uncommitted))
}

async fn recover_uncommitted_pending<D>(
    core: &crate::core::ImCore,
    client: &crate::core::ImClient,
    store: &PendingDeviceRevokeStore,
    registry: &DeviceJoinRemoteRegistry,
    pending_records: Vec<(
        crate::internal::secret_vault::record::SecretRef,
        PendingDeviceRevoke,
    )>,
    resolver: &mut D,
) -> crate::ImResult<usize>
where
    D: DeviceRevokeDocumentResolver,
{
    if registry.did != *client.did() {
        return Err(crate::ImError::PermissionDenied);
    }
    let document = resolver.resolve(client.did()).await?;
    let mut completed = 0usize;
    for (secret_ref, mut pending) in pending_records {
        if pending.remote_result.is_some() {
            return Err(crate::ImError::PermissionDenied);
        }
        let remote_result = recover_remote_result_from_authority(&pending, registry, &document)?;
        pending.remote_result = Some(remote_result.clone());
        if store.save(&pending)? != secret_ref {
            return Err(crate::ImError::PermissionDenied);
        }
        converge_local_state(core, client, &pending, &remote_result)?;
        store.delete(&secret_ref)?;
        completed = completed.saturating_add(1);
    }
    Ok(completed)
}

fn recover_remote_result_from_authority(
    pending: &PendingDeviceRevoke,
    registry: &DeviceJoinRemoteRegistry,
    document: &Value,
) -> crate::ImResult<DeviceRevokeRemoteResult> {
    pending.validate()?;
    let expected_checkpoint = pending.expected_result_checkpoint()?;
    let expected_generation = pending
        .target_auth_generation
        .checked_add(1)
        .ok_or(crate::ImError::PermissionDenied)?;
    if registry.did != pending.did
        || registry.checkpoint != expected_checkpoint
        || registry.checkpoint.document_hash
            != crate::internal::identity_wire::document::document_hash(document)?
        || document.get("id").and_then(Value::as_str) != Some(pending.did.as_str())
        || !anp::authentication::validate_did_document_binding(document, true)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let target = registry
        .devices
        .iter()
        .find(|device| device.device_id == pending.target_device_id)
        .ok_or(crate::ImError::PermissionDenied)?;
    if target.status != DeviceAuthorizationStatus::Revoked
        || target.management_ready
        || target.auth_generation != expected_generation
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let manifest = anp::authentication::validate_device_manifest(document)
        .map_err(|_| crate::ImError::PermissionDenied)?
        .ok_or(crate::ImError::PermissionDenied)?;
    if manifest
        .devices
        .iter()
        .any(|device| device.device_id == pending.target_device_id)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    validate_manifest_device(document, &pending.authorizing_device)?;
    Ok(DeviceRevokeRemoteResult {
        target_device_id: pending.target_device_id.clone(),
        auth_generation: expected_generation,
        checkpoint: expected_checkpoint,
    })
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
    validate_user_presence(user_presence_at, now).map_err(rejected_before_commit)?;
    crate::ids::ProtocolDeviceId::parse(target_device_id).map_err(rejected_before_commit)?;
    if target_device_id == authorizing_device_id {
        return Err(rejected_before_commit(crate::ImError::PermissionDenied));
    }
    let did = client.did().clone();
    let (secret_ref, mut pending) = match store
        .load(&did, target_device_id)
        .map_err(unknown_outcome)?
    {
        Some((secret_ref, pending)) => {
            validate_pending_authorizer(
                &pending,
                &did,
                authorizing_device_id,
                authorizing_signing_key_id,
            )
            .map_err(unknown_outcome)?;
            (secret_ref, pending)
        }
        None => {
            let registry = remote
                .registry(&did)
                .await
                .map_err(redact_remote_error)
                .map_err(rejected_before_commit)?;
            let document = resolver
                .resolve(&did)
                .await
                .map_err(redact_remote_error)
                .map_err(rejected_before_commit)?;
            let pending = prepare_initial_intent(
                client,
                target_device_id,
                authorizing_device_id,
                authorizing_signing_key_id,
                registry,
                document,
            )
            .map_err(rejected_before_commit)?;
            let secret_ref = store.save(&pending).map_err(rejected_before_commit)?;
            (secret_ref, pending)
        }
    };

    if pending.remote_result.is_none() {
        let identity_signer = &client.runtime().key_provider;
        let prepared = crate::internal::identity_wire::device_revoke::prepare_revoke(
            pending.operation_id.clone(),
            pending.target_device_id.clone(),
            pending.expected_checkpoint.clone(),
            pending.new_document.clone(),
            pending.authorizing_device.device_id.clone(),
            &pending.authorizing_device.signing_key_id,
            &|kid, message| identity_signer.sign_device_assertion(kid, message),
            now,
        )
        .map_err(unknown_outcome)?;
        let expected_checkpoint = pending
            .expected_result_checkpoint()
            .map_err(unknown_outcome)?;
        let expected_generation = pending
            .target_auth_generation
            .checked_add(1)
            .ok_or_else(|| unknown_outcome(crate::ImError::PermissionDenied))?;
        let remote_result = match remote
            .revoke(&prepared, expected_generation, &expected_checkpoint)
            .await
        {
            Ok(result) => result,
            Err(error) => {
                if stale_intent_error(&error) {
                    store
                        .delete(&secret_ref)
                        .map_err(|_| unknown_outcome(crate::ImError::PermissionDenied))?;
                    return Err(rejected_before_commit(redact_remote_error(error)));
                }
                return Err(unknown_outcome(redact_remote_error(error)));
            }
        };
        if remote_result.target_device_id != pending.target_device_id
            || remote_result.auth_generation != expected_generation
            || remote_result.checkpoint != expected_checkpoint
        {
            return Err(unknown_outcome(crate::ImError::PermissionDenied));
        }
        pending.remote_result = Some(remote_result);
        if store.save(&pending).map_err(unknown_outcome)? != secret_ref {
            return Err(unknown_outcome(crate::ImError::PermissionDenied));
        }
    }

    let remote_result = pending
        .remote_result
        .as_ref()
        .ok_or_else(|| unknown_outcome(crate::ImError::PermissionDenied))?;
    converge_local_state(core, client, &pending, remote_result).map_err(unknown_outcome)?;
    let target_device_id =
        crate::ids::ProtocolDeviceId::parse(&pending.target_device_id).map_err(unknown_outcome)?;
    store.delete(&secret_ref).map_err(unknown_outcome)?;
    Ok(crate::identity::DeviceRevokeResult {
        did,
        target_device_id,
        status: crate::identity::DeviceRevokeStatus::Revoked,
    })
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
    crate::internal::identity_daemon_subkey::resign_did_document_with_signer(
        &mut new_document,
        did,
        client.runtime().key_provider.as_ref(),
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

fn rejected_before_commit(_error: crate::ImError) -> crate::ImError {
    crate::ImError::DeviceRevokeOutcome {
        category: crate::error::DeviceRevokeOutcomeCategory::RejectedBeforeCommit,
    }
}

fn unknown_outcome(_error: crate::ImError) -> crate::ImError {
    crate::ImError::DeviceRevokeOutcome {
        category: crate::error::DeviceRevokeOutcomeCategory::OutcomeUnknown,
    }
}

#[cfg(test)]
mod tests;
