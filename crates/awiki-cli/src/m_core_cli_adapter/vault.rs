use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use im_core::{
    IdentitySecretStorageBackend, IdentitySecretStoragePolicy, IdentitySelector, ImCore,
    ImCoreOpenOptions, ImCoreSecretVaultOptions,
};
use rand::rngs::OsRng;
use rand::RngCore;
use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::cli_output::ExitError;
use crate::m_core_cli_adapter::message_result::CommandResult;

pub const ROOT_KEY_HINT: &str =
    "Set AWIKI_IM_CORE_VAULT_ROOT_KEY_B64 or let awiki-cli create its no-prompt local vault root key file.";
const LOCAL_ROOT_KEY_FILE_NAME: &str = "root-key.b64u";
const LOCAL_ROOT_KEY_SOURCE: &str = "local_private_file";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliVaultOpenPlan {
    pub mode: IdentitySecretStoragePolicy,
    pub vault_enabled: bool,
    pub vault_required: bool,
    pub root_key_available: bool,
    pub root_key_source: String,
    pub vault_dir: String,
    pub workspace_id: String,
    pub device_id: String,
    pub local_root_key_file: String,
}

pub fn build_im_core_open_options(
    resolved: &crate::workspace_config::Resolved,
) -> Result<ImCoreOpenOptions, ExitError> {
    let multi_device_join_enabled = multi_device_join_enabled()?;
    let multi_device_root_transfer_enabled = multi_device_root_transfer_enabled()?;
    let multi_device_device_revoke_enabled = multi_device_device_revoke_enabled()?;
    let multi_device_direct_e2ee_enabled = multi_device_direct_e2ee_enabled()?;
    let multi_device_group_e2ee_enabled = multi_device_group_e2ee_enabled()?;
    let plan = cli_vault_open_plan(resolved)?;
    if !plan.vault_enabled {
        return Ok(ImCoreOpenOptions::file_compat()
            .with_multi_device_join_enabled(multi_device_join_enabled)
            .with_multi_device_root_transfer_enabled(multi_device_root_transfer_enabled)
            .with_multi_device_device_revoke_enabled(multi_device_device_revoke_enabled)
            .with_multi_device_direct_e2ee_enabled(multi_device_direct_e2ee_enabled)
            .with_multi_device_group_e2ee_enabled(multi_device_group_e2ee_enabled));
    }
    if !plan.root_key_available {
        return Err(missing_root_key_error("build im-core"));
    }
    let root_key = load_or_create_cli_vault_root_key(&plan)
        .map_err(|err| super::error::map_im_error(err, "build im-core identity vault"))?;
    Ok(ImCoreOpenOptions::default()
        .with_identity_secret_vault(
            plan.mode,
            ImCoreSecretVaultOptions::new(
                root_key,
                plan.vault_dir,
                plan.workspace_id,
                plan.device_id,
            ),
        )
        .with_multi_device_join_enabled(multi_device_join_enabled)
        .with_multi_device_root_transfer_enabled(multi_device_root_transfer_enabled)
        .with_multi_device_device_revoke_enabled(multi_device_device_revoke_enabled)
        .with_multi_device_direct_e2ee_enabled(multi_device_direct_e2ee_enabled)
        .with_multi_device_group_e2ee_enabled(multi_device_group_e2ee_enabled))
}

pub(crate) fn multi_device_join_enabled() -> Result<bool, ExitError> {
    match std::env::var("AWIKI_MULTI_DEVICE_JOIN_ENABLED") {
        Err(std::env::VarError::NotPresent) => Ok(false),
        Ok(value) if value.trim() == "1" => Ok(true),
        Ok(value) if value.trim().is_empty() || value.trim() == "0" => Ok(false),
        Ok(_) | Err(std::env::VarError::NotUnicode(_)) => Err(ExitError::new(
            "invalid_config",
            2,
            "AWIKI_MULTI_DEVICE_JOIN_ENABLED must be 0 or 1.",
            "Leave it unset for the fail-closed default, or set it to 1 for an explicit rollout.",
        )),
    }
}

pub(crate) fn multi_device_root_transfer_enabled() -> Result<bool, ExitError> {
    match std::env::var("AWIKI_MULTI_DEVICE_ROOT_TRANSFER_ENABLED") {
        Err(std::env::VarError::NotPresent) => Ok(false),
        Ok(value) if value.trim() == "1" => Ok(true),
        Ok(value) if value.trim().is_empty() || value.trim() == "0" => Ok(false),
        Ok(_) | Err(std::env::VarError::NotUnicode(_)) => Err(ExitError::new(
            "invalid_config",
            2,
            "AWIKI_MULTI_DEVICE_ROOT_TRANSFER_ENABLED must be 0 or 1.",
            "Leave it unset for the fail-closed default, or set it to 1 for an explicit rollout.",
        )),
    }
}

pub(crate) fn multi_device_device_revoke_enabled() -> Result<bool, ExitError> {
    match std::env::var("AWIKI_MULTI_DEVICE_DEVICE_REVOKE_ENABLED") {
        Err(std::env::VarError::NotPresent) => Ok(false),
        Ok(value) if value.trim() == "1" => Ok(true),
        Ok(value) if value.trim().is_empty() || value.trim() == "0" => Ok(false),
        Ok(_) | Err(std::env::VarError::NotUnicode(_)) => Err(ExitError::new(
            "invalid_config",
            2,
            "AWIKI_MULTI_DEVICE_DEVICE_REVOKE_ENABLED must be 0 or 1.",
            "Leave it unset for the fail-closed default, or set it to 1 for an explicit rollout.",
        )),
    }
}

pub(crate) fn multi_device_direct_e2ee_enabled() -> Result<bool, ExitError> {
    match std::env::var("AWIKI_MULTI_DEVICE_DIRECT_E2EE_ENABLED") {
        Err(std::env::VarError::NotPresent) => Ok(false),
        Ok(value) if value.trim() == "1" => Ok(true),
        Ok(value) if value.trim() == "0" => Ok(false),
        Ok(_) | Err(std::env::VarError::NotUnicode(_)) => Err(ExitError::new(
            "invalid_config",
            2,
            "AWIKI_MULTI_DEVICE_DIRECT_E2EE_ENABLED must be 0 or 1.",
            "Leave it unset for the fail-closed default, or set it explicitly to 0 or 1.",
        )),
    }
}

pub(crate) fn multi_device_group_e2ee_enabled() -> Result<bool, ExitError> {
    match std::env::var("AWIKI_MULTI_DEVICE_GROUP_E2EE_ENABLED") {
        Err(std::env::VarError::NotPresent) => Ok(false),
        Ok(value) if value.trim() == "1" => Ok(true),
        Ok(value) if value.trim() == "0" => Ok(false),
        Ok(_) | Err(std::env::VarError::NotUnicode(_)) => Err(ExitError::new(
            "invalid_config",
            2,
            "AWIKI_MULTI_DEVICE_GROUP_E2EE_ENABLED must be 0 or 1.",
            "Leave it unset for the fail-closed default, or set it explicitly to 0 or 1.",
        )),
    }
}

pub fn cli_vault_open_plan(
    resolved: &crate::workspace_config::Resolved,
) -> Result<CliVaultOpenPlan, ExitError> {
    let secret_storage =
        crate::workspace_config::resolve_secret_storage(resolved).map_err(|err| {
            ExitError::new(
                "invalid_config",
            2,
            format!("invalid secret_storage config: {err}"),
            "Use vault_required for new workspaces; root keys are read from env or a local private file.",
        )
    })?;
    let mode = match secret_storage.mode.as_str() {
        "file_compat" => IdentitySecretStoragePolicy::FileCompat,
        "vault_preferred" => IdentitySecretStoragePolicy::VaultPreferred,
        "vault_required" => IdentitySecretStoragePolicy::VaultRequired,
        _ => {
            return Err(ExitError::new(
                "invalid_config",
                2,
                "invalid secret_storage.mode.",
                "Use file_compat, vault_preferred, or vault_required.",
            ));
        }
    };
    let local_root_key_file = local_root_key_file(&secret_storage.vault_dir)
        .to_string_lossy()
        .into_owned();
    let root_key_available = secret_storage.root_key_available
        || Path::new(&local_root_key_file).is_file()
        || !matches!(mode, IdentitySecretStoragePolicy::FileCompat);
    let root_key_source = if secret_storage.root_key_available {
        secret_storage.root_key_source
    } else if Path::new(&local_root_key_file).is_file() {
        LOCAL_ROOT_KEY_SOURCE.to_string()
    } else if !matches!(mode, IdentitySecretStoragePolicy::FileCompat) {
        "local_private_file_pending_create".to_string()
    } else {
        "unset".to_string()
    };
    Ok(CliVaultOpenPlan {
        mode,
        vault_enabled: !matches!(mode, IdentitySecretStoragePolicy::FileCompat),
        vault_required: matches!(mode, IdentitySecretStoragePolicy::VaultRequired),
        root_key_available,
        root_key_source,
        vault_dir: secret_storage.vault_dir,
        workspace_id: secret_storage.workspace_id,
        device_id: secret_storage.device_id,
        local_root_key_file,
    })
}

pub fn identity_vault_status_via_im_core(
    resolved: &crate::workspace_config::Resolved,
    selector: IdentitySelector,
) -> Result<CommandResult, ExitError> {
    let plan = cli_vault_open_plan(resolved)?;
    let core = build_im_core_for_vault_status(resolved, &plan)?;
    identity_vault_status_result(&core, selector, &plan)
}

pub async fn identity_vault_status_via_im_core_async(
    resolved: &crate::workspace_config::Resolved,
    selector: IdentitySelector,
) -> Result<CommandResult, ExitError> {
    let plan = cli_vault_open_plan(resolved)?;
    let core = build_im_core_for_vault_status_async(resolved, &plan).await?;
    identity_vault_status_result_async(&core, selector, &plan).await
}

pub fn identity_vault_migrate_via_im_core(
    resolved: &crate::workspace_config::Resolved,
    selector: IdentitySelector,
    dry_run: bool,
) -> Result<CommandResult, ExitError> {
    let plan = cli_vault_open_plan(resolved)?;
    require_vault_root_key_for_mutation(&plan, "id vault migrate")?;
    if dry_run {
        return Ok(vault_mutation_plan_result(
            "migrate_identity_secrets_to_vault",
            &plan,
        ));
    }
    let core = super::build_im_core(resolved)?;
    let report = core
        .identities()
        .migrate_identity_vault(selector)
        .map_err(|err| super::map_im_error(err, "id vault migrate"))?;
    Ok(vault_migration_result(report, &plan))
}

pub async fn identity_vault_migrate_via_im_core_async(
    resolved: &crate::workspace_config::Resolved,
    selector: IdentitySelector,
    dry_run: bool,
) -> Result<CommandResult, ExitError> {
    let plan = cli_vault_open_plan(resolved)?;
    require_vault_root_key_for_mutation(&plan, "id vault migrate")?;
    if dry_run {
        return Ok(vault_mutation_plan_result(
            "migrate_identity_secrets_to_vault",
            &plan,
        ));
    }
    let core = super::build_im_core_async(resolved).await?;
    let report = core
        .identities()
        .migrate_identity_vault_async(selector)
        .await
        .map_err(|err| super::map_im_error(err, "id vault migrate"))?;
    Ok(vault_migration_result(report, &plan))
}

pub fn identity_vault_cleanup_plaintext_via_im_core(
    resolved: &crate::workspace_config::Resolved,
    selector: IdentitySelector,
    dry_run: bool,
) -> Result<CommandResult, ExitError> {
    let plan = cli_vault_open_plan(resolved)?;
    require_vault_root_key_for_mutation(&plan, "id vault cleanup-plaintext")?;
    if dry_run {
        return Ok(vault_mutation_plan_result(
            "cleanup_identity_plaintext_compat_files",
            &plan,
        ));
    }
    let core = super::build_im_core(resolved)?;
    let status = core
        .identities()
        .vault_status(selector)
        .map_err(|err| super::map_im_error(err, "id vault cleanup-plaintext"))?;
    if status.selected_backend != IdentitySecretStorageBackend::Vault {
        return Err(ExitError::new(
            "vault_not_ready",
            3,
            "id vault cleanup-plaintext requires verified vault-backed identity metadata.",
            "Run `awiki-cli id vault status` and migrate the identity once the im-core cleanup API is available.",
        ));
    }
    unsupported_vault_mutation_result("cleanup-plaintext", status_json(status))
}

pub async fn identity_vault_cleanup_plaintext_via_im_core_async(
    resolved: &crate::workspace_config::Resolved,
    selector: IdentitySelector,
    dry_run: bool,
) -> Result<CommandResult, ExitError> {
    let plan = cli_vault_open_plan(resolved)?;
    require_vault_root_key_for_mutation(&plan, "id vault cleanup-plaintext")?;
    if dry_run {
        return Ok(vault_mutation_plan_result(
            "cleanup_identity_plaintext_compat_files",
            &plan,
        ));
    }
    let core = super::build_im_core_async(resolved).await?;
    let status = core
        .identities()
        .vault_status_async(selector)
        .await
        .map_err(|err| super::map_im_error(err, "id vault cleanup-plaintext"))?;
    if status.selected_backend != IdentitySecretStorageBackend::Vault {
        return Err(ExitError::new(
            "vault_not_ready",
            3,
            "id vault cleanup-plaintext requires verified vault-backed identity metadata.",
            "Run `awiki-cli id vault status` and migrate the identity once the im-core cleanup API is available.",
        ));
    }
    unsupported_vault_mutation_result("cleanup-plaintext", status_json(status))
}

pub fn vault_diagnostics_snapshot(resolved: &crate::workspace_config::Resolved) -> Value {
    match cli_vault_open_plan(resolved) {
        Ok(plan) => json!({
            "mode": mode_label(plan.mode),
            "vault_enabled": plan.vault_enabled,
            "vault_required": plan.vault_required,
            "vault_dir": plan.vault_dir,
            "workspace_id": plan.workspace_id,
            "device_id": plan.device_id,
            "local_root_key_file": plan.local_root_key_file,
            "root_key": root_key_status(&plan),
        }),
        Err(err) => json!({
            "mode": "invalid",
            "error": err.detail.message,
            "root_key": {
                "available": false,
                "source": "unset",
                "env": im_core::vault::IM_CORE_VAULT_ROOT_KEY_ENV,
                "local_private_file": null,
            }
        }),
    }
}

fn identity_vault_status_result(
    core: &ImCore,
    selector: IdentitySelector,
    plan: &CliVaultOpenPlan,
) -> Result<CommandResult, ExitError> {
    let status = core
        .identities()
        .vault_status(selector)
        .map_err(|err| super::map_im_error(err, "id vault status"))?;
    Ok(status_command_result(status_json(status), plan))
}

fn build_im_core_for_vault_status(
    resolved: &crate::workspace_config::Resolved,
    plan: &CliVaultOpenPlan,
) -> Result<ImCore, ExitError> {
    let config = super::core_config::build_im_core_config(resolved)?;
    let paths = super::paths::build_im_core_paths(resolved)?;
    if plan.vault_enabled && root_key_material_available(plan) {
        let options = build_im_core_open_options(resolved)?;
        return ImCore::new_with_options(config, paths, options)
            .map_err(|err| super::error::map_im_error(err, "id vault status"));
    }
    ImCore::new(config, paths).map_err(|err| super::error::map_im_error(err, "id vault status"))
}

async fn build_im_core_for_vault_status_async(
    resolved: &crate::workspace_config::Resolved,
    plan: &CliVaultOpenPlan,
) -> Result<ImCore, ExitError> {
    let config = super::core_config::build_im_core_config(resolved)?;
    let paths = super::paths::build_im_core_paths(resolved)?;
    if plan.vault_enabled && root_key_material_available(plan) {
        let options = build_im_core_open_options(resolved)?;
        return ImCore::open_with_options(config, paths, options)
            .await
            .map_err(|err| super::error::map_im_error(err, "id vault status"));
    }
    ImCore::open(config, paths)
        .await
        .map_err(|err| super::error::map_im_error(err, "id vault status"))
}

async fn identity_vault_status_result_async(
    core: &ImCore,
    selector: IdentitySelector,
    plan: &CliVaultOpenPlan,
) -> Result<CommandResult, ExitError> {
    let status = core
        .identities()
        .vault_status_async(selector)
        .await
        .map_err(|err| super::map_im_error(err, "id vault status"))?;
    Ok(status_command_result(status_json(status), plan))
}

fn status_command_result(status: Value, plan: &CliVaultOpenPlan) -> CommandResult {
    let selected_backend = status["selected_backend"].as_str().unwrap_or("unknown");
    let mut warnings = Vec::new();
    if !plan.root_key_available && plan.vault_enabled {
        warnings.push(format!(
            "identity vault root key is unavailable; vault-backed identity open will fail."
        ));
    }
    for warning in status["warnings"].as_array().into_iter().flatten() {
        if let Some(warning) = warning.as_str() {
            warnings.push(warning.to_string());
        }
    }
    CommandResult {
        data: json!({
            "vault": {
                "open_options": open_options_json(plan),
                "status_context": {
                    "checked_without_vault_context": plan.vault_enabled && !root_key_material_available(plan),
                },
                "identity": status,
            }
        }),
        summary: format!("Identity vault status: {selected_backend}"),
        warnings,
    }
}

fn status_json(status: im_core::IdentityVaultStatus) -> Value {
    json!({
        "identity": super::identity::cli_identity_summary_from_sdk(&status.identity, &[]),
        "storage_policy": mode_label(status.storage_policy),
        "selected_backend": match status.selected_backend {
            IdentitySecretStorageBackend::FileCompat => "file_compat",
            IdentitySecretStorageBackend::Vault => "vault",
        },
        "vault_available": status.vault_available,
        "vault_metadata_present": status.vault_metadata_present,
        "vault_metadata_verified": status.vault_metadata_verified,
        "workspace_id": status.workspace_id,
        "device_id": status.device_id,
        "plaintext_compat_retained": status.plaintext_compat_retained,
        "missing": status.missing,
        "warnings": status.warnings,
    })
}

fn vault_migration_result(
    report: im_core::IdentityVaultMigrationReport,
    plan: &CliVaultOpenPlan,
) -> CommandResult {
    let mut warnings = report.warnings;
    if report.plaintext_compat_retained {
        warnings.push(
            "identity plaintext compatibility files are still retained after migration".to_string(),
        );
    }
    CommandResult {
        data: json!({
            "vault": {
                "open_options": open_options_json(plan),
                "migration": {
                    "identity": super::identity::cli_identity_summary_from_sdk(&report.identity, &[]),
                    "migrated": report.migrated,
                    "verified": report.verified,
                    "plaintext_compat_retained": report.plaintext_compat_retained,
                    "status": status_json(report.status),
                }
            }
        }),
        summary: "Identity vault migration completed".to_string(),
        warnings,
    }
}

fn open_options_json(plan: &CliVaultOpenPlan) -> Value {
    json!({
        "mode": mode_label(plan.mode),
        "vault_enabled": plan.vault_enabled,
        "vault_required": plan.vault_required,
        "vault_dir": plan.vault_dir,
        "workspace_id": plan.workspace_id,
        "device_id": plan.device_id,
        "local_root_key_file": plan.local_root_key_file,
        "root_key": root_key_status(plan),
    })
}

fn root_key_status(plan: &CliVaultOpenPlan) -> Value {
    json!({
        "available": plan.root_key_available,
        "source": plan.root_key_source,
        "env": im_core::vault::IM_CORE_VAULT_ROOT_KEY_ENV,
        "local_private_file": plan.local_root_key_file,
    })
}

fn root_key_material_available(plan: &CliVaultOpenPlan) -> bool {
    plan.vault_enabled
        && plan.root_key_available
        && plan.root_key_source != "local_private_file_pending_create"
}

fn require_vault_root_key_for_mutation(
    plan: &CliVaultOpenPlan,
    context: &'static str,
) -> Result<(), ExitError> {
    if !plan.vault_enabled {
        return Err(ExitError::new(
            "vault_not_enabled",
            3,
            format!("{context}: identity secret storage is file_compat."),
            "Set secret_storage.mode to vault_preferred or vault_required before running vault mutations.",
        ));
    }
    if !plan.root_key_available {
        return Err(missing_root_key_error(context));
    }
    Ok(())
}

fn missing_root_key_error(context: &'static str) -> ExitError {
    ExitError::new(
        "vault_root_key_required",
        3,
        format!("{context}: identity secret vault root key is required."),
        ROOT_KEY_HINT,
    )
}

fn vault_mutation_plan_result(action: &str, plan: &CliVaultOpenPlan) -> CommandResult {
    CommandResult {
        data: json!({
            "plan": {
                "action": action,
                "open_options": open_options_json(plan),
                "root_key_material": "[redacted]",
            }
        }),
        summary: "Dry run: identity vault mutation planned".to_string(),
        warnings: Vec::new(),
    }
}

fn unsupported_vault_mutation_result(
    action: &str,
    status: Value,
) -> Result<CommandResult, ExitError> {
    Err(ExitError::new(
        "unsupported_capability",
        2,
        format!(
            "id vault {action}: im-core does not expose a CLI-safe mutation API in this build."
        ),
        format!(
            "Current status is {}; use `awiki-cli id register` with secret_storage.mode=vault_required for new vault-backed identities.",
            status["selected_backend"].as_str().unwrap_or("unknown")
        ),
    ))
}

fn mode_label(mode: IdentitySecretStoragePolicy) -> &'static str {
    match mode {
        IdentitySecretStoragePolicy::FileCompat => "file_compat",
        IdentitySecretStoragePolicy::VaultPreferred => "vault_preferred",
        IdentitySecretStoragePolicy::VaultRequired => "vault_required",
    }
}

fn load_or_create_cli_vault_root_key(
    plan: &CliVaultOpenPlan,
) -> im_core::ImResult<im_core::vault::DeviceVaultRootKey> {
    match std::env::var(im_core::vault::IM_CORE_VAULT_ROOT_KEY_ENV) {
        Ok(raw) if !raw.trim().is_empty() => {
            return im_core::vault::parse_device_vault_root_key_b64(
                &raw,
                im_core::vault::IM_CORE_VAULT_ROOT_KEY_ENV,
            );
        }
        _ => {}
    }
    let path = Path::new(&plan.local_root_key_file);
    match fs::read_to_string(path) {
        Ok(raw) => {
            return im_core::vault::parse_device_vault_root_key_b64(&raw, LOCAL_ROOT_KEY_SOURCE);
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(im_core::ImError::CredentialFileUnreadable {
                path_kind: "cli_identity_vault_root_key".to_string(),
                detail: err.to_string(),
            });
        }
    }
    let mut bytes = [0_u8; im_core::vault::DEVICE_VAULT_ROOT_KEY_LEN];
    OsRng.fill_bytes(&mut bytes);
    if !write_cli_vault_root_key_file(path, &bytes)? {
        let raw =
            fs::read_to_string(path).map_err(|err| im_core::ImError::CredentialFileUnreadable {
                path_kind: "cli_identity_vault_root_key".to_string(),
                detail: err.to_string(),
            })?;
        return im_core::vault::parse_device_vault_root_key_b64(&raw, LOCAL_ROOT_KEY_SOURCE);
    }
    Ok(im_core::vault::DeviceVaultRootKey::from_bytes(bytes))
}

fn write_cli_vault_root_key_file(
    path: &Path,
    root_key: &[u8; im_core::vault::DEVICE_VAULT_ROOT_KEY_LEN],
) -> im_core::ImResult<bool> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(im_core::ImError::from)?;
        set_private_dir_mode(parent)?;
    }
    let encoded = URL_SAFE_NO_PAD.encode(root_key);
    let mut file = match create_private_file(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => return Ok(false),
        Err(err) => return Err(im_core::ImError::from(err)),
    };
    file.write_all(encoded.as_bytes())
        .map_err(im_core::ImError::from)?;
    file.write_all(b"\n").map_err(im_core::ImError::from)?;
    file.sync_all().map_err(im_core::ImError::from)?;
    set_private_file_mode(path)?;
    Ok(true)
}

fn local_root_key_file(vault_dir: &str) -> PathBuf {
    Path::new(vault_dir).join(LOCAL_ROOT_KEY_FILE_NAME)
}

#[cfg(unix)]
fn create_private_file(path: &Path) -> std::io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn create_private_file(path: &Path) -> std::io::Result<fs::File> {
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
}

#[cfg(unix)]
fn set_private_dir_mode(path: &Path) -> im_core::ImResult<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(im_core::ImError::from)
}

#[cfg(not(unix))]
fn set_private_dir_mode(_path: &Path) -> im_core::ImResult<()> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file_mode(path: &Path) -> im_core::ImResult<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(im_core::ImError::from)
}

#[cfg(not(unix))]
fn set_private_file_mode(_path: &Path) -> im_core::ImResult<()> {
    Ok(())
}
