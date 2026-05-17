use crate::buildinfo::BuildInfo;
use crate::cli::{self, ParsedCommand};
use crate::cmdmeta;
use crate::config::{self, Overrides, Resolved};
use crate::docs;
use crate::doctor;
use crate::identity::{self, IdentityError, Manager};
use crate::output::{self, ErrorEnvelope, ExitError, Format, IdentityMeta, Meta, SuccessEnvelope};
use crate::store::{self, StoreError};
use crate::traceutil;
use crate::upgrade;
use serde_json::{json, Value};
use std::fs;
use std::io::{self, Write};
use std::path::Path;

mod debug_handlers;
mod error_hints;
mod group_e2ee_handlers;
mod group_handlers;
mod id_recover_handlers;
mod id_replace_did_handlers;
mod mail_handlers;
mod msg_handlers;
mod page_handlers;
mod runtime_handlers;
mod runtime_hermes_handlers;
mod runtime_host_notify_refresh;
mod site_handlers;
mod update_handlers;
pub(super) mod update_preflight;

#[derive(Debug, Clone)]
pub struct GlobalOptions {
    pub format: String,
    pub format_changed: bool,
    pub jq: String,
    pub dry_run: bool,
    pub identity: String,
    pub identity_changed: bool,
    pub verbose: bool,
}

impl Default for GlobalOptions {
    fn default() -> Self {
        Self {
            format: "json".to_string(),
            format_changed: false,
            jq: String::new(),
            dry_run: false,
            identity: String::new(),
            identity_changed: false,
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
    let command = match cli::parse_env() {
        Ok(command) => command,
        Err(err) => return App::default().handle_error(err),
    };
    let trace_run = traceutil::Run::new(&command.trace_command());
    traceutil::set_current(Some(trace_run));
    let mut app = App {
        globals: command.globals.clone(),
        update_warning: String::new(),
    };
    let exit_code = if let Err(err) = app.preflight(&command) {
        app.handle_error(err)
    } else {
        match cli::dispatch(&app, &command) {
            Ok(()) => 0,
            Err(err) => app.handle_error(err),
        }
    };
    if !command.emits_raw_output() {
        app.emit_trace();
    }
    traceutil::set_current(None);
    exit_code
}

impl Drop for App {
    fn drop(&mut self) {
        traceutil::set_current(None);
    }
}

impl App {
    fn emit_trace(&self) {
        let _ = traceutil::emit_current(&mut io::stderr());
    }

    pub(super) fn resolve_config_raw(&self) -> anyhow::Result<Resolved> {
        let mut phase = traceutil::start_phase("resolve_config");
        let result = self.resolve_config_untraced();
        phase.finish();
        result
    }

    fn resolve_config_untraced(&self) -> anyhow::Result<Resolved> {
        let result = config::resolve(Overrides {
            identity: self.globals.identity.clone(),
            identity_changed: self.globals.identity_changed,
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
        let mut data = config::snapshot(&resolved);
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

    pub fn run_config_set(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let Some(raw_did_domain) = command.flags.get("did-domain") else {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "config set requires --did-domain.",
                "Use --did-domain <domain>.",
            ));
        };
        let did_domain = config::normalize_did_domain(raw_did_domain).map_err(|err| {
            ExitError::new(
                "invalid_argument",
                2,
                err.to_string(),
                "Use a bare domain such as tenant.example.",
            )
        })?;
        if self.globals.dry_run {
            return self.render_success(
                "awiki-cli config set",
                &resolved,
                json!({
                    "plan": {
                        "action": "config_set_did_domain",
                        "did_domain": did_domain,
                        "config_file": resolved.paths.config_file,
                    }
                }),
                "Dry run: DID domain update planned",
                Vec::new(),
            );
        }
        config::update_did_domain(&resolved.paths, &did_domain).map_err(internal_anyhow)?;
        let resolved = self.resolve_config_for_workspace()?;
        self.render_success(
            "awiki-cli config set",
            &resolved,
            json!({
                "did_domain": resolved.did_domain,
                "config_file": resolved.paths.config_file,
            }),
            "DID domain updated",
            Vec::new(),
        )
    }

    pub fn run_doctor(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let report = doctor::run(&resolved);
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
        let resolved = self.resolve_config()?;
        if args.is_empty() {
            return self.render_success(
                "awiki-cli docs",
                &resolved,
                json!({ "topics": docs::all() }),
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
        let Some(topic) = docs::lookup(raw) else {
            return Err(ExitError::new(
                "not_found",
                5,
                format!("Unknown docs topic {raw:?}"),
                "Run `awiki-cli docs` to list available topics.",
            ));
        };
        self.render_success(
            "awiki-cli docs",
            &resolved,
            json!({ "topic": topic }),
            &format!("Documentation topic {}", topic.name),
            Vec::new(),
        )
    }

    pub fn run_schema(&self, args: &[String]) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        if args.is_empty() {
            return self.render_success(
                "awiki-cli schema",
                &resolved,
                json!({ "commands": cmdmeta::specs(), "phase": "phase1-shell" }),
                "Static command contract",
                Vec::new(),
            );
        }
        let target = args.join(" ");
        let Some(spec) = cmdmeta::lookup(&target) else {
            return Err(ExitError::new(
                "not_found",
                5,
                format!("Unknown command schema target {target:?}"),
                "Use `awiki-cli schema` to list command contracts.",
            ));
        };
        self.render_success(
            "awiki-cli schema",
            &resolved,
            json!({ "command": spec, "children": cmdmeta::children_of(spec.name) }),
            &format!("Static contract for {}", spec.name),
            Vec::new(),
        )
    }

    pub fn run_init(&self) -> Result<(), ExitError> {
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
            config::write_file_config(&resolved.paths.config_file, &resolved)
                .map_err(internal_anyhow)?;
            resolved.config_exists = true;
        }
        ensure_sqlite_schema(&resolved).map_err(internal_anyhow)?;
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
                    "status": "not_managed_in_rust_slice",
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
        let manager = self.identity_manager(&resolved);
        if self.globals.dry_run {
            let existing = manager.list().unwrap_or_default();
            let alias = identity::choose_default_identity_name(&identity_name, &existing, &name);
            return self.render_identity_result(
                "awiki-cli id create",
                &resolved,
                identity::CommandResult {
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
        let result = identity::create_identity(&resolved, &manager, &name, &identity_name)
            .map_err(identity_exit)?;
        self.render_identity_result("awiki-cli id create", &resolved, result)
    }

    pub fn run_id_register(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let params = identity::RegisterParams {
            identity_name: self.globals.identity.clone(),
            handle: string_flag(command, "handle"),
            phone: string_flag(command, "phone"),
            email: string_flag(command, "email"),
            otp: string_flag(command, "otp"),
            invite_code: string_flag(command, "invite-code"),
            wait: command
                .flags
                .get("wait")
                .is_some_and(|value| value == "true"),
            verification_timeout: 300,
            poll_interval_seconds: 5.0,
        };
        let manager = self.identity_manager(&resolved);
        let result = if self.globals.dry_run {
            identity::register_plan(&manager, &resolved.did_domain, &params)
                .map_err(identity_exit)?
        } else {
            identity::register(&resolved, &manager, params).map_err(identity_exit)?
        };
        self.render_identity_result("awiki-cli id register", &resolved, result)
    }

    pub fn run_id_list(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let result =
            identity::list_identities(&self.identity_manager(&resolved)).map_err(identity_exit)?;
        self.render_identity_result("awiki-cli id list", &resolved, result)
    }

    pub fn run_id_current(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let result =
            identity::current_identity(&self.identity_manager(&resolved)).map_err(identity_exit)?;
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
        let manager = self.identity_manager(&resolved);
        let result = if self.globals.dry_run {
            identity::use_plan(&command.args[0])
        } else {
            identity::switch_default_identity(&manager, &command.args[0]).map_err(identity_exit)?
        };
        self.render_identity_result("awiki-cli id use", &resolved, result)
    }

    pub fn run_id_status(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let result =
            identity::identity_status(&self.identity_manager(&resolved)).map_err(identity_exit)?;
        self.render_identity_result("awiki-cli id status", &resolved, result)
    }

    pub fn run_id_import_v1(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let name = command.flags.get("name").cloned().unwrap_or_default();
        let import_all = command
            .flags
            .get("all")
            .is_some_and(|value| value == "true");
        if self.globals.dry_run {
            return self.render_identity_result(
                "awiki-cli id import-v1",
                &resolved,
                identity::CommandResult {
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
        let result = identity::import_v1(&self.identity_manager(&resolved), &name, import_all)
            .map_err(identity_exit)?;
        self.render_identity_result("awiki-cli id import-v1", &resolved, result)
    }

    pub fn run_id_bind(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let params = identity::BindParams {
            phone: string_flag(command, "phone"),
            email: string_flag(command, "email"),
            otp: string_flag(command, "otp"),
            wait: command
                .flags
                .get("wait")
                .is_some_and(|value| value == "true"),
            verification_timeout: 300,
            poll_interval_seconds: 5.0,
        };
        let manager = self.identity_manager(&resolved);
        let result = if self.globals.dry_run {
            identity::bind_plan(&params)
        } else {
            identity::bind(&resolved, &manager, params).map_err(identity_exit)?
        };
        self.render_identity_result("awiki-cli id bind", &resolved, result)
    }

    pub fn run_id_refresh_token(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let manager = self.identity_manager(&resolved);
        let result = if self.globals.dry_run {
            identity::refresh_token_plan(&manager, &self.globals.identity)
        } else {
            identity::refresh_token(&resolved, &manager, &self.globals.identity)
                .map_err(identity_exit)?
        };
        self.render_identity_result("awiki-cli id refresh-token", &resolved, result)
    }

    pub fn run_id_profile_set(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let display_name = string_flag(command, "display-name");
        let bio = string_flag(command, "bio");
        let tags = string_flag(command, "tags");
        let markdown = string_flag(command, "markdown");
        let markdown_file = string_flag(command, "markdown-file");
        if !markdown.trim().is_empty() && !markdown_file.trim().is_empty() {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "Use either --markdown or --markdown-file, not both.",
                "Choose one profile body source.",
            ));
        }
        let resolved = self.resolve_config()?;
        if self.globals.dry_run {
            return self.render_identity_result(
                "awiki-cli id profile set",
                &resolved,
                identity::CommandResult {
                    data: json!({
                        "plan": {
                            "action": "update_profile",
                            "display_name": display_name,
                            "bio": bio,
                            "tags": tags,
                            "markdown": markdown,
                            "markdown_file": markdown_file,
                            "remote_calls": ["did.profile.update_me"],
                            "local_writes": ["identity.json", "index.json"],
                        }
                    }),
                    summary: "Dry run: profile update planned".to_string(),
                    warnings: Vec::new(),
                },
            );
        }
        let result = identity::set_profile(
            &resolved,
            &self.identity_manager(&resolved),
            identity::SetProfileParams {
                display_name,
                bio,
                tags_csv: tags,
                markdown,
                markdown_file,
            },
        )
        .map_err(identity_exit)?;
        self.render_identity_result("awiki-cli id profile set", &resolved, result)
    }

    pub fn run_id_profile_get(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let result = identity::get_profile(
            &resolved,
            &self.identity_manager(&resolved),
            identity::GetProfileParams {
                self_profile: command
                    .flags
                    .get("self")
                    .is_some_and(|value| value == "true"),
                handle: string_flag(command, "handle"),
                did: string_flag(command, "did"),
            },
        )
        .map_err(identity_exit)?;
        self.render_identity_result("awiki-cli id profile get", &resolved, result)
    }

    pub fn run_id_resolve(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let result = identity::resolve_identity(
            &resolved,
            identity::ResolveParams {
                handle: string_flag(command, "handle"),
                did: string_flag(command, "did"),
            },
        )
        .map_err(identity_exit)?;
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

        let mut phase = traceutil::start_phase("workspace_upgrade");
        let result = upgrade::upgrade_if_needed(&resolved, crate::buildinfo::VERSION);
        phase.finish();
        result.map_err(|err| internal_anyhow(anyhow::Error::new(err)))?;

        self.resolve_config_untraced().map_err(internal_anyhow)
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
        result: identity::CommandResult,
    ) -> Result<(), ExitError> {
        self.render_success(
            command,
            resolved,
            identity::sanitize_public_value(result.data),
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
        let format = output::normalize_format(&resolved.output_format).unwrap_or(Format::Json);
        let warnings = update_preflight::merge_update_warning(&self.update_warning, warnings);
        let envelope = SuccessEnvelope {
            ok: true,
            command: command.to_string(),
            data,
            warnings,
            summary: summary.to_string(),
            notice: None,
            meta: Meta {
                version: crate::buildinfo::VERSION.to_string(),
                identity: identity_meta_from_resolved(resolved),
                dry_run: self.globals.dry_run,
                format: format.as_str().to_string(),
            },
        };
        output::render_success(io::stdout(), format, &self.globals.jq, &envelope)
            .map_err(internal_anyhow)
    }

    fn handle_error(&self, err: ExitError) -> i32 {
        let format = output::normalize_format(&self.globals.format).unwrap_or(Format::Json);
        let envelope = ErrorEnvelope {
            ok: false,
            error: err.detail.clone(),
            notice: None,
            meta: Meta {
                version: crate::buildinfo::VERSION.to_string(),
                identity: None,
                dry_run: self.globals.dry_run,
                format: format.as_str().to_string(),
            },
        };
        if output::render_error(io::stderr(), format, &self.globals.jq, &envelope).is_err() {
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
    upgrade::inspect(resolved, crate::buildinfo::VERSION)
        .map(|inspection| serde_json::to_value(inspection).unwrap_or_else(|_| json!({})))
        .unwrap_or_else(|err| {
            let paths = upgrade::resolve_paths(resolved);
            json!({
                "paths": paths,
                "meta": Value::Null,
                "journal": Value::Null,
                "detection": upgrade::detect(resolved, None),
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

fn ensure_sqlite_schema(resolved: &Resolved) -> anyhow::Result<()> {
    let db = store::open(&resolved.paths)?;
    store::ensure_schema(&db)?;
    Ok(())
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

fn not_implemented_side_effect(command: &str) -> ExitError {
    ExitError::new(
        "not_implemented",
        1,
        format!("{command} requires non-dry-run implementation in a later port slice."),
        "Use --dry-run for this first Rust parity slice.",
    )
}

fn identity_exit(err: IdentityError) -> ExitError {
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
        IdentityError::Service(err) => identity_service_exit(err),
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

fn identity_service_exit(err: identity::wire::ServiceError) -> ExitError {
    let message = err.to_string();
    match err {
        identity::wire::ServiceError {
            status_code: 400, ..
        } => ExitError::new(
            "invalid_argument",
            2,
            message,
            "Ensure the handle, verification method, and local alias are valid.",
        ),
        identity::wire::ServiceError {
            status_code: 401, ..
        } => ExitError::new(
            "auth_required",
            3,
            message,
            "Use an identity with valid DID key material, or run `awiki-cli id refresh-token` / `awiki-cli id register` / `awiki-cli id recover` first.",
        ),
        identity::wire::ServiceError {
            status_code: 404, ..
        } => ExitError::new(
            "not_found",
            5,
            message,
            "Ensure the handle, verification method, and local alias are valid.",
        ),
        identity::wire::ServiceError {
            status_code: 409, ..
        } => ExitError::new(
            "conflict",
            1,
            message,
            "Ensure the handle, verification method, and local alias are valid.",
        ),
        identity::wire::ServiceError { rpc_code, .. } if rpc_code != 0 => match rpc_code {
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
    let hint = error_hints::refine_workspace_write_hint(
        &err,
        "Run `awiki-cli doctor` to inspect the local workspace state.",
    );
    ExitError::new("internal_error", 1, err.to_string(), hint)
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
        .map(|summary| (summary.identity_name, summary.did, summary.is_default));
    store::LegacyOwnerLookup::from_entries(entries)
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
