use im_core::{
    IdentitySecretStorageBackend, IdentitySecretStoragePolicy, IdentitySelector, ImCore,
    ImCoreOpenOptions, ImCoreSecretVaultOptions,
};
use serde_json::{json, Value};

use crate::cli_output::ExitError;
use crate::m_core_cli_adapter::message_result::CommandResult;

pub const ROOT_KEY_HINT: &str =
    "Set AWIKI_IM_CORE_VAULT_ROOT_KEY_B64 to a base64/base64url encoded 32-byte key.";

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
}

pub fn build_im_core_open_options(
    resolved: &crate::workspace_config::Resolved,
) -> Result<ImCoreOpenOptions, ExitError> {
    let plan = cli_vault_open_plan(resolved)?;
    if !plan.vault_enabled {
        return Ok(ImCoreOpenOptions::file_compat());
    }
    if !plan.root_key_available {
        return Err(missing_root_key_error("build im-core"));
    }
    let root_key = im_core::vault::im_core_vault_root_key_from_env()
        .map_err(|err| super::error::map_im_error(err, "build im-core identity vault"))?;
    Ok(ImCoreOpenOptions::default().with_identity_secret_vault(
        plan.mode,
        ImCoreSecretVaultOptions::new(root_key, plan.vault_dir, plan.workspace_id, plan.device_id),
    ))
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
                "Use file_compat, vault_preferred, or vault_required without storing root keys.",
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
    Ok(CliVaultOpenPlan {
        mode,
        vault_enabled: !matches!(mode, IdentitySecretStoragePolicy::FileCompat),
        vault_required: matches!(mode, IdentitySecretStoragePolicy::VaultRequired),
        root_key_available: secret_storage.root_key_available,
        root_key_source: secret_storage.root_key_source,
        vault_dir: secret_storage.vault_dir,
        workspace_id: secret_storage.workspace_id,
        device_id: secret_storage.device_id,
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
            "root_key": root_key_status(&plan),
        }),
        Err(err) => json!({
            "mode": "invalid",
            "error": err.detail.message,
            "root_key": {
                "available": false,
                "source": "unset",
                "env": im_core::vault::IM_CORE_VAULT_ROOT_KEY_ENV,
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
    if plan.vault_enabled && plan.root_key_available {
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
    if plan.vault_enabled && plan.root_key_available {
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
            "{} is not set; vault-backed identity open will fail.",
            im_core::vault::IM_CORE_VAULT_ROOT_KEY_ENV
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
                    "checked_without_vault_context": plan.vault_enabled && !plan.root_key_available,
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
        "root_key": root_key_status(plan),
    })
}

fn root_key_status(plan: &CliVaultOpenPlan) -> Value {
    json!({
        "available": plan.root_key_available,
        "source": plan.root_key_source,
        "env": im_core::vault::IM_CORE_VAULT_ROOT_KEY_ENV,
    })
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
        format!(
            "{context}: {} is required for identity secret vault.",
            im_core::vault::IM_CORE_VAULT_ROOT_KEY_ENV
        ),
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
            "Current status is {}; use `awiki-cli id register` or `awiki-cli id recover` with secret_storage.mode=vault_required for new vault-backed identities.",
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
