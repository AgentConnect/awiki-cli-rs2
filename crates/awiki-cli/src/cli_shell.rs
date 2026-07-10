use crate::build_info::BuildInfo;
use crate::cli_docs;
use crate::cli_output::{
    self, ErrorEnvelope, ExitError, Format, IdentityMeta, Meta, SuccessEnvelope,
};
use crate::cli_parser::{self, ParsedCommand};
use crate::cli_trace;
use crate::command_catalog;
use crate::diagnostics;
use crate::m_core_cli_adapter::message_result::CommandResult;
use crate::workspace_config::{self, Overrides, Resolved, WorkspaceConfigErrorKind};
use crate::workspace_upgrade;
use serde_json::{json, Value};
use std::fs;
use std::io::{self, Write};
use std::path::Path;

mod debug_handlers;
mod error_hints;
mod group_e2ee_handlers;
mod group_handlers;
mod handle_helpers;
mod id_recover_handlers;
mod id_replace_did_handlers;
mod legacy_identity {
    pub(super) use crate::workspace_upgrade::legacy_identity::{
        choose_default_identity_name, create_migration_identity,
        ensure_all_identity_private_keys_compatible, import_v1_migration, CommandResult,
        IdentityError, Manager,
    };
}
mod legacy_sqlite {
    pub(super) use crate::workspace_upgrade::legacy_sqlite::{
        ensure_schema, import_legacy_database, open, scan_legacy_database, LegacyOwnerLookup,
        StoreError,
    };
}
mod mail_handlers;
mod msg_handlers;
mod page_handlers;
mod people_handlers;
mod runtime_handlers;
mod runtime_hermes_handlers;
mod runtime_host_notify_refresh;
mod site_handlers;
pub mod unsupported;
mod update_handlers;
pub(super) mod update_preflight;

use legacy_identity::{IdentityError, Manager};
use legacy_sqlite as store;
use legacy_sqlite::StoreError;

#[derive(Debug, Clone)]
pub struct GlobalOptions {
    pub format: String,
    pub format_changed: bool,
    pub jq: String,
    pub dry_run: bool,
    pub diagnostic: bool,
    pub migration: bool,
    pub identity: String,
    pub identity_changed: bool,
    pub tenant: String,
    pub tenant_changed: bool,
    pub verbose: bool,
}

impl Default for GlobalOptions {
    fn default() -> Self {
        Self {
            format: "json".to_string(),
            format_changed: false,
            jq: String::new(),
            dry_run: false,
            diagnostic: false,
            migration: false,
            identity: String::new(),
            identity_changed: false,
            tenant: String::new(),
            tenant_changed: false,
            verbose: false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct App {
    pub globals: GlobalOptions,
    update_warning: String,
}

pub fn execute() -> i32 {
    match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime.block_on(execute_async()),
        Err(err) => App::default().handle_error(ExitError::new(
            "internal_error",
            1,
            format!("failed to start async runtime: {err}"),
            "Retry the command or run with --diagnostic for more context.",
        )),
    }
}

pub async fn execute_async() -> i32 {
    let command = match cli_parser::parse_env() {
        Ok(command) => command,
        Err(err) => return App::default().handle_error(err),
    };
    let trace_run = cli_trace::Run::new(&command.trace_command());
    cli_trace::set_current(Some(trace_run));
    let mut app = App {
        globals: command.globals.clone(),
        update_warning: String::new(),
    };
    let exit_code = if let Err(err) = app.preflight(&command) {
        app.handle_error(err)
    } else {
        match cli_parser::dispatch_async(&app, &command).await {
            Ok(()) => 0,
            Err(err) => app.handle_error(err),
        }
    };
    if !command.emits_raw_output() {
        app.emit_trace();
    }
    cli_trace::set_current(None);
    exit_code
}

impl Drop for App {
    fn drop(&mut self) {
        cli_trace::set_current(None);
    }
}

impl App {
    fn emit_trace(&self) {
        let _ = cli_trace::emit_current(&mut io::stderr());
    }

    pub(super) fn resolve_config_raw(&self) -> anyhow::Result<Resolved> {
        let mut phase = cli_trace::start_phase("resolve_config");
        let result = self.resolve_config_untraced();
        phase.finish();
        result
    }

    fn resolve_config_untraced(&self) -> anyhow::Result<Resolved> {
        let result = workspace_config::resolve(Overrides {
            identity: self.globals.identity.clone(),
            identity_changed: self.globals.identity_changed,
            tenant: self.globals.tenant.clone(),
            tenant_changed: self.globals.tenant_changed,
            format: self.globals.format.clone(),
            format_changed: self.globals.format_changed,
        });
        result
    }

    pub fn run_status(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let data = json!({
            "cli": {
                "phase": "phase1-shell",
                "version": BuildInfo::current(),
            },
            "paths": resolved.paths,
            "state": identity_status(&resolved),
            "config": {
                "config_exists": resolved.config_exists,
                "config_error": resolved.config_error,
                "env_hits": resolved.env_hits,
                "sources": resolved.sources,
            },
        });
        self.render_success(
            "awiki-cli status",
            &resolved,
            data,
            "Identity status loaded",
            Vec::new(),
        )
    }

    pub fn run_version(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        self.render_success(
            "awiki-cli version",
            &resolved,
            serde_json::to_value(BuildInfo::current()).unwrap_or_else(|_| json!({})),
            "Build information",
            Vec::new(),
        )
    }

    pub fn run_config_show(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let mut data = workspace_config::snapshot(&resolved);
        let object = data.as_object_mut().expect("snapshot is object");
        object.insert(
            "identity_store".to_string(),
            identity_store_snapshot(&resolved),
        );
        object.insert("database".to_string(), database_snapshot(&resolved));
        object.insert(
            "workspace_upgrade".to_string(),
            workspace_upgrade_snapshot(&resolved),
        );
        self.render_success(
            "awiki-cli config show",
            &resolved,
            data,
            "Resolved configuration",
            Vec::new(),
        )
    }

    pub fn run_tenant_list(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let current =
            workspace_config::tenant_context_for_resolved(&resolved).map_err(internal_anyhow)?;
        let tenants = workspace_config::list_tenants().map_err(internal_anyhow)?;
        self.render_success(
            "awiki-cli tenant list",
            &resolved,
            json!({
                "active": current.active,
                "tenants": tenants,
            }),
            "Tenants loaded",
            Vec::new(),
        )
    }

    pub fn run_tenant_current(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let current =
            workspace_config::tenant_context_for_resolved(&resolved).map_err(internal_anyhow)?;
        self.render_success(
            "awiki-cli tenant current",
            &resolved,
            json!({ "tenant": current }),
            "Current tenant loaded",
            Vec::new(),
        )
    }

    pub fn run_tenant_create(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let name = required_arg(command, "tenant create", "name")?;
        let input = workspace_config::TenantCreateInput {
            name: name.to_string(),
            display_name: command.flags.get("display-name").cloned(),
            backend_base_url: required_flag(command, "tenant create", "backend-base-url")?
                .to_string(),
            did_host: required_flag(command, "tenant create", "did-host")?.to_string(),
        };
        let before = self.resolve_config()?;
        if self.globals.dry_run {
            let planned =
                workspace_config::preview_create_tenant(input).map_err(internal_anyhow)?;
            return self.render_success(
                "awiki-cli tenant create",
                &before,
                json!({
                    "plan": {
                        "action": "tenant_create",
                        "tenant": planned,
                    }
                }),
                "Dry run: tenant creation planned",
                Vec::new(),
            );
        }
        let tenant = workspace_config::create_tenant(input).map_err(internal_anyhow)?;
        self.render_success(
            "awiki-cli tenant create",
            &before,
            json!({ "tenant": tenant }),
            "Tenant created",
            Vec::new(),
        )
    }

    pub fn run_tenant_use(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let name = required_arg(command, "tenant use", "name")?;
        let before = self.resolve_config()?;
        if self.globals.dry_run {
            let planned = workspace_config::preview_use_tenant(name).map_err(internal_anyhow)?;
            return self.render_success(
                "awiki-cli tenant use",
                &before,
                json!({
                    "plan": {
                        "action": "tenant_use",
                        "tenant": planned,
                    }
                }),
                "Dry run: tenant switch planned",
                Vec::new(),
            );
        }
        let tenant = workspace_config::use_tenant(name).map_err(internal_anyhow)?;
        let resolved = self.resolve_config()?;
        self.render_success(
            "awiki-cli tenant use",
            &resolved,
            json!({ "tenant": tenant }),
            "Tenant switched",
            Vec::new(),
        )
    }

    pub fn run_tenant_reconfigure(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let name = required_arg(command, "tenant reconfigure", "name")?;
        let backend_base_url = required_flag(command, "tenant reconfigure", "backend-base-url")?;
        let did_host = required_flag(command, "tenant reconfigure", "did-host")?;
        let before = self.resolve_config()?;
        if self.globals.dry_run {
            let planned =
                workspace_config::preview_reconfigure_tenant(name, backend_base_url, did_host)
                    .map_err(internal_anyhow)?;
            return self.render_success(
                "awiki-cli tenant reconfigure",
                &before,
                json!({
                    "plan": {
                        "action": "tenant_reconfigure",
                        "tenant": planned,
                    }
                }),
                "Dry run: tenant reconfiguration planned",
                Vec::new(),
            );
        }
        let tenant = workspace_config::reconfigure_tenant(name, backend_base_url, did_host)
            .map_err(internal_anyhow)?;
        let resolved = self.resolve_config()?;
        self.render_success(
            "awiki-cli tenant reconfigure",
            &resolved,
            json!({ "tenant": tenant }),
            "Tenant reconfigured",
            Vec::new(),
        )
    }

    pub fn run_doctor(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let report = diagnostics::run(&resolved);
        let summary = report.summary.clone();
        self.render_success(
            "awiki-cli doctor",
            &resolved,
            serde_json::to_value(report).unwrap_or_else(|_| json!({})),
            &summary,
            Vec::new(),
        )
    }

    pub fn run_docs(&self, args: &[String]) -> Result<(), ExitError> {
        if args.is_empty() {
            return self.render_static_success(
                "awiki-cli docs",
                json!({ "topics": cli_docs::all() }),
                "Available documentation topics",
                Vec::new(),
            );
        }
        if args.len() > 1 {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "docs accepts at most one topic.",
                "Run `awiki-cli docs` without arguments to list topics.",
            ));
        }
        let raw = &args[0];
        let Some(topic) = cli_docs::lookup(raw) else {
            return Err(ExitError::new(
                "not_found",
                5,
                format!("Unknown docs topic {raw:?}"),
                "Run `awiki-cli docs` to list available topics.",
            ));
        };
        self.render_static_success(
            "awiki-cli docs",
            json!({ "topic": topic }),
            &format!("Documentation topic {}", topic.name),
            Vec::new(),
        )
    }

    pub fn run_schema(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        if command.flags.contains_key("all") && command.flags.contains_key("audience") {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "schema accepts either --all or --audience, not both.",
                "Use `awiki-cli schema --all` or `awiki-cli schema --audience diagnostic`.",
            ));
        }
        if command.flags.contains_key("all") {
            return self.render_static_success(
                "awiki-cli schema",
                json!({ "commands": command_catalog::specs(), "phase": "phase1-shell", "audience": "all" }),
                "Static command contract",
                Vec::new(),
            );
        }
        if let Some(audience) = command.flags.get("audience") {
            let Some(commands) = command_catalog::audience_schema_specs(audience) else {
                return Err(ExitError::new(
                    "invalid_argument",
                    2,
                    format!("unknown schema audience {audience:?}."),
                    "Use default, advanced, operator, diagnostic, migration, internal, or all.",
                ));
            };
            return self.render_static_success(
                "awiki-cli schema",
                json!({ "commands": commands, "phase": "phase1-shell", "audience": audience }),
                "Static command contract",
                Vec::new(),
            );
        }
        if command.args.is_empty() {
            return self.render_static_success(
                "awiki-cli schema",
                json!({ "commands": command_catalog::default_surface_schema_specs(), "phase": "phase1-shell" }),
                "Static command contract",
                Vec::new(),
            );
        }
        let target = command.args.join(" ");
        let Some(spec) = command_catalog::lookup(&target) else {
            return Err(ExitError::new(
                "not_found",
                5,
                format!("Unknown command schema target {target:?}"),
                "Use `awiki-cli schema` to list command contracts.",
            ));
        };
        self.render_static_success(
            "awiki-cli schema",
            json!({
                "command": command_catalog::schema_spec_for_command(&spec),
                "children": if spec.include_in_default_surface() {
                    command_catalog::SchemaSpecList::Default(command_catalog::default_surface_schema_children_of(spec.name))
                } else {
                    command_catalog::SchemaSpecList::All(command_catalog::children_of(spec.name))
                },
            }),
            &format!("Static contract for {}", spec.name),
            Vec::new(),
        )
    }

    pub fn run_help(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let text = if command.args.is_empty() {
            render_root_help()
        } else {
            let target = command.args.join(" ");
            let Some(spec) = command_catalog::lookup(&target) else {
                return Err(ExitError::new(
                    "not_found",
                    5,
                    format!("Unknown command help target {target:?}"),
                    "Use `awiki-cli --help` to inspect supported commands.",
                ));
            };
            render_command_help(&spec)
        };
        print!("{text}");
        Ok(())
    }

    pub async fn run_init_async(&self) -> Result<(), ExitError> {
        let mut resolved = self.resolve_config()?;
        let dirs = init_dirs(&resolved);
        if self.globals.dry_run {
            return self.render_success(
                "awiki-cli init",
                &resolved,
                json!({
                    "plan": {
                        "action": "init_workspace",
                        "root_dir": resolved.paths.workspace_home_dir,
                        "root_source": resolved.sources.get("workspace_home_dir"),
                        "directories": dirs,
                        "config_file": resolved.paths.config_file,
                        "config_exists": resolved.config_exists,
                        "config_error": resolved.config_error,
                    }
                }),
                "Dry run: workspace initialization planned",
                Vec::new(),
            );
        }
        for dir in &dirs {
            fs::create_dir_all(dir).map_err(internal_io)?;
        }
        if !resolved.config_exists {
            workspace_config::write_file_config(&resolved.paths.config_file, &resolved)
                .map_err(internal_anyhow)?;
            resolved.config_exists = true;
        }
        ensure_sqlite_schema(&resolved)
            .await
            .map_err(internal_anyhow)?;
        self.render_success(
            "awiki-cli init",
            &resolved,
            json!({
                "workspace": {
                    "root_dir": resolved.paths.workspace_home_dir,
                    "root_source": resolved.sources.get("workspace_home_dir"),
                    "paths": resolved.paths,
                    "config_file": resolved.paths.config_file,
                    "config_exists": resolved.config_exists,
                },
                "listener": {
                    "enabled": resolved.runtime_listener_enabled,
                    "auto_install": resolved.runtime_listener_auto_install,
                    "auto_start": resolved.runtime_listener_auto_start,
                    "managed_by": "awiki-cli runtime listener",
                    "status_command": "awiki-cli runtime listener status",
                }
            }),
            "Workspace initialized",
            Vec::new(),
        )
    }

    pub fn run_completion(&self, shell: &str) -> Result<(), ExitError> {
        let script = match shell {
            "bash" => "_awiki-cli() {\n  COMPREPLY=()\n}\ncomplete -F _awiki-cli awiki-cli\n",
            "zsh" => "#compdef awiki-cli\n_arguments '*::arg:->args'\n",
            "fish" => "complete -c awiki-cli -f\n",
            "powershell" => "Register-ArgumentCompleter -CommandName awiki-cli -ScriptBlock {}\n",
            _ => "",
        };
        print!("{script}");
        Ok(())
    }

    pub fn run_id_create(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let name = command.flags.get("name").cloned().unwrap_or_default();
        let identity_name = command
            .flags
            .get("identity")
            .filter(|value| !value.trim().is_empty())
            .cloned()
            .unwrap_or_default();
        if name.trim().is_empty() {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "id create requires --name.",
                "Usage: awiki-cli id create --name \"Alice\" [--identity alice]",
            ));
        }
        let resolved = self.resolve_config_for_workspace()?;
        require_legacy_file_compat_identity_storage(&resolved, "id create")?;
        let manager = self.identity_manager(&resolved);
        if self.globals.dry_run {
            let existing = manager.list().unwrap_or_default();
            let alias =
                legacy_identity::choose_default_identity_name(&identity_name, &existing, &name);
            return self.render_identity_result(
                "awiki-cli id create",
                &resolved,
                CommandResult {
                    data: json!({
                        "plan": {
                            "action": "create_identity",
                            "identity_name": alias,
                            "display_name": name,
                            "writes": ["index.json", "identity.json", "auth.json", "did_document.json", "key-1-private.pem", "key-1-public.pem", "e2ee-signing-private.pem", "e2ee-agreement-private.pem"],
                        }
                    }),
                    summary: "Dry run: local DID identity creation planned".to_string(),
                    warnings: Vec::new(),
                },
            );
        }
        let result =
            legacy_identity::create_migration_identity(&resolved, &manager, &name, &identity_name)
                .map_err(identity_exit)?;
        self.render_identity_result(
            "awiki-cli id create",
            &resolved,
            legacy_command_result(result),
        )
    }

    pub fn run_id_register(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let result = if self.globals.dry_run {
            crate::m_core_cli_adapter::identity::register_handle_plan_via_im_core(
                &resolved,
                command,
                &self.globals.identity,
            )?
        } else {
            crate::m_core_cli_adapter::identity::register_handle_via_im_core(
                &resolved,
                command,
                &self.globals.identity,
            )?
        };
        self.render_identity_result("awiki-cli id register", &resolved, result)
    }

    pub async fn run_id_register_async(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let result = if self.globals.dry_run {
            crate::m_core_cli_adapter::identity::register_handle_plan_via_im_core_async(
                &resolved,
                command,
                &self.globals.identity,
            )
            .await?
        } else {
            crate::m_core_cli_adapter::identity::register_handle_via_im_core_async(
                &resolved,
                command,
                &self.globals.identity,
            )
            .await?
        };
        self.render_identity_result("awiki-cli id register", &resolved, result)
    }

    pub fn run_id_list(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let result = crate::m_core_cli_adapter::identity::list_identities_via_im_core(&resolved)?;
        self.render_identity_result("awiki-cli id list", &resolved, result)
    }

    pub async fn run_id_list_async(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let result =
            crate::m_core_cli_adapter::identity::list_identities_via_im_core_async(&resolved)
                .await?;
        self.render_identity_result("awiki-cli id list", &resolved, result)
    }

    pub fn run_id_current(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let result = crate::m_core_cli_adapter::identity::current_identity_via_im_core(&resolved)?;
        self.render_identity_result("awiki-cli id current", &resolved, result)
    }

    pub async fn run_id_current_async(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let result =
            crate::m_core_cli_adapter::identity::current_identity_via_im_core_async(&resolved)
                .await?;
        self.render_identity_result("awiki-cli id current", &resolved, result)
    }

    pub fn run_id_use(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        if command.args.len() != 1 {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "id use requires exactly one identity name.",
                "Usage: awiki-cli id use <identity>",
            ));
        }
        let resolved = self.resolve_config_for_workspace()?;
        let result = if self.globals.dry_run {
            crate::m_core_cli_adapter::identity::use_identity_plan_via_im_core(&command.args[0])
        } else {
            crate::m_core_cli_adapter::identity::use_identity_via_im_core(
                &resolved,
                &command.args[0],
            )?
        };
        self.render_identity_result("awiki-cli id use", &resolved, result)
    }

    pub async fn run_id_use_async(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        if command.args.len() != 1 {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "id use requires exactly one identity name.",
                "Usage: awiki-cli id use <identity>",
            ));
        }
        let resolved = self.resolve_config_for_workspace()?;
        let result = if self.globals.dry_run {
            crate::m_core_cli_adapter::identity::use_identity_plan_via_im_core(&command.args[0])
        } else {
            crate::m_core_cli_adapter::identity::use_identity_via_im_core_async(
                &resolved,
                &command.args[0],
            )
            .await?
        };
        self.render_identity_result("awiki-cli id use", &resolved, result)
    }

    pub fn run_id_status(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let result = crate::m_core_cli_adapter::identity::identity_status_via_im_core(&resolved)?;
        self.render_identity_result("awiki-cli id status", &resolved, result)
    }

    pub async fn run_id_status_async(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let result =
            crate::m_core_cli_adapter::identity::identity_status_via_im_core_async(&resolved)
                .await?;
        self.render_identity_result("awiki-cli id status", &resolved, result)
    }

    pub fn run_id_vault_status(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let selector =
            crate::m_core_cli_adapter::identity::cli_identity_selector(&self.globals.identity);
        let result = crate::m_core_cli_adapter::vault::identity_vault_status_via_im_core(
            &resolved, selector,
        )
        .map_err(crate::m_core_cli_adapter::error::map_identity_boundary_error)?;
        self.render_identity_result("awiki-cli id vault status", &resolved, result)
    }

    pub async fn run_id_vault_status_async(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let selector =
            crate::m_core_cli_adapter::identity::cli_identity_selector(&self.globals.identity);
        let result = crate::m_core_cli_adapter::vault::identity_vault_status_via_im_core_async(
            &resolved, selector,
        )
        .await
        .map_err(crate::m_core_cli_adapter::error::map_identity_boundary_error)?;
        self.render_identity_result("awiki-cli id vault status", &resolved, result)
    }

    pub fn run_id_vault_migrate(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let selector =
            crate::m_core_cli_adapter::identity::cli_identity_selector(&self.globals.identity);
        let result = crate::m_core_cli_adapter::vault::identity_vault_migrate_via_im_core(
            &resolved,
            selector,
            self.globals.dry_run,
        )?;
        self.render_identity_result("awiki-cli id vault migrate", &resolved, result)
    }

    pub async fn run_id_vault_migrate_async(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let selector =
            crate::m_core_cli_adapter::identity::cli_identity_selector(&self.globals.identity);
        let result = crate::m_core_cli_adapter::vault::identity_vault_migrate_via_im_core_async(
            &resolved,
            selector,
            self.globals.dry_run,
        )
        .await?;
        self.render_identity_result("awiki-cli id vault migrate", &resolved, result)
    }

    pub fn run_id_vault_cleanup_plaintext(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let selector =
            crate::m_core_cli_adapter::identity::cli_identity_selector(&self.globals.identity);
        let result =
            crate::m_core_cli_adapter::vault::identity_vault_cleanup_plaintext_via_im_core(
                &resolved,
                selector,
                self.globals.dry_run,
            )?;
        self.render_identity_result("awiki-cli id vault cleanup-plaintext", &resolved, result)
    }

    pub async fn run_id_vault_cleanup_plaintext_async(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let selector =
            crate::m_core_cli_adapter::identity::cli_identity_selector(&self.globals.identity);
        let result =
            crate::m_core_cli_adapter::vault::identity_vault_cleanup_plaintext_via_im_core_async(
                &resolved,
                selector,
                self.globals.dry_run,
            )
            .await?;
        self.render_identity_result("awiki-cli id vault cleanup-plaintext", &resolved, result)
    }

    pub fn run_id_import_v1(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        require_legacy_file_compat_identity_storage(&resolved, "id import-v1")?;
        let name = command.flags.get("name").cloned().unwrap_or_default();
        let import_all = command
            .flags
            .get("all")
            .is_some_and(|value| value == "true");
        if self.globals.dry_run {
            return self.render_identity_result(
                "awiki-cli id import-v1",
                &resolved,
                CommandResult {
                    data: json!({
                        "plan": {
                            "action": "import_v1_identities",
                            "name": name,
                            "all": import_all,
                        }
                    }),
                    summary: "Dry run: v1 credential import planned".to_string(),
                    warnings: Vec::new(),
                },
            );
        }
        let result = legacy_identity::import_v1_migration(
            &self.identity_manager(&resolved),
            &name,
            import_all,
        )
        .map_err(identity_exit)?;
        self.render_identity_result(
            "awiki-cli id import-v1",
            &resolved,
            legacy_command_result(result),
        )
    }

    pub fn run_id_bind(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let result = if self.globals.dry_run {
            crate::m_core_cli_adapter::identity::bind_contact_plan_via_im_core(command)?
        } else {
            crate::m_core_cli_adapter::identity::bind_contact_via_im_core(
                &resolved,
                &self.globals.identity,
                command,
            )
            .map_err(crate::m_core_cli_adapter::error::map_identity_boundary_error)?
        };
        self.render_identity_result("awiki-cli id bind", &resolved, result)
    }

    pub async fn run_id_bind_async(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let result = if self.globals.dry_run {
            crate::m_core_cli_adapter::identity::bind_contact_plan_via_im_core(command)?
        } else {
            crate::m_core_cli_adapter::identity::bind_contact_via_im_core_async(
                &resolved,
                &self.globals.identity,
                command,
            )
            .await
            .map_err(crate::m_core_cli_adapter::error::map_identity_boundary_error)?
        };
        self.render_identity_result("awiki-cli id bind", &resolved, result)
    }

    pub fn run_id_refresh_token(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let result = if self.globals.dry_run {
            crate::m_core_cli_adapter::auth::refresh_token_plan_via_im_core(&self.globals.identity)
        } else {
            crate::m_core_cli_adapter::auth::refresh_token_via_im_core(
                &resolved,
                &self.globals.identity,
            )
            .map_err(crate::m_core_cli_adapter::error::map_identity_boundary_error)?
        };
        self.render_identity_result("awiki-cli id refresh-token", &resolved, result)
    }

    pub async fn run_id_refresh_token_async(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let result = if self.globals.dry_run {
            crate::m_core_cli_adapter::auth::refresh_token_plan_via_im_core(&self.globals.identity)
        } else {
            crate::m_core_cli_adapter::auth::refresh_token_via_im_core_async(
                &resolved,
                &self.globals.identity,
            )
            .await
            .map_err(crate::m_core_cli_adapter::error::map_identity_boundary_error)?
        };
        self.render_identity_result("awiki-cli id refresh-token", &resolved, result)
    }

    pub fn run_id_profile_set(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let display_name = string_flag(command, "display-name");
        let bio = string_flag(command, "bio");
        let tags = string_flag(command, "tags");
        let markdown = string_flag(command, "markdown");
        let markdown_file = string_flag(command, "markdown-file");
        let avatar_uri = string_flag(command, "avatar-uri");
        let avatar_url = string_flag(command, "avatar-url");
        if !markdown.trim().is_empty() && !markdown_file.trim().is_empty() {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "Use either --markdown or --markdown-file, not both.",
                "Choose one profile body source.",
            ));
        }
        let resolved = self.resolve_config_for_workspace()?;
        if self.globals.dry_run {
            return self.render_identity_result(
                "awiki-cli id profile set",
                &resolved,
                CommandResult {
                    data: json!({
                        "plan": {
                            "action": "update_profile",
                            "display_name": display_name,
                            "bio": bio,
                            "tags": tags,
                            "markdown": markdown,
                            "markdown_file": markdown_file,
                            "avatar_uri": avatar_uri,
                            "avatar_url": avatar_url,
                            "remote_calls": ["did.profile.update_me"],
                            "local_writes": ["identity.json", "index.json"],
                        }
                    }),
                    summary: "Dry run: profile update planned".to_string(),
                    warnings: Vec::new(),
                },
            );
        }
        let request = crate::m_core_cli_adapter::identity::set_profile_request(
            display_name,
            bio,
            tags,
            markdown,
            markdown_file,
            avatar_uri,
            avatar_url,
        )?;
        let result = crate::m_core_cli_adapter::identity::set_profile_via_im_core(
            &resolved,
            &self.globals.identity,
            request,
        )
        .map_err(crate::m_core_cli_adapter::error::map_identity_boundary_error)?;
        self.render_identity_result("awiki-cli id profile set", &resolved, result)
    }

    pub async fn run_id_profile_set_async(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let display_name = string_flag(command, "display-name");
        let bio = string_flag(command, "bio");
        let tags = string_flag(command, "tags");
        let markdown = string_flag(command, "markdown");
        let markdown_file = string_flag(command, "markdown-file");
        let avatar_uri = string_flag(command, "avatar-uri");
        let avatar_url = string_flag(command, "avatar-url");
        if !markdown.trim().is_empty() && !markdown_file.trim().is_empty() {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "Use either --markdown or --markdown-file, not both.",
                "Choose one profile body source.",
            ));
        }
        let resolved = self.resolve_config_for_workspace()?;
        if self.globals.dry_run {
            return self.run_id_profile_set(command);
        }
        let request = crate::m_core_cli_adapter::identity::set_profile_request(
            display_name,
            bio,
            tags,
            markdown,
            markdown_file,
            avatar_uri,
            avatar_url,
        )?;
        let result = crate::m_core_cli_adapter::identity::set_profile_via_im_core_async(
            &resolved,
            &self.globals.identity,
            request,
        )
        .await
        .map_err(crate::m_core_cli_adapter::error::map_identity_boundary_error)?;
        self.render_identity_result("awiki-cli id profile set", &resolved, result)
    }

    pub fn run_id_profile_get(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let request = crate::m_core_cli_adapter::identity::get_profile_request(command);
        let self_profile = request.self_profile
            || (request.handle.trim().is_empty() && request.did.trim().is_empty());
        let result = if self_profile {
            crate::m_core_cli_adapter::identity::get_self_profile_via_im_core(
                &resolved,
                &self.globals.identity,
            )
            .map_err(crate::m_core_cli_adapter::error::map_identity_boundary_error)?
        } else {
            crate::m_core_cli_adapter::identity::get_public_profile_via_im_core(
                &resolved,
                &self.globals.identity,
                request,
            )?
        };
        self.render_identity_result("awiki-cli id profile get", &resolved, result)
    }

    pub async fn run_id_profile_get_async(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let request = crate::m_core_cli_adapter::identity::get_profile_request(command);
        let self_profile = request.self_profile
            || (request.handle.trim().is_empty() && request.did.trim().is_empty());
        let result = if self_profile {
            crate::m_core_cli_adapter::identity::get_self_profile_via_im_core_async(
                &resolved,
                &self.globals.identity,
            )
            .await
            .map_err(crate::m_core_cli_adapter::error::map_identity_boundary_error)?
        } else {
            crate::m_core_cli_adapter::identity::get_public_profile_via_im_core_async(
                &resolved,
                &self.globals.identity,
                request,
            )
            .await?
        };
        self.render_identity_result("awiki-cli id profile get", &resolved, result)
    }

    pub fn run_id_resolve(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let result = crate::m_core_cli_adapter::identity::resolve_identity_via_im_core(
            &resolved,
            &self.globals.identity,
            crate::m_core_cli_adapter::identity::resolve_request(command),
        )?;
        self.render_identity_result("awiki-cli id resolve", &resolved, result)
    }

    pub async fn run_id_resolve_async(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let result = crate::m_core_cli_adapter::identity::resolve_identity_via_im_core_async(
            &resolved,
            &self.globals.identity,
            crate::m_core_cli_adapter::identity::resolve_request(command),
        )
        .await?;
        self.render_identity_result("awiki-cli id resolve", &resolved, result)
    }

    fn resolve_config(&self) -> Result<Resolved, ExitError> {
        self.resolve_config_raw().map_err(internal_anyhow)
    }

    fn resolve_config_for_workspace(&self) -> Result<Resolved, ExitError> {
        let resolved = self.resolve_config_raw().map_err(internal_anyhow)?;
        if self.globals.dry_run {
            return Ok(resolved);
        }

        let mut phase = cli_trace::start_phase("workspace_upgrade");
        let result = workspace_upgrade::upgrade_if_needed(&resolved, crate::build_info::VERSION);
        phase.finish();
        result.map_err(|err| internal_anyhow(anyhow::Error::new(err)))?;

        let resolved = self.resolve_config_untraced().map_err(internal_anyhow)?;
        if legacy_file_compat_identity_storage_enabled(&resolved)? {
            legacy_identity::ensure_all_identity_private_keys_compatible(&resolved.paths)
                .map_err(identity_exit)?;
        }
        Ok(resolved)
    }

    fn open_store(
        &self,
        resolved: &Resolved,
        hint: &str,
    ) -> Result<rusqlite::Connection, ExitError> {
        store::open(&resolved.paths).map_err(|err| store_exit(err, hint))
    }

    fn identity_manager(&self, resolved: &Resolved) -> Manager {
        Manager::new(resolved.paths.clone())
    }

    fn render_identity_result(
        &self,
        command: &str,
        resolved: &Resolved,
        result: CommandResult,
    ) -> Result<(), ExitError> {
        self.render_success(
            command,
            resolved,
            sanitize_public_value(result.data),
            &result.summary,
            result.warnings,
        )
    }

    fn render_success(
        &self,
        command: &str,
        resolved: &Resolved,
        data: Value,
        summary: &str,
        warnings: Vec<String>,
    ) -> Result<(), ExitError> {
        let format = cli_output::normalize_format(&resolved.output_format).unwrap_or(Format::Json);
        let warnings = update_preflight::merge_update_warning(&self.update_warning, warnings);
        let envelope = SuccessEnvelope {
            ok: true,
            command: command.to_string(),
            data,
            warnings,
            summary: summary.to_string(),
            notice: None,
            meta: Meta {
                version: crate::build_info::VERSION.to_string(),
                identity: identity_meta_from_resolved(resolved),
                dry_run: self.globals.dry_run,
                format: format.as_str().to_string(),
            },
        };
        cli_output::render_success(io::stdout(), format, &self.globals.jq, &envelope)
            .map_err(internal_anyhow)
    }

    fn render_static_success(
        &self,
        command: &str,
        data: Value,
        summary: &str,
        warnings: Vec<String>,
    ) -> Result<(), ExitError> {
        let format = cli_output::normalize_format(&self.globals.format).unwrap_or(Format::Json);
        let warnings = update_preflight::merge_update_warning(&self.update_warning, warnings);
        let envelope = SuccessEnvelope {
            ok: true,
            command: command.to_string(),
            data,
            warnings,
            summary: summary.to_string(),
            notice: None,
            meta: Meta {
                version: crate::build_info::VERSION.to_string(),
                identity: None,
                dry_run: self.globals.dry_run,
                format: format.as_str().to_string(),
            },
        };
        cli_output::render_success(io::stdout(), format, &self.globals.jq, &envelope)
            .map_err(internal_anyhow)
    }

    fn handle_error(&self, err: ExitError) -> i32 {
        let format = cli_output::normalize_format(&self.globals.format).unwrap_or(Format::Json);
        let envelope = ErrorEnvelope {
            ok: false,
            error: err.detail.clone(),
            notice: None,
            meta: Meta {
                version: crate::build_info::VERSION.to_string(),
                identity: None,
                dry_run: self.globals.dry_run,
                format: format.as_str().to_string(),
            },
        };
        if cli_output::render_error(io::stderr(), format, &self.globals.jq, &envelope).is_err() {
            let _ = writeln!(io::stderr(), "{}", err.detail.message);
        }
        err.exit_code
    }
}

fn optional_bool_flag(command: &ParsedCommand, name: &str) -> Result<Option<bool>, ExitError> {
    if !command.changed_flags.iter().any(|flag| flag == name) {
        return Ok(None);
    }
    let value = command
        .flags
        .get(name)
        .map(String::as_str)
        .unwrap_or("false")
        .trim()
        .to_ascii_lowercase();
    match value.as_str() {
        "true" | "1" | "yes" | "on" => Ok(Some(true)),
        "false" | "0" | "no" | "off" => Ok(Some(false)),
        _ => Err(ExitError::new(
            "invalid_argument",
            2,
            format!("--{name} must be a boolean."),
            "Use true or false.",
        )),
    }
}

fn changed_string_flag(command: &ParsedCommand, name: &str) -> Option<String> {
    if command.changed_flags.iter().any(|flag| flag == name) {
        Some(command.flags.get(name).cloned().unwrap_or_default())
    } else {
        None
    }
}

fn string_flag(command: &ParsedCommand, name: &str) -> String {
    command.flags.get(name).cloned().unwrap_or_default()
}

fn identity_status(resolved: &Resolved) -> Value {
    json!({
        "active_identity": if resolved.active_identity.is_empty() { Value::Null } else { json!(resolved.active_identity) },
        "identity_count": count_identity_dirs(&resolved.paths.identity_dir),
        "legacy_scan": {
            "credentials_dir": resolved.paths.legacy_credentials_dir,
            "data_dir": resolved.paths.legacy_data_dir,
            "identities": [],
        },
    })
}

fn identity_store_snapshot(resolved: &Resolved) -> Value {
    json!({
        "identity_dir": resolved.paths.identity_dir,
        "index_file": Path::new(&resolved.paths.identity_dir).join("index.json").to_string_lossy(),
        "default_identity": Value::Null,
        "legacy_scan": {
            "credentials_dir": resolved.paths.legacy_credentials_dir,
            "data_dir": resolved.paths.legacy_data_dir,
            "identities": [],
        },
    })
}

fn database_snapshot(resolved: &Resolved) -> Value {
    let exists = Path::new(&resolved.paths.database_file).exists();
    json!({
        "database_file": resolved.paths.database_file,
        "exists": exists,
    })
}

fn workspace_upgrade_snapshot(resolved: &Resolved) -> Value {
    workspace_upgrade::inspect(resolved, crate::build_info::VERSION)
        .map(|inspection| serde_json::to_value(inspection).unwrap_or_else(|_| json!({})))
        .unwrap_or_else(|err| {
            let paths = workspace_upgrade::resolve_paths(resolved);
            json!({
                "paths": paths,
                "meta": Value::Null,
                "journal": Value::Null,
                "detection": workspace_upgrade::detect(resolved, None),
                "error": err.to_string(),
            })
        })
}

fn init_dirs(resolved: &Resolved) -> Vec<String> {
    vec![
        resolved.paths.workspace_home_dir.clone(),
        resolved.paths.config_dir.clone(),
        resolved.paths.data_dir.clone(),
        resolved.paths.state_dir.clone(),
        resolved.paths.cache_dir.clone(),
        resolved.paths.logs_dir.clone(),
        resolved.paths.identity_dir.clone(),
        Path::new(&resolved.paths.workspace_home_dir)
            .join("upgrade")
            .to_string_lossy()
            .into_owned(),
    ]
}

fn required_arg<'a>(
    command: &'a ParsedCommand,
    usage: &str,
    name: &str,
) -> Result<&'a str, ExitError> {
    match command.args.as_slice() {
        [value] if !value.trim().is_empty() => Ok(value.trim()),
        [] => Err(ExitError::new(
            "invalid_argument",
            2,
            format!("{usage} requires <{name}>."),
            format!("Usage: awiki-cli {usage} <{name}>."),
        )),
        _ => Err(ExitError::new(
            "invalid_argument",
            2,
            format!("{usage} accepts exactly one <{name}>."),
            format!("Usage: awiki-cli {usage} <{name}>."),
        )),
    }
}

fn required_flag<'a>(
    command: &'a ParsedCommand,
    usage: &str,
    name: &str,
) -> Result<&'a str, ExitError> {
    let value = command
        .flags
        .get(name)
        .map(String::as_str)
        .unwrap_or_default();
    if value.trim().is_empty() {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            format!("{usage} requires --{name}."),
            format!("Usage: awiki-cli {usage} --{name} <value>."),
        ));
    }
    Ok(value.trim())
}

async fn ensure_sqlite_schema(resolved: &Resolved) -> anyhow::Result<()> {
    crate::m_core_cli_adapter::build_im_core(resolved)?
        .bootstrap()
        .initialize_local_state_async()
        .await?;
    Ok(())
}

fn ensure_sqlite_schema_blocking(resolved: &Resolved) -> anyhow::Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| anyhow::anyhow!("failed to start async runtime: {err}"))?
        .block_on(ensure_sqlite_schema(resolved))
}

fn identity_meta_from_resolved(resolved: &Resolved) -> Option<IdentityMeta> {
    if resolved.active_identity.trim().is_empty() {
        return None;
    }
    Some(IdentityMeta {
        name: resolved.active_identity.clone(),
        did: String::new(),
    })
}

fn count_identity_dirs(path: &str) -> usize {
    fs::read_dir(path)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| entry.path().is_dir())
                .count()
        })
        .unwrap_or(0)
}

fn sanitize_public_value(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| {
                    if is_sensitive_public_key(&key) {
                        (key, Value::String("[redacted]".to_string()))
                    } else {
                        (key, sanitize_public_value(value))
                    }
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(sanitize_public_value).collect()),
        other => other,
    }
}

fn render_root_help() -> String {
    let mut lines = vec![
        "AWiki CLI".to_string(),
        String::new(),
        "Usage:".to_string(),
        "  awiki-cli <command> [flags]".to_string(),
        String::new(),
        "Commands:".to_string(),
    ];
    append_command_rows(&mut lines, &command_catalog::public_help_root_specs());
    lines.extend([
        String::new(),
        "Global Flags:".to_string(),
        "  --tenant <name>      Use an existing tenant for this command only".to_string(),
        "  --identity <name>    Use a local identity for this command only".to_string(),
        "  --format <format>    Output format for data commands: json, pretty, table, ndjson"
            .to_string(),
        "  --dry-run            Show the planned side effects without applying them".to_string(),
        "  --help, -h           Show help".to_string(),
        String::new(),
        "Use `awiki-cli <command> --help` for command-specific help.".to_string(),
        "Use `awiki-cli schema [COMMAND]` for machine-readable command metadata.".to_string(),
    ]);
    finish_help(lines)
}

fn render_command_help(spec: &command_catalog::CommandSpec) -> String {
    let mut lines = vec![spec.short.to_string()];
    if !spec.long.trim().is_empty() {
        lines.extend([String::new(), spec.long.to_string()]);
    }
    lines.extend([
        String::new(),
        "Usage:".to_string(),
        format!("  awiki-cli {}", command_usage(spec)),
    ]);

    let children = command_catalog::public_help_children_of(spec.name);
    if !children.is_empty() {
        lines.extend([String::new(), "Commands:".to_string()]);
        append_command_rows(&mut lines, &children);
    }

    if !spec.flags.is_empty() {
        lines.extend([String::new(), "Flags:".to_string()]);
        append_flag_rows(&mut lines, spec.flags);
    }

    lines.extend([
        String::new(),
        format!(
            "Use `awiki-cli schema {}` for machine-readable command metadata.",
            spec.name.replace('.', " ")
        ),
    ]);
    finish_help(lines)
}

fn command_usage(spec: &command_catalog::CommandSpec) -> String {
    let mut segments: Vec<&str> = spec.name.split('.').collect();
    if !segments.is_empty() {
        let last = segments.len() - 1;
        segments[last] = spec.use_;
    }
    segments.join(" ")
}

fn append_command_rows(lines: &mut Vec<String>, commands: &[command_catalog::CommandSpec]) {
    for spec in commands {
        lines.push(format!(
            "  {:<18} {}",
            command_display_name(spec),
            spec.short
        ));
    }
}

fn command_display_name(spec: &command_catalog::CommandSpec) -> String {
    spec.use_
        .split_whitespace()
        .next()
        .unwrap_or(spec.use_)
        .to_string()
}

fn append_flag_rows(lines: &mut Vec<String>, flags: &[command_catalog::FlagSpec]) {
    for flag in flags {
        if flag.deprecated {
            continue;
        }
        let value_hint = match flag.flag_type {
            "bool" => String::new(),
            value if value.is_empty() => " <value>".to_string(),
            value => format!(" <{value}>"),
        };
        let required = if flag.required { " (required)" } else { "" };
        lines.push(format!(
            "  --{:<22} {}{}",
            format!("{}{}", flag.name, value_hint),
            flag.usage,
            required
        ));
    }
}

fn finish_help(mut lines: Vec<String>) -> String {
    lines.push(String::new());
    lines.join("\n")
}

fn is_sensitive_public_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase();
    normalized == "jwt_token"
        || normalized == "did_document"
        || normalized == "key1_public_pem"
        || normalized == "root_key_material"
        || normalized.ends_with("_private_pem")
        || normalized.ends_with("_private_key_pem")
        || normalized.ends_with("_public_pem")
        || normalized.ends_with("_token")
        || normalized.ends_with("_secret")
        || normalized.ends_with("_secret_ref")
        || normalized.ends_with("_secret_ref_json")
        || normalized.ends_with("_ref_json")
        || normalized == "secret_ref"
        || normalized == "secretref"
}

fn legacy_command_result(result: legacy_identity::CommandResult) -> CommandResult {
    CommandResult {
        data: result.data,
        summary: result.summary,
        warnings: result.warnings,
    }
}

fn legacy_file_compat_identity_storage_enabled(resolved: &Resolved) -> Result<bool, ExitError> {
    let secret_storage = workspace_config::resolve_secret_storage(resolved).map_err(|err| {
        ExitError::new(
            "invalid_config",
            2,
            format!("invalid secret_storage config: {err}"),
            "Use secret_storage.mode=file_compat only for legacy plaintext identity migrations.",
        )
    })?;
    Ok(secret_storage.mode == "file_compat")
}

fn require_legacy_file_compat_identity_storage(
    resolved: &Resolved,
    command: &'static str,
) -> Result<(), ExitError> {
    if legacy_file_compat_identity_storage_enabled(resolved)? {
        return Ok(());
    }
    Err(ExitError::new(
        "legacy_plaintext_identity_storage_disabled",
        3,
        format!("{command}: legacy plaintext identity storage is disabled."),
        "Use `awiki-cli id register` or `awiki-cli id recover` for vault-backed identities. Set secret_storage.mode=file_compat only for explicit legacy migration work.",
    ))
}

pub(crate) fn identity_exit(err: IdentityError) -> ExitError {
    match err {
        IdentityError::InvalidInput(message) => ExitError::new(
            "invalid_argument",
            2,
            message,
            "Run `awiki-cli id list` to inspect available identities.",
        ),
        IdentityError::NotFound(message)
        | IdentityError::LegacyNotFound(message)
        | IdentityError::NoDefaultIdentity(message) => ExitError::new(
            "not_found",
            5,
            message,
            "Run `awiki-cli id list` to inspect available identities.",
        ),
        IdentityError::Conflict(message) => ExitError::new(
            "conflict",
            1,
            message,
            "Use a different --identity value if the alias is already occupied.",
        ),
        IdentityError::AuthRequired(message) => ExitError::new(
            "auth_required",
            3,
            message,
            "Use an identity with valid DID key material, or run `awiki-cli id refresh-token` / `awiki-cli id register` / `awiki-cli id recover` first.",
        ),
        IdentityError::Service(err) => {
            identity_service_exit(err.status_code, err.rpc_code, err.to_string())
        }
        IdentityError::Io(err) => ExitError::new(
            "internal_error",
            1,
            err.to_string(),
            "Run `awiki-cli doctor` to inspect the local identity store.",
        ),
        IdentityError::Json(err) => ExitError::new(
            "internal_error",
            1,
            err.to_string(),
            "Run `awiki-cli doctor` to inspect the local identity store.",
        ),
        IdentityError::Internal(message) => ExitError::new(
            "internal_error",
            1,
            message,
            "Run `awiki-cli doctor` to inspect configuration and storage paths.",
        ),
    }
}

fn identity_service_exit(status_code: u16, rpc_code: i64, message: String) -> ExitError {
    match (status_code, rpc_code) {
        (400, _) => ExitError::new(
            "invalid_argument",
            2,
            message,
            "Ensure the handle, verification method, and local alias are valid.",
        ),
        (401, _) => ExitError::new(
            "auth_required",
            3,
            message,
            "Use an identity with valid DID key material, or run `awiki-cli id refresh-token` / `awiki-cli id register` / `awiki-cli id recover` first.",
        ),
        (404, _) => ExitError::new(
            "not_found",
            5,
            message,
            "Ensure the handle, verification method, and local alias are valid.",
        ),
        (409, _) => ExitError::new(
            "conflict",
            1,
            message,
            "Ensure the handle, verification method, and local alias are valid.",
        ),
        (_, rpc_code) if rpc_code != 0 => match rpc_code {
            -32602 => ExitError::new(
                "invalid_argument",
                2,
                message,
                "Ensure the handle, verification method, and local alias are valid.",
            ),
            -32000 => ExitError::new(
                "auth_required",
                3,
                message,
                "Use an identity with valid DID key material, or run `awiki-cli id refresh-token` / `awiki-cli id register` / `awiki-cli id recover` first.",
            ),
            -32002 => ExitError::new(
                "not_found",
                5,
                message,
                "Ensure the handle, verification method, and local alias are valid.",
            ),
            -32003 | -32004 => ExitError::new(
                "conflict",
                1,
                message,
                "Ensure the handle, verification method, and local alias are valid.",
            ),
            _ => ExitError::new(
                "internal_error",
                1,
                message,
                "Ensure the handle, verification method, and local alias are valid.",
            ),
        },
        _ => ExitError::new(
            "internal_error",
            1,
            message,
            "Ensure the handle, verification method, and local alias are valid.",
        ),
    }
}

fn internal_io(err: std::io::Error) -> ExitError {
    ExitError::new(
        "internal_error",
        1,
        err.to_string(),
        "Check directory permissions for the awiki-cli workspace.",
    )
}

fn internal_anyhow(err: anyhow::Error) -> ExitError {
    if let Some(err) = err.downcast_ref::<workspace_config::WorkspaceConfigError>() {
        return workspace_config_exit(err);
    }
    let hint = error_hints::refine_workspace_write_hint(
        &err,
        "Run `awiki-cli doctor` to inspect the local workspace state.",
    );
    ExitError::new("internal_error", 1, err.to_string(), hint)
}

fn workspace_config_exit(err: &workspace_config::WorkspaceConfigError) -> ExitError {
    match err.kind() {
        WorkspaceConfigErrorKind::InvalidArgument => {
            ExitError::new("invalid_argument", 2, err.to_string(), err.hint())
        }
        WorkspaceConfigErrorKind::InvalidConfig => {
            ExitError::new("invalid_config", 2, err.to_string(), err.hint())
        }
        WorkspaceConfigErrorKind::NotFound => {
            ExitError::new("not_found", 5, err.to_string(), err.hint())
        }
        WorkspaceConfigErrorKind::Conflict => {
            ExitError::new("conflict", 1, err.to_string(), err.hint())
        }
    }
}

fn store_exit(err: StoreError, hint: &str) -> ExitError {
    match err {
        StoreError::LegacyDatabaseNotFound | StoreError::NotFound(_) => {
            ExitError::new("not_found", 5, err.to_string(), hint)
        }
        StoreError::UnsafeSql(_) | StoreError::UnsupportedLegacySchema(_) => {
            ExitError::new("invalid_argument", 2, err.to_string(), hint)
        }
        StoreError::Invalid(_) | StoreError::Sqlite(_) | StoreError::Io(_) => {
            ExitError::new("internal_error", 1, err.to_string(), hint)
        }
    }
}

fn legacy_owner_lookup(manager: &Manager) -> store::LegacyOwnerLookup {
    let entries = manager
        .list()
        .unwrap_or_default()
        .into_iter()
        .map(|summary| {
            (
                summary.unique_id,
                summary.identity_name,
                summary.did,
                summary.is_default,
            )
        });
    store::LegacyOwnerLookup::from_identity_entries(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_anyhow_refines_windows_directory_sync_hint() {
        let err = internal_anyhow(anyhow::anyhow!(
            r"write config yaml: sync config dir: sync C:\Users\liuzhuocheng\.awiki-cli: Access is denied."
        ));

        assert_eq!(err.detail.code, "internal_error");
        assert_eq!(
            err.detail.hint,
            error_hints::WINDOWS_DIR_SYNC_COMPATIBILITY_HINT
        );
    }

    #[test]
    fn internal_anyhow_keeps_fallback_for_normal_permission_errors() {
        let err = internal_anyhow(anyhow::anyhow!(
            r"create config dir: mkdir C:\Users\liuzhuocheng\.awiki-cli: Access is denied."
        ));

        assert_eq!(
            err.detail.hint,
            "Run `awiki-cli doctor` to inspect the local workspace state."
        );
    }
}
