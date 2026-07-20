//! AWiki-local root-key transfer state and encrypted-inner JSON hook.
//!
//! This module deliberately stops at the existing P5 v2 plaintext/ciphertext
//! boundary. It does not add fields to standard P5, its AAD, ANP SDK models, or
//! federation. The root private key exists only in SecretVault, a zeroizing
//! envelope value, and a zeroizing plaintext buffer immediately before/after
//! the P5 v2 AEAD hook.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::internal::identity_device_join_runtime::{
    DeviceJoinRemoteDeviceSummary, DeviceJoinRemoteRegistry,
};
use crate::internal::identity_device_state::{
    DeviceAuthorizationRole, DeviceAuthorizationStatus, IdentityDeviceMode,
    IdentityInternalCheckpoint,
};
use crate::internal::identity_store::{
    IdentityStore, IdentityVaultMigrationStatus, IndexEntry, SaveIdentitySecretStorage,
};
use crate::internal::platform_secret::SecretBytes;
use crate::internal::secret_vault::policy::SecretAccessPolicy;
use crate::internal::secret_vault::record::{SecretKind, SecretMetadata, SecretRef};
use crate::internal::secret_vault::{SealIfAbsentResult, SealSecretRequest, SecretVault};

pub(crate) const ROOT_KEY_ENVELOPE_SYSTEM_TYPE: &str = "awiki.device.root-key.v1";
pub(crate) const ROOT_KEY_IMPORTED_SYSTEM_TYPE: &str = "awiki.device.root-key-imported.v1";
pub(crate) const ROOT_KEY_CONTROL_DELIVERY_CLASS: &str = "awiki-root-key-control";
const ROOT_KEY_IMPORT_RESULT: &str = "imported";
const ROOT_KEY_IMPORT_SCHEMA_VERSION: u32 = 1;
const ROOT_KEY_CONTROL_MAX_TTL_SECONDS: i64 = 300;
const USER_PRESENCE_MAX_AGE_SECONDS: i64 = 300;
const USER_PRESENCE_FUTURE_SKEW_SECONDS: i64 = 30;
const ROOT_IMPORT_PENDING_FILE: &str = ".root-key-import.pending.json";
const ROOT_CONTROL_MAX_INNER_BYTES: usize = 64 * 1024;
const ROOT_PRIVATE_PEM_MAX_BYTES: usize = 16 * 1024;
const ROOT_CONTROL_ID_MAX_BYTES: usize = 2 * 1024;
const ROOT_CONTROL_DEVICE_ID_MAX_BYTES: usize = 256;
const ROOT_CONTROL_TIMESTAMP_MAX_BYTES: usize = 64;
const MAX_DOCUMENT_VERSION: u64 = i64::MAX as u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct RootKeyTransferGate {
    enabled: bool,
}

impl RootKeyTransferGate {
    pub(crate) const fn from_rollout_flag(enabled: bool) -> Self {
        Self { enabled }
    }

    fn require_enabled(self) -> crate::ImResult<()> {
        if self.enabled {
            Ok(())
        } else {
            Err(crate::ImError::unsupported(
                "awiki-root-key-transfer-disabled",
            ))
        }
    }
}

pub(crate) struct RootKeyTransferCore<'a> {
    paths: &'a crate::paths::IdentityRegistryPaths,
    secret_storage: SaveIdentitySecretStorage,
    gate: RootKeyTransferGate,
}

impl<'a> RootKeyTransferCore<'a> {
    /// Constructs the pure Core boundary. Production callers must pass their
    /// explicit AWiki rollout gate; there is intentionally no implicit enable.
    pub(crate) fn from_core_for_rollout(
        core: &'a crate::core::ImCore,
        enabled: bool,
    ) -> crate::ImResult<Self> {
        Ok(Self {
            paths: &core.inner().sdk_paths().identities,
            secret_storage: SaveIdentitySecretStorage::from_core(core)?,
            gate: RootKeyTransferGate::from_rollout_flag(enabled),
        })
    }

    #[cfg(test)]
    fn for_test(
        paths: &'a crate::paths::IdentityRegistryPaths,
        secret_storage: SaveIdentitySecretStorage,
        enabled: bool,
    ) -> Self {
        Self {
            paths,
            secret_storage,
            gate: RootKeyTransferGate::from_rollout_flag(enabled),
        }
    }

    /// Opens the sender root from Vault and returns only a zeroizing plaintext
    /// buffer for the existing P5 v2 AEAD hook plus AWiki-private sidecar data.
    pub(crate) fn prepare_envelope(
        &self,
        input: RootKeyEnvelopePrepareInput<'_>,
    ) -> crate::ImResult<PreparedRootKeyEnvelope> {
        self.gate.require_enabled()?;
        validate_message_id(input.message_id)?;
        // PostgreSQL TIMESTAMPTZ normalizes offsets and truncates beyond
        // microseconds. Emit whole-second expiry so the DB projection can
        // round-trip the same instant without changing protocol semantics.
        let expires_at = canonical_control_expiry(input.expires_at)?;
        validate_short_window(input.now, expires_at)?;
        validate_user_presence(input.user_presence_at, input.now)?;
        crate::ids::ProtocolDeviceId::parse(input.recipient_device_id)?;

        let store = IdentityStore::new(self.paths);
        let index = store.load_index()?;
        let entry = local_entry(&index, input.local_alias)?;
        let did = crate::ids::Did::parse(&entry.did)?;
        let (sender, recipient) = validate_current_transfer_route(
            entry,
            &did,
            input.did_document,
            input.registry,
            input.recipient_device_id,
        )?;

        let vault_context = require_vault_context(entry, &self.secret_storage)?;
        let root_ref = vault_context
            .refs
            .did_document_root_private
            .as_ref()
            .ok_or_else(|| crate::ImError::IdentityNotReady {
                identity: did.as_str().to_owned(),
                missing: vec!["did_document_root_private_key".to_owned()],
            })?;
        validate_secret_ref(
            root_ref,
            &vault_context,
            SecretKind::IdentityRootPrivate,
            None,
        )?;
        let root_secret = vault_context.vault.open(root_ref)?;
        let root_pem = std::str::from_utf8(root_secret.expose_secret()).map_err(|_| {
            crate::ImError::CredentialFileUnreadable {
                path_kind: "did_document_root_private_key".to_owned(),
                detail: "Vault root private key is not UTF-8".to_owned(),
            }
        })?;
        let validated_root = validate_root_document(input.did_document, &did, &root_ref.key_id)?;
        validate_root_private(root_pem, &validated_root)?;

        let expires_at = format_time(expires_at)?;
        let envelope = RootKeyEnvelope {
            system_type: ROOT_KEY_ENVELOPE_SYSTEM_TYPE.to_owned(),
            message_id: input.message_id.to_owned(),
            did: did.as_str().to_owned(),
            root_key_id: root_ref.key_id.clone(),
            document_version: input.registry.checkpoint.document_version,
            document_hash: input.registry.checkpoint.document_hash.clone(),
            sender_device_id: sender.device_id.clone(),
            recipient_device_id: recipient.device_id.clone(),
            expires_at: expires_at.clone(),
            root_private_key: root_pem.to_owned(),
        };
        envelope.validate_shape()?;
        let plaintext = encode_zeroizing_json(&envelope)?;
        drop(envelope);
        drop(root_secret);
        Ok(PreparedRootKeyEnvelope {
            plaintext,
            transport_context: RootImportTransportContext {
                message_id: input.message_id.to_owned(),
                delivery_class: ROOT_KEY_CONTROL_DELIVERY_CLASS.to_owned(),
                sender_device_id: sender.device_id.clone(),
                recipient_device_id: recipient.device_id.clone(),
                expires_at,
            },
        })
    }

    /// Imports one decrypted RootKeyEnvelope and creates the sole signed ACK.
    /// Exact replay returns byte-equivalent completion data without re-sealing
    /// or replacing the root Vault record.
    pub(crate) fn import_envelope(
        &self,
        plaintext: &SecretBytes,
        input: RootKeyEnvelopeImportInput<'_>,
    ) -> crate::ImResult<ImportedRootKeyAck> {
        self.gate.require_enabled()?;
        let envelope = decode_root_envelope(plaintext)?;
        envelope.validate_shape()?;
        input.transport_context.validate()?;
        input.direct_binding.validate_for_envelope(&envelope)?;
        input.transport_context.validate_for_envelope(&envelope)?;

        let store = IdentityStore::new(self.paths);
        let index_lock = store.lock_index_mutation()?;
        let mut index = store.load_index()?;
        let entry = local_entry(&index, input.local_alias)?.clone();
        let identity_dir = store.local_identity_dir(&entry.dir_name)?;
        let did = crate::ids::Did::parse(&entry.did)?;
        if envelope.did != did.as_str() || input.current_registry.did != did {
            return denied();
        }
        let reservation = envelope.reservation();

        if let Some(persisted) = entry.root_key_import.as_ref() {
            if persisted.schema_version != ROOT_KEY_IMPORT_SCHEMA_VERSION {
                return denied();
            }
            validate_reservation_shape(&persisted.reservation)?;
            if persisted.reservation == reservation {
                let replay = self.validate_exact_replay(
                    &entry,
                    &envelope,
                    input.current_did_document,
                    input.current_registry,
                    input.direct_binding,
                    persisted,
                )?;
                let target = index
                    .credentials
                    .get_mut(input.local_alias)
                    .ok_or_else(|| crate::ImError::IdentityNotFound {
                        selector: input.local_alias.to_owned(),
                    })?;
                let checkpoint_changed = target
                    .device_state
                    .as_ref()
                    .and_then(|state| state.checkpoint.as_ref())
                    != Some(&input.current_registry.checkpoint);
                if checkpoint_changed {
                    target
                        .device_state
                        .as_mut()
                        .ok_or(crate::ImError::PermissionDenied)?
                        .checkpoint = Some(input.current_registry.checkpoint.clone());
                    store.save_index_locked(&index_lock, index)?;
                }
                // The index/checkpoint is authoritative. The DID document is
                // a repairable local projection; if this write fails after a
                // committed import, the same exact Envelope replay retries it.
                store.save_did_document(&entry.dir_name, input.current_did_document)?;
                remove_matching_pending(&identity_dir, &reservation);
                return imported_ack(replay, input.transport_context.clone(), true);
            }
            if persisted.reservation.message_id == reservation.message_id {
                return denied();
            }

            let previous_expires_at =
                parse_time("root_import.expires_at", &persisted.reservation.expires_at)?;
            if input.now < previous_expires_at {
                return denied();
            }
            validate_short_window(
                input.now,
                parse_time("envelope.expires_at", &envelope.expires_at)?,
            )?;
            let validated = validate_fresh_import(
                &entry,
                &did,
                &envelope,
                input.current_did_document,
                input.current_registry,
                input.direct_binding,
            )?;
            let vault_context = require_vault_context(&entry, &self.secret_storage)?;
            let root_ref = vault_context
                .refs
                .did_document_root_private
                .as_ref()
                .ok_or(crate::ImError::PermissionDenied)?;
            validate_secret_ref(
                root_ref,
                &vault_context,
                SecretKind::IdentityRootPrivate,
                Some(&envelope.root_key_id),
            )?;
            let existing_root = vault_context.vault.open(root_ref)?;
            if existing_root.expose_secret() != envelope.root_private_key.as_bytes() {
                return denied();
            }
            let completion = sign_completion(
                &envelope,
                &validated.current_root.fingerprint,
                input.now,
                validated.recipient,
                input.current_did_document,
                &vault_context,
            )?;
            validate_completion_for_envelope(&completion, &envelope, input.transport_context)?;

            let target = index
                .credentials
                .get_mut(input.local_alias)
                .ok_or_else(|| crate::ImError::IdentityNotFound {
                    selector: input.local_alias.to_owned(),
                })?;
            if target.root_key_import.as_ref() != Some(persisted) {
                return denied();
            }
            let target_root_ref = target
                .vault_migration
                .as_ref()
                .and_then(|metadata| metadata.vnext_refs.as_ref())
                .and_then(|refs| refs.did_document_root_private.as_ref())
                .ok_or(crate::ImError::PermissionDenied)?;
            if target_root_ref != root_ref {
                return denied();
            }
            if let Some(state) = target.device_state.as_mut() {
                state.checkpoint = Some(input.current_registry.checkpoint.clone());
            }
            let previous_reservation = persisted.reservation.clone();
            target.root_key_import = Some(PersistedRootKeyImport {
                schema_version: ROOT_KEY_IMPORT_SCHEMA_VERSION,
                reservation: reservation.clone(),
                completion: completion.clone(),
                management_token_operation_id: None,
            });
            index.schema_version = 5;
            // No Vault write occurs in this retry path. The index replacement
            // atomically moves the replay/ACK reservation to the new message.
            store.save_index_locked(&index_lock, index)?;
            store.save_did_document(&entry.dir_name, input.current_did_document)?;
            remove_matching_pending(&identity_dir, &previous_reservation);
            return imported_ack(completion, input.transport_context.clone(), false);
        }

        validate_short_window(
            input.now,
            parse_time("envelope.expires_at", &envelope.expires_at)?,
        )?;
        let validated = validate_fresh_import(
            &entry,
            &did,
            &envelope,
            input.current_did_document,
            input.current_registry,
            input.direct_binding,
        )?;

        let vault_context = require_vault_context(&entry, &self.secret_storage)?;
        if vault_context.refs.did_document_root_private.is_some() {
            return denied();
        }
        validate_secret_ref(
            &vault_context.refs.device_request_signing_private,
            &vault_context,
            SecretKind::IdentityDeviceSigningPrivate,
            Some(&validated.recipient.signing_key_id),
        )?;
        let completion = sign_completion(
            &envelope,
            &validated.current_root.fingerprint,
            input.now,
            validated.recipient,
            input.current_did_document,
            &vault_context,
        )?;
        validate_completion_for_envelope(&completion, &envelope, input.transport_context)?;

        ensure_pending_reservation(&identity_dir, &reservation, input.now)?;
        let root_ref = expected_root_ref(&vault_context, &envelope.root_key_id);
        seal_or_resume_root(
            &vault_context,
            &root_ref,
            envelope.root_private_key.as_bytes(),
        )?;

        let target = index
            .credentials
            .get_mut(input.local_alias)
            .ok_or_else(|| crate::ImError::IdentityNotFound {
                selector: input.local_alias.to_owned(),
            })?;
        let metadata = target
            .vault_migration
            .as_mut()
            .ok_or(crate::ImError::PermissionDenied)?;
        let refs = metadata
            .vnext_refs
            .as_mut()
            .ok_or(crate::ImError::PermissionDenied)?;
        if refs.did_document_root_private.is_some() || target.root_key_import.is_some() {
            return denied();
        }
        refs.did_document_root_private = Some(root_ref);
        if let Some(state) = target.device_state.as_mut() {
            state.checkpoint = Some(input.current_registry.checkpoint.clone());
        }
        target.root_key_import = Some(PersistedRootKeyImport {
            schema_version: ROOT_KEY_IMPORT_SCHEMA_VERSION,
            reservation: reservation.clone(),
            completion: completion.clone(),
            management_token_operation_id: None,
        });
        index.schema_version = 5;
        // The atomic index image is the linearization point: it contains the
        // root SecretRef, consumed message reservation and signed completion.
        store.save_index_locked(&index_lock, index)?;
        store.save_did_document(&entry.dir_name, input.current_did_document)?;
        remove_matching_pending(&identity_dir, &reservation);
        imported_ack(completion, input.transport_context.clone(), false)
    }

    /// Rebuilds the encrypted ACK hook after a process restart without keeping
    /// RootKeyEnvelope plaintext or the root PEM outside Vault. This is the
    /// normal retry path once the atomic import record exists.
    pub(crate) fn resume_imported_ack(
        &self,
        input: RootKeyAckResumeInput<'_>,
    ) -> crate::ImResult<ImportedRootKeyAck> {
        self.gate.require_enabled()?;
        validate_message_id(input.message_id)?;
        let store = IdentityStore::new(self.paths);
        let _index_lock = store.lock_index_mutation()?;
        let mut index = store.load_index()?;
        let entry = local_entry(&index, input.local_alias)?.clone();
        let identity_dir = store.local_identity_dir(&entry.dir_name)?;
        let did = crate::ids::Did::parse(&entry.did)?;
        let (authorization, local_checkpoint) = require_local_authorization(&entry, &did)?;
        validate_checkpoint_advance(local_checkpoint, &input.current_registry.checkpoint)?;
        let persisted = entry
            .root_key_import
            .as_ref()
            .filter(|persisted| {
                persisted.schema_version == ROOT_KEY_IMPORT_SCHEMA_VERSION
                    && persisted.reservation.message_id == input.message_id
            })
            .ok_or(crate::ImError::PermissionDenied)?;
        let reservation = &persisted.reservation;
        validate_reservation_shape(reservation)?;
        if reservation.did != did.as_str() {
            return denied();
        }
        validate_current_document(&did, input.current_did_document, input.current_registry)?;
        validate_reservation_snapshot_advance(reservation, &input.current_registry.checkpoint)?;
        let current_root =
            validate_root_document(input.current_did_document, &did, &reservation.root_key_id)?;
        validate_completion_for_reservation(
            &persisted.completion,
            reservation,
            &current_root.fingerprint,
        )?;

        let importer =
            unique_registry_device(input.current_registry, &reservation.recipient_device_id)?;
        if importer.status != DeviceAuthorizationStatus::Active
            || importer.role != DeviceAuthorizationRole::Admin
        {
            return denied();
        }
        validate_local_importer(authorization, importer, false)?;
        validate_manifest_device(input.current_did_document, importer)?;
        verify_completion_signature(&persisted.completion, input.current_did_document, importer)?;

        let vault_context = require_vault_context(&entry, &self.secret_storage)?;
        let root_ref = vault_context
            .refs
            .did_document_root_private
            .as_ref()
            .ok_or(crate::ImError::PermissionDenied)?;
        validate_secret_ref(
            root_ref,
            &vault_context,
            SecretKind::IdentityRootPrivate,
            Some(&reservation.root_key_id),
        )?;
        let root_secret = vault_context.vault.open(root_ref)?;
        let root_pem = std::str::from_utf8(root_secret.expose_secret())
            .map_err(|_| crate::ImError::PermissionDenied)?;
        validate_root_private(root_pem, &current_root)?;

        let target = index
            .credentials
            .get_mut(input.local_alias)
            .ok_or_else(|| crate::ImError::IdentityNotFound {
                selector: input.local_alias.to_owned(),
            })?;
        let checkpoint_changed = target
            .device_state
            .as_ref()
            .and_then(|state| state.checkpoint.as_ref())
            != Some(&input.current_registry.checkpoint);
        if checkpoint_changed {
            target
                .device_state
                .as_mut()
                .ok_or(crate::ImError::PermissionDenied)?
                .checkpoint = Some(input.current_registry.checkpoint.clone());
            store.save_index_locked(&_index_lock, index)?;
        }
        store.save_did_document(&entry.dir_name, input.current_did_document)?;
        remove_matching_pending(&identity_dir, reservation);
        imported_ack(
            persisted.completion.clone(),
            RootImportTransportContext::from_reservation(reservation),
            true,
        )
    }

    /// Verifies the signed completion carried by a decrypted ACK before the
    /// original sender treats the import as complete.
    pub(crate) fn validate_imported_ack_plaintext(
        &self,
        plaintext: &SecretBytes,
        transport_context: &RootImportTransportContext,
        current_did_document: &Value,
        current_registry: &DeviceJoinRemoteRegistry,
        now: OffsetDateTime,
    ) -> crate::ImResult<RootKeyImportedCompletion> {
        self.gate.require_enabled()?;
        if plaintext.expose_secret().len() > ROOT_CONTROL_MAX_INNER_BYTES {
            return denied();
        }
        let inner: RootKeyImportedInner = decode_strict_json(plaintext)?;
        if inner.system_type != ROOT_KEY_IMPORTED_SYSTEM_TYPE {
            return denied();
        }
        let completion = inner.completion;
        validate_unsigned_completion(&completion)?;
        transport_context.validate()?;
        if completion.ack_for_message_id != transport_context.message_id
            || completion.sending_device_id != transport_context.sender_device_id
            || completion.importing_device_id != transport_context.recipient_device_id
            || completion.did != current_registry.did.as_str()
            || completion.document_version > current_registry.checkpoint.document_version
            || (completion.document_version == current_registry.checkpoint.document_version
                && completion.document_hash != current_registry.checkpoint.document_hash)
            || completion.device_signature.is_empty()
        {
            return denied();
        }
        let imported_at = parse_time("completion.imported_at", &completion.imported_at)?;
        let expires_at = parse_time(
            "transport_context.expires_at",
            &transport_context.expires_at,
        )?;
        if imported_at > expires_at || imported_at > now + Duration::seconds(30) {
            return denied();
        }
        let did = crate::ids::Did::parse(&completion.did)?;
        validate_current_document(&did, current_did_document, current_registry)?;
        let current_root =
            validate_root_document(current_did_document, &did, &completion.root_key_id)?;
        if current_root.fingerprint != completion.root_public_key_fingerprint {
            return denied();
        }
        let importer =
            require_registry_admin(current_registry, &completion.importing_device_id, true)?;
        validate_manifest_device(current_did_document, importer)?;
        verify_completion_signature(&completion, current_did_document, importer)?;
        Ok(completion)
    }

    fn validate_exact_replay(
        &self,
        entry: &IndexEntry,
        envelope: &RootKeyEnvelope,
        current_document: &Value,
        current_registry: &DeviceJoinRemoteRegistry,
        direct_binding: &RootControlDirectBinding,
        persisted: &PersistedRootKeyImport,
    ) -> crate::ImResult<RootKeyImportedCompletion> {
        let did = crate::ids::Did::parse(&entry.did)?;
        let (authorization, local_checkpoint) = require_local_authorization(entry, &did)?;
        validate_checkpoint_advance(local_checkpoint, &current_registry.checkpoint)?;
        validate_current_document(&did, current_document, current_registry)?;
        validate_snapshot_advance(envelope, &current_registry.checkpoint)?;
        let current_root = validate_root_document(current_document, &did, &envelope.root_key_id)?;
        validate_root_private(&envelope.root_private_key, &current_root)?;
        validate_persisted_completion(&persisted.completion, envelope, &current_root.fingerprint)?;

        let vault_context = require_vault_context(entry, &self.secret_storage)?;
        let root_ref = vault_context
            .refs
            .did_document_root_private
            .as_ref()
            .ok_or(crate::ImError::PermissionDenied)?;
        validate_secret_ref(
            root_ref,
            &vault_context,
            SecretKind::IdentityRootPrivate,
            Some(&envelope.root_key_id),
        )?;
        let opened = vault_context.vault.open(root_ref)?;
        if opened.expose_secret() != envelope.root_private_key.as_bytes() {
            return denied();
        }

        let importer = unique_registry_device(current_registry, &envelope.recipient_device_id)?;
        if importer.status != DeviceAuthorizationStatus::Active
            || importer.role != DeviceAuthorizationRole::Admin
        {
            return denied();
        }
        validate_local_importer(authorization, importer, false)?;
        let sender = unique_registry_device(current_registry, &envelope.sender_device_id)?;
        validate_manifest_device(current_document, sender)?;
        validate_manifest_device(current_document, importer)?;
        direct_binding.validate_current_devices(sender, importer)?;
        verify_completion_signature(&persisted.completion, current_document, importer)?;
        Ok(persisted.completion.clone())
    }
}

/// Revalidates the exact sender/recipient route without opening root material.
/// Runtime retries use this before re-sending an already-persisted ciphertext.
pub(crate) fn validate_current_transfer_route<'a>(
    entry: &IndexEntry,
    did: &crate::ids::Did,
    did_document: &Value,
    registry: &'a DeviceJoinRemoteRegistry,
    recipient_device_id: &str,
) -> crate::ImResult<(
    &'a DeviceJoinRemoteDeviceSummary,
    &'a DeviceJoinRemoteDeviceSummary,
)> {
    if registry.did != *did {
        return denied();
    }
    let (authorization, local_checkpoint) = require_local_authorization(entry, did)?;
    if authorization.status != DeviceAuthorizationStatus::Active
        || authorization.role != DeviceAuthorizationRole::Admin
        || !authorization.management_ready
        || authorization.protocol_device_id.as_str() == recipient_device_id
    {
        return denied();
    }
    validate_checkpoint_advance(local_checkpoint, &registry.checkpoint)?;
    validate_current_document(did, did_document, registry)?;
    let sender = require_registry_admin(registry, authorization.protocol_device_id.as_str(), true)?;
    let recipient = require_registry_admin(registry, recipient_device_id, false)?;
    validate_manifest_device(did_document, sender)?;
    validate_manifest_device(did_document, recipient)?;
    if sender.signing_key_id != authorization.signing_key_id
        || sender.e2ee_key_id != authorization.e2ee_key_id
    {
        return denied();
    }
    Ok((sender, recipient))
}

pub(crate) struct RootKeyEnvelopePrepareInput<'a> {
    pub(crate) local_alias: &'a str,
    pub(crate) did_document: &'a Value,
    pub(crate) registry: &'a DeviceJoinRemoteRegistry,
    pub(crate) recipient_device_id: &'a str,
    pub(crate) message_id: &'a str,
    pub(crate) user_presence_at: OffsetDateTime,
    pub(crate) now: OffsetDateTime,
    pub(crate) expires_at: OffsetDateTime,
}

pub(crate) struct RootKeyEnvelopeImportInput<'a> {
    pub(crate) local_alias: &'a str,
    pub(crate) direct_binding: &'a RootControlDirectBinding,
    pub(crate) transport_context: &'a RootImportTransportContext,
    /// Freshly resolved current DID Document. When its version is newer than
    /// the Envelope checkpoint, the pinned monotonic checkpoint, unchanged
    /// current root, and current device eligibility replace any dependency on
    /// an unavailable historical full-document snapshot.
    pub(crate) current_did_document: &'a Value,
    pub(crate) current_registry: &'a DeviceJoinRemoteRegistry,
    pub(crate) now: OffsetDateTime,
}

pub(crate) struct RootKeyAckResumeInput<'a> {
    pub(crate) local_alias: &'a str,
    pub(crate) message_id: &'a str,
    pub(crate) current_did_document: &'a Value,
    pub(crate) current_registry: &'a DeviceJoinRemoteRegistry,
}

pub(crate) struct PreparedRootKeyEnvelope {
    plaintext: SecretBytes,
    transport_context: RootImportTransportContext,
}

impl PreparedRootKeyEnvelope {
    pub(crate) fn plaintext(&self) -> &SecretBytes {
        &self.plaintext
    }

    pub(crate) fn transport_context(&self) -> &RootImportTransportContext {
        &self.transport_context
    }
}

impl std::fmt::Debug for PreparedRootKeyEnvelope {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedRootKeyEnvelope")
            .field("plaintext", &"<redacted-zeroizing-p5-inner-json>")
            .field("transport_context", &self.transport_context)
            .finish()
    }
}

pub(crate) struct ImportedRootKeyAck {
    plaintext: SecretBytes,
    completion: RootKeyImportedCompletion,
    transport_context: RootImportTransportContext,
    replayed: bool,
}

impl ImportedRootKeyAck {
    pub(crate) fn plaintext(&self) -> &SecretBytes {
        &self.plaintext
    }

    /// This exact object is copied as AWiki-private transport metadata. The
    /// encrypted inner ACK was built from the same value and signature.
    pub(crate) fn completion(&self) -> &RootKeyImportedCompletion {
        &self.completion
    }

    /// The completion sidecar retains the original Envelope direction. The
    /// standard P5 ACK itself is sent in the reverse device direction.
    pub(crate) fn transport_context(&self) -> &RootImportTransportContext {
        &self.transport_context
    }

    pub(crate) fn replayed(&self) -> bool {
        self.replayed
    }
}

impl std::fmt::Debug for ImportedRootKeyAck {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ImportedRootKeyAck")
            .field("plaintext", &"<redacted-zeroizing-p5-inner-json>")
            .field("completion", &self.completion)
            .field("transport_context", &self.transport_context)
            .field("replayed", &self.replayed)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RootControlDirectBinding {
    message_id: String,
    /// The exact P5 v2 session that produced the decrypted inner plaintext.
    session_id: String,
    /// Receiving orientation: local is the importing recipient and peer is
    /// the sending management device.
    session_binding: anp::direct_e2ee::V2SessionBinding,
}

impl RootControlDirectBinding {
    /// Captures the exact established P5 v2 state that authenticated and
    /// decrypted the root-control plaintext. Sibling modules cannot assemble
    /// this security assertion from unverified strings.
    pub(crate) fn from_decrypted_session(
        message_id: impl Into<String>,
        session: &anp::direct_e2ee::V2DirectSessionState,
    ) -> crate::ImResult<Self> {
        session
            .validate()
            .map_err(|_| crate::ImError::PermissionDenied)?;
        if session.disabled || session.status != anp::direct_e2ee::V2_SESSION_STATUS_ESTABLISHED {
            return denied();
        }
        Ok(Self {
            message_id: message_id.into(),
            session_id: session.session_id.clone(),
            session_binding: session.binding.clone(),
        })
    }

    fn validate_for_envelope(&self, envelope: &RootKeyEnvelope) -> crate::ImResult<()> {
        self.session_binding
            .validate()
            .map_err(|_| crate::ImError::PermissionDenied)?;
        validate_session_id(&self.session_id)?;
        if self.message_id != envelope.message_id
            || self.session_binding.local_did != envelope.did
            || self.session_binding.peer_did != envelope.did
            || self.session_binding.local_device_id != envelope.recipient_device_id
            || self.session_binding.peer_device_id != envelope.sender_device_id
        {
            return denied();
        }
        Ok(())
    }

    fn validate_current_devices(
        &self,
        sender: &DeviceJoinRemoteDeviceSummary,
        recipient: &DeviceJoinRemoteDeviceSummary,
    ) -> crate::ImResult<()> {
        if self.session_binding.local_e2ee_key_id != recipient.e2ee_key_id
            || self.session_binding.peer_e2ee_key_id != sender.e2ee_key_id
        {
            return denied();
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RootImportTransportContext {
    pub(crate) message_id: String,
    pub(crate) delivery_class: String,
    pub(crate) sender_device_id: String,
    pub(crate) recipient_device_id: String,
    pub(crate) expires_at: String,
}

impl RootImportTransportContext {
    fn from_reservation(reservation: &RootKeyImportReservation) -> Self {
        Self {
            message_id: reservation.message_id.clone(),
            delivery_class: ROOT_KEY_CONTROL_DELIVERY_CLASS.to_owned(),
            sender_device_id: reservation.sender_device_id.clone(),
            recipient_device_id: reservation.recipient_device_id.clone(),
            expires_at: reservation.expires_at.clone(),
        }
    }

    pub(crate) fn validate(&self) -> crate::ImResult<()> {
        validate_message_id(&self.message_id)?;
        crate::ids::ProtocolDeviceId::parse(&self.sender_device_id)?;
        crate::ids::ProtocolDeviceId::parse(&self.recipient_device_id)?;
        if self.delivery_class != ROOT_KEY_CONTROL_DELIVERY_CLASS
            || self.expires_at.len() > ROOT_CONTROL_TIMESTAMP_MAX_BYTES
            || self.sender_device_id == self.recipient_device_id
        {
            return denied();
        }
        parse_time("transport_context.expires_at", &self.expires_at)?;
        Ok(())
    }

    fn canonicalized(mut self) -> crate::ImResult<Self> {
        self.expires_at = format_time(parse_time(
            "transport_context.expires_at",
            &self.expires_at,
        )?)?;
        Ok(self)
    }

    fn validate_for_envelope(&self, envelope: &RootKeyEnvelope) -> crate::ImResult<()> {
        let expiry_matches = parse_time("transport_context.expires_at", &self.expires_at)?
            .unix_timestamp_nanos()
            == parse_time("envelope.expires_at", &envelope.expires_at)?.unix_timestamp_nanos();
        if self.message_id != envelope.message_id
            || self.sender_device_id != envelope.sender_device_id
            || self.recipient_device_id != envelope.recipient_device_id
            || !expiry_matches
        {
            return denied();
        }
        Ok(())
    }
}

/// The decrypted Envelope is intentionally neither `Clone` nor `Debug`.
/// Every String, including `root_private_key`, is zeroized on drop.
#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
struct RootKeyEnvelope {
    system_type: String,
    message_id: String,
    did: String,
    root_key_id: String,
    document_version: u64,
    document_hash: String,
    sender_device_id: String,
    recipient_device_id: String,
    expires_at: String,
    root_private_key: String,
}

impl RootKeyEnvelope {
    fn validate_shape(&self) -> crate::ImResult<()> {
        if self.system_type != ROOT_KEY_ENVELOPE_SYSTEM_TYPE
            || self.document_version == 0
            || self.document_version > MAX_DOCUMENT_VERSION
            || self.root_private_key.trim().is_empty()
            || self.root_private_key.len() > ROOT_PRIVATE_PEM_MAX_BYTES
            || self.did.len() > ROOT_CONTROL_ID_MAX_BYTES
            || self.root_key_id.len() > ROOT_CONTROL_ID_MAX_BYTES
            || self.sender_device_id.len() > ROOT_CONTROL_DEVICE_ID_MAX_BYTES
            || self.recipient_device_id.len() > ROOT_CONTROL_DEVICE_ID_MAX_BYTES
            || self.expires_at.len() > ROOT_CONTROL_TIMESTAMP_MAX_BYTES
            || self.sender_device_id == self.recipient_device_id
        {
            return denied();
        }
        validate_message_id(&self.message_id)?;
        let did = crate::ids::Did::parse(&self.did)?;
        crate::ids::ProtocolDeviceId::parse(&self.sender_device_id)?;
        crate::ids::ProtocolDeviceId::parse(&self.recipient_device_id)?;
        validate_key_id(&did, &self.root_key_id)?;
        validate_document_hash(&self.document_hash)?;
        parse_time("envelope.expires_at", &self.expires_at)?;
        Ok(())
    }

    fn reservation(&self) -> RootKeyImportReservation {
        RootKeyImportReservation {
            message_id: self.message_id.clone(),
            did: self.did.clone(),
            root_key_id: self.root_key_id.clone(),
            document_version: self.document_version,
            document_hash: self.document_hash.clone(),
            sender_device_id: self.sender_device_id.clone(),
            recipient_device_id: self.recipient_device_id.clone(),
            expires_at: self.expires_at.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RootKeyImportedCompletion {
    #[serde(rename = "type")]
    pub(crate) completion_type: String,
    pub(crate) ack_for_message_id: String,
    pub(crate) did: String,
    pub(crate) sending_device_id: String,
    pub(crate) importing_device_id: String,
    pub(crate) root_key_id: String,
    pub(crate) root_public_key_fingerprint: String,
    pub(crate) document_version: u64,
    pub(crate) document_hash: String,
    pub(crate) result: String,
    pub(crate) imported_at: String,
    pub(crate) device_signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RootKeyImportReservation {
    message_id: String,
    did: String,
    root_key_id: String,
    document_version: u64,
    document_hash: String,
    sender_device_id: String,
    recipient_device_id: String,
    expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PersistedRootKeyImport {
    pub(crate) schema_version: u32,
    pub(crate) reservation: RootKeyImportReservation,
    pub(crate) completion: RootKeyImportedCompletion,
    /// Stable, local-only idempotency key for the post-import token issue.
    /// It is rotated only after an exact replay returns an expired token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) management_token_operation_id: Option<String>,
}

impl PersistedRootKeyImport {
    pub(crate) fn message_id(&self) -> &str {
        &self.reservation.message_id
    }

    pub(crate) fn root_key_id(&self) -> &str {
        &self.reservation.root_key_id
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RootKeyImportedInner {
    system_type: String,
    completion: RootKeyImportedCompletion,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingRootKeyImport {
    schema_version: u32,
    reservation: RootKeyImportReservation,
}

struct VaultImportContext {
    vault: Arc<dyn SecretVault + Send + Sync>,
    workspace_id: String,
    vault_context_device_id: String,
    identity_id: Option<String>,
    did: String,
    refs: crate::internal::key_provider::vault::VNextVaultKeyMaterialRefs,
}

struct ValidatedRoot {
    public_pem: String,
    fingerprint: String,
}

struct FreshImportValidation<'a> {
    recipient: &'a DeviceJoinRemoteDeviceSummary,
    current_root: ValidatedRoot,
}

fn validate_fresh_import<'a>(
    entry: &IndexEntry,
    did: &crate::ids::Did,
    envelope: &RootKeyEnvelope,
    current_document: &Value,
    current_registry: &'a DeviceJoinRemoteRegistry,
    direct_binding: &RootControlDirectBinding,
) -> crate::ImResult<FreshImportValidation<'a>> {
    let (authorization, local_checkpoint) = require_local_authorization(entry, did)?;
    validate_envelope_checkpoint_against_pin(envelope, local_checkpoint)?;
    validate_checkpoint_advance(local_checkpoint, &current_registry.checkpoint)?;
    validate_current_document(did, current_document, current_registry)?;
    validate_snapshot_advance(envelope, &current_registry.checkpoint)?;
    let current_root = validate_root_document(current_document, did, &envelope.root_key_id)?;
    validate_root_private(&envelope.root_private_key, &current_root)?;

    let sender = require_registry_admin(current_registry, &envelope.sender_device_id, true)?;
    let recipient = require_registry_admin(current_registry, &envelope.recipient_device_id, false)?;
    validate_local_importer(authorization, recipient, true)?;
    validate_manifest_device(current_document, sender)?;
    validate_manifest_device(current_document, recipient)?;
    direct_binding.validate_current_devices(sender, recipient)?;
    Ok(FreshImportValidation {
        recipient,
        current_root,
    })
}

fn validate_local_importer(
    authorization: &crate::internal::identity_device_state::DeviceAuthorizationProjection,
    importer: &DeviceJoinRemoteDeviceSummary,
    require_not_ready: bool,
) -> crate::ImResult<()> {
    if authorization.status != DeviceAuthorizationStatus::Active
        || authorization.role != DeviceAuthorizationRole::Admin
        || (require_not_ready && authorization.management_ready)
        || authorization.protocol_device_id.as_str() != importer.device_id
        || authorization.signing_key_id != importer.signing_key_id
        || authorization.e2ee_key_id != importer.e2ee_key_id
    {
        return denied();
    }
    Ok(())
}

fn imported_ack(
    completion: RootKeyImportedCompletion,
    transport_context: RootImportTransportContext,
    replayed: bool,
) -> crate::ImResult<ImportedRootKeyAck> {
    // The Inbox projection may spell the same PostgreSQL timestamp with an
    // explicit offset/fraction. Persist/retry the ACK sidecar in the same
    // canonical RFC3339 form as the encrypted Envelope reservation so an
    // ambiguous POST retry remains byte-identical.
    let transport_context = transport_context.canonicalized()?;
    let plaintext = encode_zeroizing_json(&RootKeyImportedInner {
        system_type: ROOT_KEY_IMPORTED_SYSTEM_TYPE.to_owned(),
        completion: completion.clone(),
    })?;
    Ok(ImportedRootKeyAck {
        plaintext,
        completion,
        transport_context,
        replayed,
    })
}

fn local_entry<'a>(
    index: &'a crate::internal::identity_store::IndexPayload,
    local_alias: &str,
) -> crate::ImResult<&'a IndexEntry> {
    let local_alias = local_alias.trim();
    if local_alias.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("local_alias".to_owned()),
            "local alias is required",
        ));
    }
    index
        .credentials
        .get(local_alias)
        .ok_or_else(|| crate::ImError::IdentityNotFound {
            selector: local_alias.to_owned(),
        })
}

fn require_local_authorization<'a>(
    entry: &'a IndexEntry,
    did: &crate::ids::Did,
) -> crate::ImResult<(
    &'a crate::internal::identity_device_state::DeviceAuthorizationProjection,
    &'a IdentityInternalCheckpoint,
)> {
    let state = entry
        .device_state
        .as_ref()
        .ok_or(crate::ImError::PermissionDenied)?;
    state.validate_for_did(did)?;
    if state.mode != IdentityDeviceMode::VNext {
        return denied();
    }
    Ok((
        state
            .authorization
            .as_ref()
            .ok_or(crate::ImError::PermissionDenied)?,
        state
            .checkpoint
            .as_ref()
            .ok_or(crate::ImError::PermissionDenied)?,
    ))
}

fn require_vault_context(
    entry: &IndexEntry,
    secret_storage: &SaveIdentitySecretStorage,
) -> crate::ImResult<VaultImportContext> {
    let SaveIdentitySecretStorage::Vault {
        workspace_id,
        device_id,
        vault,
    } = secret_storage
    else {
        return Err(crate::ImError::IdentityVault {
            failure: crate::IdentityVaultFailure::Unavailable,
        });
    };
    let metadata = entry
        .vault_migration
        .as_ref()
        .ok_or(crate::ImError::IdentityVault {
            failure: crate::IdentityVaultFailure::MetadataMissing,
        })?;
    if metadata.status != IdentityVaultMigrationStatus::Verified {
        return Err(crate::ImError::IdentityVault {
            failure: crate::IdentityVaultFailure::MetadataUnverified,
        });
    }
    if metadata.workspace_id != *workspace_id {
        return Err(crate::ImError::IdentityVault {
            failure: crate::IdentityVaultFailure::WorkspaceMismatch,
        });
    }
    if metadata.device_id != *device_id {
        return Err(crate::ImError::IdentityVault {
            failure: crate::IdentityVaultFailure::DeviceMismatch,
        });
    }
    let refs = metadata
        .vnext_key_material_refs()
        .ok_or(crate::ImError::PermissionDenied)?;
    Ok(VaultImportContext {
        vault: vault.clone(),
        workspace_id: workspace_id.clone(),
        vault_context_device_id: device_id.clone(),
        identity_id: Some(entry.unique_id.clone()).filter(|value| !value.trim().is_empty()),
        did: entry.did.clone(),
        refs,
    })
}

fn validate_secret_ref(
    secret_ref: &SecretRef,
    context: &VaultImportContext,
    expected_kind: SecretKind,
    expected_key_id: Option<&str>,
) -> crate::ImResult<()> {
    if secret_ref.workspace_id != context.workspace_id
        || secret_ref.device_id != context.vault_context_device_id
        || secret_ref.identity_id != context.identity_id
        || secret_ref.did.as_deref() != Some(context.did.as_str())
        || secret_ref.kind != expected_kind
        || secret_ref.key_version != 1
        || expected_key_id.is_some_and(|key_id| secret_ref.key_id != key_id)
    {
        return denied();
    }
    Ok(())
}

fn expected_root_ref(context: &VaultImportContext, root_key_id: &str) -> SecretRef {
    SecretMetadata {
        workspace_id: context.workspace_id.clone(),
        device_id: context.vault_context_device_id.clone(),
        identity_id: context.identity_id.clone(),
        did: Some(context.did.clone()),
        kind: SecretKind::IdentityRootPrivate,
        key_id: root_key_id.to_owned(),
        key_version: 1,
        policy: SecretAccessPolicy::no_prompt_local_secret(),
    }
    .secret_ref()
}

fn seal_or_resume_root(
    context: &VaultImportContext,
    expected_ref: &SecretRef,
    root_private: &[u8],
) -> crate::ImResult<()> {
    let result = context.vault.seal_if_absent(SealSecretRequest {
        metadata: SecretMetadata {
            workspace_id: expected_ref.workspace_id.clone(),
            device_id: expected_ref.device_id.clone(),
            identity_id: expected_ref.identity_id.clone(),
            did: expected_ref.did.clone(),
            kind: expected_ref.kind.clone(),
            key_id: expected_ref.key_id.clone(),
            key_version: expected_ref.key_version,
            policy: SecretAccessPolicy::no_prompt_local_secret(),
        },
        plaintext: SecretBytes::from_vec(root_private.to_vec()),
    })?;
    let secret_ref = match result {
        SealIfAbsentResult::Sealed(secret_ref) | SealIfAbsentResult::AlreadyExists(secret_ref) => {
            secret_ref
        }
    };
    if secret_ref != *expected_ref {
        return denied();
    }
    let opened = context.vault.open(expected_ref)?;
    if opened.expose_secret() != root_private {
        return denied();
    }
    Ok(())
}

fn validate_current_document(
    did: &crate::ids::Did,
    document: &Value,
    registry: &DeviceJoinRemoteRegistry,
) -> crate::ImResult<()> {
    if registry.did != *did
        || registry.checkpoint.document_version == 0
        || registry.checkpoint.document_version > MAX_DOCUMENT_VERSION
        || registry.checkpoint.registry_version == 0
        || document.get("id").and_then(Value::as_str) != Some(did.as_str())
        || crate::internal::identity_wire::device_genesis::document_hash(document)?
            != registry.checkpoint.document_hash
        || !anp::authentication::validate_did_document_binding(document, true)
    {
        return denied();
    }
    anp::authentication::validate_device_manifest(document)
        .map_err(|_| crate::ImError::PermissionDenied)?
        .ok_or(crate::ImError::PermissionDenied)?;
    Ok(())
}

fn validate_root_document(
    document: &Value,
    did: &crate::ids::Did,
    root_key_id: &str,
) -> crate::ImResult<ValidatedRoot> {
    validate_key_id(did, root_key_id)?;
    if document.get("id").and_then(Value::as_str) != Some(did.as_str())
        || !anp::authentication::validate_did_document_binding(document, true)
        || document
            .get("proof")
            .and_then(|proof| proof.get("verificationMethod"))
            .and_then(Value::as_str)
            != Some(root_key_id)
        || !relationship_contains(document, "assertionMethod", root_key_id)
    {
        return denied();
    }
    let methods = document
        .get("verificationMethod")
        .and_then(Value::as_array)
        .ok_or(crate::ImError::PermissionDenied)?;
    let matches = methods
        .iter()
        .filter(|method| method.get("id").and_then(Value::as_str) == Some(root_key_id))
        .collect::<Vec<_>>();
    if matches.len() != 1
        || matches[0].get("controller").and_then(Value::as_str) != Some(did.as_str())
    {
        return denied();
    }
    let public = anp::authentication::extract_public_key(matches[0])
        .map_err(|_| crate::ImError::PermissionDenied)?;
    if !matches!(public, anp::PublicKeyMaterial::Ed25519(_)) {
        return denied();
    }
    let fingerprint = format!(
        "e1_{}",
        anp::authentication::compute_multikey_fingerprint(&public)
            .map_err(|_| crate::ImError::PermissionDenied)?
    );
    if did.as_str().rsplit(':').next() != Some(fingerprint.as_str()) {
        return denied();
    }
    Ok(ValidatedRoot {
        public_pem: public.to_pem(),
        fingerprint,
    })
}

fn validate_root_private(root_private_pem: &str, root: &ValidatedRoot) -> crate::ImResult<()> {
    let private = anp::PrivateKeyMaterial::from_pem(root_private_pem)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    if !matches!(private, anp::PrivateKeyMaterial::Ed25519(_))
        || private.public_key().to_pem() != root.public_pem
    {
        return denied();
    }
    Ok(())
}

fn validate_manifest_device(
    document: &Value,
    registry_device: &DeviceJoinRemoteDeviceSummary,
) -> crate::ImResult<()> {
    let device = anp::authentication::find_eligible_device(
        document,
        &registry_device.device_id,
        anp::authentication::PROFILE_DIRECT_E2EE_V2,
    )
    .map_err(|_| crate::ImError::PermissionDenied)?
    .ok_or(crate::ImError::PermissionDenied)?;
    if device.signing_key_id != registry_device.signing_key_id
        || device.e2ee_key_id != registry_device.e2ee_key_id
    {
        return denied();
    }
    Ok(())
}

fn unique_registry_device<'a>(
    registry: &'a DeviceJoinRemoteRegistry,
    device_id: &str,
) -> crate::ImResult<&'a DeviceJoinRemoteDeviceSummary> {
    let matches = registry
        .devices
        .iter()
        .filter(|device| device.device_id == device_id)
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return denied();
    }
    Ok(matches[0])
}

fn require_registry_admin<'a>(
    registry: &'a DeviceJoinRemoteRegistry,
    device_id: &str,
    management_ready: bool,
) -> crate::ImResult<&'a DeviceJoinRemoteDeviceSummary> {
    let device = unique_registry_device(registry, device_id)?;
    if device.status != DeviceAuthorizationStatus::Active
        || device.role != DeviceAuthorizationRole::Admin
        || device.management_ready != management_ready
    {
        return denied();
    }
    Ok(device)
}

fn validate_checkpoint_advance(
    pinned: &IdentityInternalCheckpoint,
    current: &IdentityInternalCheckpoint,
) -> crate::ImResult<()> {
    if pinned.document_version == 0
        || pinned.document_version > MAX_DOCUMENT_VERSION
        || current.document_version == 0
        || current.document_version > MAX_DOCUMENT_VERSION
        || current.document_version < pinned.document_version
        || current.registry_version < pinned.registry_version
        || (current.document_version == pinned.document_version
            && current.document_hash != pinned.document_hash)
    {
        return denied();
    }
    validate_document_hash(&current.document_hash)
}

fn validate_snapshot_advance(
    envelope: &RootKeyEnvelope,
    current: &IdentityInternalCheckpoint,
) -> crate::ImResult<()> {
    if current.document_version < envelope.document_version
        || (current.document_version == envelope.document_version
            && current.document_hash != envelope.document_hash)
    {
        return denied();
    }
    Ok(())
}

fn validate_reservation_snapshot_advance(
    reservation: &RootKeyImportReservation,
    current: &IdentityInternalCheckpoint,
) -> crate::ImResult<()> {
    if current.document_version < reservation.document_version
        || (current.document_version == reservation.document_version
            && current.document_hash != reservation.document_hash)
    {
        return denied();
    }
    Ok(())
}

fn validate_envelope_checkpoint_against_pin(
    envelope: &RootKeyEnvelope,
    pinned: &IdentityInternalCheckpoint,
) -> crate::ImResult<()> {
    if envelope.document_version < pinned.document_version
        || (envelope.document_version == pinned.document_version
            && envelope.document_hash != pinned.document_hash)
    {
        return denied();
    }
    Ok(())
}

fn sign_completion(
    envelope: &RootKeyEnvelope,
    root_fingerprint: &str,
    imported_at: OffsetDateTime,
    importing_device: &DeviceJoinRemoteDeviceSummary,
    current_document: &Value,
    vault_context: &VaultImportContext,
) -> crate::ImResult<RootKeyImportedCompletion> {
    let mut completion = RootKeyImportedCompletion {
        completion_type: ROOT_KEY_IMPORTED_SYSTEM_TYPE.to_owned(),
        ack_for_message_id: envelope.message_id.clone(),
        did: envelope.did.clone(),
        sending_device_id: envelope.sender_device_id.clone(),
        importing_device_id: envelope.recipient_device_id.clone(),
        root_key_id: envelope.root_key_id.clone(),
        root_public_key_fingerprint: root_fingerprint.to_owned(),
        document_version: envelope.document_version,
        document_hash: envelope.document_hash.clone(),
        result: ROOT_KEY_IMPORT_RESULT.to_owned(),
        imported_at: format_time(imported_at)?,
        device_signature: String::new(),
    };
    validate_unsigned_completion(&completion)?;
    let signing_ref = &vault_context.refs.device_request_signing_private;
    validate_secret_ref(
        signing_ref,
        vault_context,
        SecretKind::IdentityDeviceSigningPrivate,
        Some(&importing_device.signing_key_id),
    )?;
    let signing_secret = vault_context.vault.open(signing_ref)?;
    let signing_pem = std::str::from_utf8(signing_secret.expose_secret())
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let private = anp::PrivateKeyMaterial::from_pem(signing_pem)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    if !matches!(private, anp::PrivateKeyMaterial::Ed25519(_)) {
        return denied();
    }
    let method = verification_method(current_document, &importing_device.signing_key_id)?;
    let expected_public = anp::authentication::extract_public_key(method)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    if private.public_key().to_pem() != expected_public.to_pem() {
        return denied();
    }
    let signature = private
        .sign_message(&unsigned_completion_bytes(&completion)?)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    completion.device_signature = URL_SAFE_NO_PAD.encode(signature);
    verify_completion_signature(&completion, current_document, importing_device)?;
    Ok(completion)
}

fn validate_unsigned_completion(completion: &RootKeyImportedCompletion) -> crate::ImResult<()> {
    if completion.completion_type != ROOT_KEY_IMPORTED_SYSTEM_TYPE
        || completion.result != ROOT_KEY_IMPORT_RESULT
        || completion.document_version == 0
        || completion.document_version > MAX_DOCUMENT_VERSION
        || completion.did.len() > ROOT_CONTROL_ID_MAX_BYTES
        || completion.root_key_id.len() > ROOT_CONTROL_ID_MAX_BYTES
        || completion.sending_device_id.len() > ROOT_CONTROL_DEVICE_ID_MAX_BYTES
        || completion.importing_device_id.len() > ROOT_CONTROL_DEVICE_ID_MAX_BYTES
        || completion.imported_at.len() > ROOT_CONTROL_TIMESTAMP_MAX_BYTES
        || completion.sending_device_id == completion.importing_device_id
    {
        return denied();
    }
    validate_message_id(&completion.ack_for_message_id)?;
    let did = crate::ids::Did::parse(&completion.did)?;
    crate::ids::ProtocolDeviceId::parse(&completion.sending_device_id)?;
    crate::ids::ProtocolDeviceId::parse(&completion.importing_device_id)?;
    validate_key_id(&did, &completion.root_key_id)?;
    validate_document_hash(&completion.document_hash)?;
    validate_e1_fingerprint(&completion.root_public_key_fingerprint)?;
    parse_time("completion.imported_at", &completion.imported_at)?;
    Ok(())
}

fn validate_reservation_shape(reservation: &RootKeyImportReservation) -> crate::ImResult<()> {
    if reservation.document_version == 0
        || reservation.document_version > MAX_DOCUMENT_VERSION
        || reservation.did.len() > ROOT_CONTROL_ID_MAX_BYTES
        || reservation.root_key_id.len() > ROOT_CONTROL_ID_MAX_BYTES
        || reservation.sender_device_id.len() > ROOT_CONTROL_DEVICE_ID_MAX_BYTES
        || reservation.recipient_device_id.len() > ROOT_CONTROL_DEVICE_ID_MAX_BYTES
        || reservation.expires_at.len() > ROOT_CONTROL_TIMESTAMP_MAX_BYTES
        || reservation.sender_device_id == reservation.recipient_device_id
    {
        return denied();
    }
    validate_message_id(&reservation.message_id)?;
    let did = crate::ids::Did::parse(&reservation.did)?;
    validate_key_id(&did, &reservation.root_key_id)?;
    crate::ids::ProtocolDeviceId::parse(&reservation.sender_device_id)?;
    crate::ids::ProtocolDeviceId::parse(&reservation.recipient_device_id)?;
    validate_document_hash(&reservation.document_hash)?;
    parse_time("root_import.expires_at", &reservation.expires_at)?;
    Ok(())
}

fn validate_completion_for_envelope(
    completion: &RootKeyImportedCompletion,
    envelope: &RootKeyEnvelope,
    context: &RootImportTransportContext,
) -> crate::ImResult<()> {
    validate_persisted_completion(
        completion,
        envelope,
        &completion.root_public_key_fingerprint,
    )?;
    if parse_time("completion.imported_at", &completion.imported_at)?
        > parse_time("transport_context.expires_at", &context.expires_at)?
    {
        return denied();
    }
    Ok(())
}

fn validate_persisted_completion(
    completion: &RootKeyImportedCompletion,
    envelope: &RootKeyEnvelope,
    expected_fingerprint: &str,
) -> crate::ImResult<()> {
    validate_unsigned_completion(completion)?;
    if completion.ack_for_message_id != envelope.message_id
        || completion.did != envelope.did
        || completion.sending_device_id != envelope.sender_device_id
        || completion.importing_device_id != envelope.recipient_device_id
        || completion.root_key_id != envelope.root_key_id
        || completion.root_public_key_fingerprint != expected_fingerprint
        || completion.document_version != envelope.document_version
        || completion.document_hash != envelope.document_hash
        || completion.device_signature.is_empty()
    {
        return denied();
    }
    validate_canonical_signature(&completion.device_signature)?;
    Ok(())
}

fn validate_completion_for_reservation(
    completion: &RootKeyImportedCompletion,
    reservation: &RootKeyImportReservation,
    expected_fingerprint: &str,
) -> crate::ImResult<()> {
    validate_unsigned_completion(completion)?;
    if completion.ack_for_message_id != reservation.message_id
        || completion.did != reservation.did
        || completion.sending_device_id != reservation.sender_device_id
        || completion.importing_device_id != reservation.recipient_device_id
        || completion.root_key_id != reservation.root_key_id
        || completion.root_public_key_fingerprint != expected_fingerprint
        || completion.document_version != reservation.document_version
        || completion.document_hash != reservation.document_hash
        || completion.device_signature.is_empty()
        || parse_time("completion.imported_at", &completion.imported_at)?
            > parse_time("root_import.expires_at", &reservation.expires_at)?
    {
        return denied();
    }
    validate_canonical_signature(&completion.device_signature)
}

fn verify_completion_signature(
    completion: &RootKeyImportedCompletion,
    current_document: &Value,
    importing_device: &DeviceJoinRemoteDeviceSummary,
) -> crate::ImResult<()> {
    validate_canonical_signature(&completion.device_signature)?;
    if importing_device.device_id != completion.importing_device_id {
        return denied();
    }
    let method = verification_method(current_document, &importing_device.signing_key_id)?;
    let verifier = anp::authentication::create_verification_method(method)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    if !matches!(verifier.public_key, anp::PublicKeyMaterial::Ed25519(_)) {
        return denied();
    }
    verifier
        .verify_signature(
            &unsigned_completion_bytes(completion)?,
            &completion.device_signature,
        )
        .map_err(|_| crate::ImError::PermissionDenied)
}

fn unsigned_completion_bytes(completion: &RootKeyImportedCompletion) -> crate::ImResult<Vec<u8>> {
    let mut value =
        serde_json::to_value(completion).map_err(|error| crate::ImError::Serialization {
            detail: error.to_string(),
        })?;
    value
        .as_object_mut()
        .ok_or_else(|| crate::ImError::Serialization {
            detail: "root import completion must serialize as an object".to_owned(),
        })?
        .remove("device_signature");
    serde_json_canonicalizer::to_vec(&value).map_err(|error| crate::ImError::Serialization {
        detail: error.to_string(),
    })
}

fn verification_method<'a>(document: &'a Value, key_id: &str) -> crate::ImResult<&'a Value> {
    let methods = document
        .get("verificationMethod")
        .and_then(Value::as_array)
        .ok_or(crate::ImError::PermissionDenied)?;
    let matches = methods
        .iter()
        .filter(|method| method.get("id").and_then(Value::as_str) == Some(key_id))
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return denied();
    }
    Ok(matches[0])
}

fn relationship_contains(document: &Value, relationship: &str, key_id: &str) -> bool {
    document
        .get(relationship)
        .and_then(Value::as_array)
        .is_some_and(|values| {
            values.iter().any(|value| {
                value.as_str() == Some(key_id)
                    || value.get("id").and_then(Value::as_str) == Some(key_id)
            })
        })
}

fn validate_message_id(value: &str) -> crate::ImResult<()> {
    if value.is_empty() || value.trim() != value || value.len() > 256 {
        return Err(crate::ImError::invalid_input(
            Some("message_id".to_owned()),
            "root control message_id is invalid",
        ));
    }
    Ok(())
}

fn validate_session_id(value: &str) -> crate::ImResult<()> {
    let decoded = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    if decoded.len() != 16 || URL_SAFE_NO_PAD.encode(decoded) != value {
        return denied();
    }
    Ok(())
}

fn validate_key_id(did: &crate::ids::Did, key_id: &str) -> crate::ImResult<()> {
    let prefix = format!("{}#", did.as_str());
    if key_id.len() > ROOT_CONTROL_ID_MAX_BYTES
        || !key_id.starts_with(&prefix)
        || key_id.len() == prefix.len()
    {
        return denied();
    }
    Ok(())
}

fn validate_document_hash(value: &str) -> crate::ImResult<()> {
    let encoded = value
        .strip_prefix("sha256:")
        .ok_or(crate::ImError::PermissionDenied)?;
    let decoded = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    if decoded.len() != 32 || URL_SAFE_NO_PAD.encode(decoded) != encoded {
        return denied();
    }
    Ok(())
}

fn validate_e1_fingerprint(value: &str) -> crate::ImResult<()> {
    let encoded = value
        .strip_prefix("e1_")
        .ok_or(crate::ImError::PermissionDenied)?;
    let decoded = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    if decoded.len() != 32 || URL_SAFE_NO_PAD.encode(decoded) != encoded {
        return denied();
    }
    Ok(())
}

fn validate_canonical_signature(value: &str) -> crate::ImResult<()> {
    let decoded = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    if decoded.len() != 64 || URL_SAFE_NO_PAD.encode(decoded) != value {
        return denied();
    }
    Ok(())
}

fn validate_short_window(now: OffsetDateTime, expires_at: OffsetDateTime) -> crate::ImResult<()> {
    if expires_at <= now || expires_at - now > Duration::seconds(ROOT_KEY_CONTROL_MAX_TTL_SECONDS) {
        return Err(crate::ImError::SessionExpired);
    }
    Ok(())
}

fn canonical_control_expiry(value: OffsetDateTime) -> crate::ImResult<OffsetDateTime> {
    value
        .replace_nanosecond(0)
        .map_err(|error| crate::ImError::Serialization {
            detail: error.to_string(),
        })
}

fn validate_user_presence(
    confirmed_at: OffsetDateTime,
    now: OffsetDateTime,
) -> crate::ImResult<()> {
    let earliest = now
        .checked_sub(Duration::seconds(USER_PRESENCE_MAX_AGE_SECONDS))
        .ok_or(crate::ImError::PermissionDenied)?;
    let latest = now
        .checked_add(Duration::seconds(USER_PRESENCE_FUTURE_SKEW_SECONDS))
        .ok_or(crate::ImError::PermissionDenied)?;
    if confirmed_at < earliest || confirmed_at > latest {
        return denied();
    }
    Ok(())
}

fn parse_time(field: &str, value: &str) -> crate::ImResult<OffsetDateTime> {
    OffsetDateTime::parse(value, &Rfc3339).map_err(|_| crate::ImError::InvalidInput {
        field: Some(field.to_owned()),
        message: "timestamp must use RFC3339".to_owned(),
    })
}

fn format_time(value: OffsetDateTime) -> crate::ImResult<String> {
    value
        .format(&Rfc3339)
        .map_err(|error| crate::ImError::Serialization {
            detail: error.to_string(),
        })
}

fn encode_zeroizing_json(value: &impl Serialize) -> crate::ImResult<SecretBytes> {
    let mut raw = Zeroizing::new(Vec::new());
    serde_json_canonicalizer::to_writer(value, &mut *raw).map_err(|error| {
        crate::ImError::Serialization {
            detail: error.to_string(),
        }
    })?;
    Ok(SecretBytes::from_vec(std::mem::take(&mut *raw)))
}

fn decode_strict_json<T: for<'de> Deserialize<'de>>(plaintext: &SecretBytes) -> crate::ImResult<T> {
    serde_json::from_slice(plaintext.expose_secret()).map_err(|error| {
        crate::ImError::Serialization {
            detail: format!("invalid AWiki root-control inner JSON: {error}"),
        }
    })
}

fn decode_root_envelope(plaintext: &SecretBytes) -> crate::ImResult<RootKeyEnvelope> {
    if plaintext.expose_secret().len() > ROOT_CONTROL_MAX_INNER_BYTES {
        return Err(crate::ImError::invalid_input(
            Some("root_control_inner".to_owned()),
            "AWiki root-control inner plaintext exceeds the size limit",
        ));
    }
    decode_strict_json(plaintext)
}

fn ensure_pending_reservation(
    identity_dir: &Path,
    reservation: &RootKeyImportReservation,
    now: OffsetDateTime,
) -> crate::ImResult<()> {
    fs::create_dir_all(identity_dir)?;
    set_private_dir_mode(identity_dir)?;
    let path = identity_dir.join(ROOT_IMPORT_PENDING_FILE);
    match fs::read(&path) {
        Ok(raw) => {
            let pending: PendingRootKeyImport =
                serde_json::from_slice(&raw).map_err(|_| crate::ImError::PermissionDenied)?;
            if pending.schema_version != ROOT_KEY_IMPORT_SCHEMA_VERSION {
                return denied();
            }
            if pending.reservation == *reservation {
                return Ok(());
            }
            let pending_expires_at = parse_time(
                "pending_root_import.expires_at",
                &pending.reservation.expires_at,
            )?;
            if now < pending_expires_at {
                return denied();
            }
            let replacement = PendingRootKeyImport {
                schema_version: ROOT_KEY_IMPORT_SCHEMA_VERSION,
                reservation: reservation.clone(),
            };
            write_private_json_atomic(&path, &replacement)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let pending = PendingRootKeyImport {
                schema_version: ROOT_KEY_IMPORT_SCHEMA_VERSION,
                reservation: reservation.clone(),
            };
            write_private_json_atomic(&path, &pending)
        }
        Err(error) => Err(error.into()),
    }
}

fn remove_matching_pending(identity_dir: &Path, reservation: &RootKeyImportReservation) {
    let path = identity_dir.join(ROOT_IMPORT_PENDING_FILE);
    let matches = fs::read(&path)
        .ok()
        .and_then(|raw| serde_json::from_slice::<PendingRootKeyImport>(&raw).ok())
        .is_some_and(|pending| {
            pending.schema_version == ROOT_KEY_IMPORT_SCHEMA_VERSION
                && pending.reservation == *reservation
        });
    if matches {
        let _ = fs::remove_file(path);
    }
}

fn write_private_json_atomic(path: &Path, value: &impl Serialize) -> crate::ImResult<()> {
    let raw = serde_json::to_vec(value).map_err(|error| crate::ImError::Serialization {
        detail: error.to_string(),
    })?;
    let parent = path
        .parent()
        .ok_or_else(|| crate::ImError::PathUnavailable {
            path_kind: "root_import_pending".to_owned(),
            detail: "pending root import path has no parent".to_owned(),
        })?;
    fs::create_dir_all(parent)?;
    set_private_dir_mode(parent)?;
    let temporary = temporary_path(path);
    let result = (|| -> crate::ImResult<()> {
        let mut file = create_private_file(&temporary)?;
        file.write_all(&raw)?;
        file.sync_all()?;
        crate::internal::atomic_file::replace(&temporary, path)?;
        set_private_file_mode(path)?;
        if let Ok(directory) = fs::File::open(parent) {
            let _ = directory.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn temporary_path(path: &Path) -> PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("root-import.json");
    path.with_file_name(format!(".{name}.{}.{}.tmp", std::process::id(), nonce))
}

#[cfg(unix)]
fn create_private_file(path: &Path) -> crate::ImResult<fs::File> {
    use std::os::unix::fs::OpenOptionsExt as _;
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .map_err(crate::ImError::from)
}

#[cfg(not(unix))]
fn create_private_file(path: &Path) -> crate::ImResult<fs::File> {
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(crate::ImError::from)
}

#[cfg(unix)]
fn set_private_dir_mode(path: &Path) -> crate::ImResult<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_dir_mode(_path: &Path) -> crate::ImResult<()> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file_mode(path: &Path) -> crate::ImResult<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_file_mode(_path: &Path) -> crate::ImResult<()> {
    Ok(())
}

fn denied<T>() -> crate::ImResult<T> {
    Err(crate::ImError::PermissionDenied)
}

#[cfg(test)]
mod tests;
