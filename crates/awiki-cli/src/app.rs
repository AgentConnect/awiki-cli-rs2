use crate::buildinfo::BuildInfo;
use crate::cli::{self, ParsedCommand};
use crate::cmdmeta;
use crate::config::{self, Overrides, Resolved};
use crate::docs;
use crate::identity::{self, IdentityError, Manager};
use crate::output::{self, ErrorEnvelope, ExitError, Format, IdentityMeta, Meta, SuccessEnvelope};
use crate::store::{self, StoreError};
use serde_json::{json, Value};
use std::fs;
use std::io::{self, Write};
use std::path::Path;

mod mail_handlers;
mod runtime_handlers;

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
}

pub fn execute() -> i32 {
    let command = match cli::parse_env() {
        Ok(command) => command,
        Err(err) => return App::default().handle_error(err),
    };
    let mut app = App {
        globals: command.globals.clone(),
    };
    if let Err(err) = app.preflight() {
        return app.handle_error(err);
    }
    match cli::dispatch(&app, &command) {
        Ok(()) => 0,
        Err(err) => app.handle_error(err),
    }
}

impl App {
    fn preflight(&mut self) -> Result<(), ExitError> {
        output::normalize_format(&self.globals.format).map_err(|err| {
            ExitError::new(
                "invalid_argument",
                2,
                err.to_string(),
                "Use --format json, pretty, ndjson, or table.",
            )
        })?;
        Ok(())
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

    pub fn run_doctor(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let checks = doctor_checks(&resolved);
        let counts = count_checks(&checks);
        let summary = if counts["error"].as_i64().unwrap_or_default() > 0 {
            "Doctor found blocking issues"
        } else if counts["warn"].as_i64().unwrap_or_default() > 0 {
            "Doctor found warnings"
        } else {
            "Doctor completed successfully"
        };
        self.render_success(
            "awiki-cli doctor",
            &resolved,
            json!({ "checks": checks, "summary": summary, "counts": counts }),
            summary,
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
        let resolved = self.resolve_config()?;
        let name = command.flags.get("name").cloned().unwrap_or_default();
        if name.trim().is_empty() {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "id create requires --name.",
                "Usage: awiki-cli id create --name \"Alice\" [--identity alice]",
            ));
        }
        let identity_name = command
            .flags
            .get("identity")
            .filter(|value| !value.trim().is_empty())
            .cloned()
            .unwrap_or_default();
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

    pub fn run_id_list(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let result =
            identity::list_identities(&self.identity_manager(&resolved)).map_err(identity_exit)?;
        self.render_identity_result("awiki-cli id list", &resolved, result)
    }

    pub fn run_id_current(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let result =
            identity::current_identity(&self.identity_manager(&resolved)).map_err(identity_exit)?;
        self.render_identity_result("awiki-cli id current", &resolved, result)
    }

    pub fn run_id_use(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        if command.args.len() != 1 {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "id use requires exactly one identity name.",
                "Usage: awiki-cli id use <identity>",
            ));
        }
        let manager = self.identity_manager(&resolved);
        let result = if self.globals.dry_run {
            identity::use_plan(&command.args[0])
        } else {
            identity::switch_default_identity(&manager, &command.args[0]).map_err(identity_exit)?
        };
        self.render_identity_result("awiki-cli id use", &resolved, result)
    }

    pub fn run_id_status(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let result =
            identity::identity_status(&self.identity_manager(&resolved)).map_err(identity_exit)?;
        self.render_identity_result("awiki-cli id status", &resolved, result)
    }

    pub fn run_id_import_v1(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
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

    pub fn run_id_refresh_token(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        if !self.globals.dry_run {
            return Err(not_implemented_side_effect("id refresh-token"));
        }
        let result =
            identity::refresh_token_plan(&self.identity_manager(&resolved), &self.globals.identity);
        self.render_identity_result("awiki-cli id refresh-token", &resolved, result)
    }

    pub fn run_msg_send(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let to = command.flags.get("to").cloned().unwrap_or_default();
        let group = command.flags.get("group").cloned().unwrap_or_default();
        if to.trim().is_empty() && group.trim().is_empty() {
            return Err(ExitError::new("invalid_argument", 2, "msg send requires either --to or --group.", "Usage: awiki-cli msg send --to <handle|did> --text \"Hello\" or awiki-cli msg send --group <group_did> --text \"Hello group\""));
        }
        if !self.globals.dry_run {
            return Err(not_implemented_side_effect("msg send"));
        }
        let action = if group.trim().is_empty() {
            "direct.send"
        } else {
            "group.send"
        };
        let target = if group.trim().is_empty() {
            json!({ "did": to, "kind": "direct" })
        } else {
            json!({ "did": group, "kind": "group" })
        };
        self.render_success(
            "awiki-cli msg send",
            &resolved,
            json!({
                "plan": {
                    "action": action,
                    "identity": self.globals.identity,
                    "target": target,
                    "message_type": command.flags.get("type").cloned().unwrap_or_else(|| "text".to_string()),
                    "runtime_mode": resolved.runtime_mode,
                    "transport": resolved.runtime_mode,
                    "local_writes": ["messages"],
                }
            }),
            "Dry run: message send planned",
            Vec::new(),
        )
    }

    pub fn run_page_create(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        if !self.globals.dry_run {
            return Err(not_implemented_side_effect("page create"));
        }
        self.render_success(
            "awiki-cli page create",
            &resolved,
            json!({
                "plan": {
                        "action": "page.create",
                        "identity": self.globals.identity,
                        "rpc_endpoint": "/content/rpc",
                        "rpc_method": "create",
                        "request": {
                        "slug": command.flags.get("slug").cloned().unwrap_or_default(),
                        "title": command.flags.get("title").cloned().unwrap_or_default(),
                        "body_bytes": command.flags.get("markdown").map(|v| v.len()).unwrap_or_default(),
                        "visibility": command.flags.get("visibility").cloned().unwrap_or_else(|| "public".to_string()),
                    }
                }
            }),
            "Dry run: page create planned",
            Vec::new(),
        )
    }

    pub fn run_debug_db_query(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        if command.args.len() != 1 {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "debug db query requires exactly one SQL statement.",
                "Usage: awiki-cli debug db query \"SELECT * FROM messages LIMIT 5\"",
            ));
        }
        let resolved = self.resolve_config()?;
        let db = self.open_store(
            &resolved,
            "Run `awiki-cli doctor` to inspect the database path and configuration.",
        )?;
        store::ensure_schema(&db)
            .map_err(|err| store_exit(err, "Initialize the local store before querying it."))?;
        let rows = store::execute_sql(&db, &command.args[0]).map_err(|err| {
            store_exit(
                err,
                "Only single-statement safe SQL is allowed. Avoid destructive statements.",
            )
        })?;
        self.render_success(
            "awiki-cli debug db query",
            &resolved,
            json!({ "database_file": resolved.paths.database_file, "rows": rows }),
            "SQLite query executed",
            Vec::new(),
        )
    }

    pub fn run_debug_db_import_v1(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let mut db = self.open_store(
            &resolved,
            "Run `awiki-cli doctor` to inspect the database path and configuration.",
        )?;
        store::ensure_schema(&db).map_err(|err| {
            store_exit(
                err,
                "Initialize the local store before importing legacy data.",
            )
        })?;
        let mut paths = resolved.paths.clone();
        if let Some(path) = command
            .flags
            .get("path")
            .filter(|value| !value.trim().is_empty())
        {
            paths.legacy_data_dir = path.trim().to_string();
        }
        if self.globals.dry_run {
            let scan = store::scan_legacy_database(&paths)
                .map_err(|err| store_exit(err, "Make sure the legacy database path is correct."))?;
            return self.render_success(
                "awiki-cli debug db import-v1",
                &resolved,
                json!({
                    "plan": {
                        "action": "import_v1_sqlite",
                        "source_scan": scan,
                        "target": resolved.paths.database_file,
                    }
                }),
                "Dry run: legacy SQLite import planned",
                Vec::new(),
            );
        }
        let report = store::import_legacy_database(&mut db, &paths).map_err(|err| {
            store_exit(
                err,
                "Make sure the v1 database exists and identities were imported first.",
            )
        })?;
        let warnings = report.warnings.clone();
        self.render_success(
            "awiki-cli debug db import-v1",
            &resolved,
            json!({ "database_file": resolved.paths.database_file, "import_report": report }),
            "Legacy SQLite import completed",
            warnings,
        )
    }

    fn resolve_config(&self) -> Result<Resolved, ExitError> {
        config::resolve(Overrides {
            identity: self.globals.identity.clone(),
            identity_changed: self.globals.identity_changed,
            format: self.globals.format.clone(),
            format_changed: self.globals.format_changed,
        })
        .map_err(internal_anyhow)
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
    json!({
        "current_version": crate::buildinfo::VERSION,
        "workspace": resolved.paths.workspace_home_dir,
        "detection": { "has_workspace": Path::new(&resolved.paths.workspace_home_dir).exists(), "has_legacy": false },
        "actions": [],
        "warnings": [],
    })
}

fn doctor_checks(resolved: &Resolved) -> Vec<Value> {
    vec![
        json!({ "name": "build", "status": "ok", "summary": "Build information is available", "details": BuildInfo::current() }),
        json!({ "name": "config_file", "status": if resolved.config_error.is_empty() { "ok" } else { "error" }, "summary": "config.yaml inspected", "details": { "exists": resolved.config_exists, "path": resolved.paths.config_file, "error": resolved.config_error } }),
        json!({ "name": "env", "status": "ok", "summary": "Environment compatibility checked", "details": { "env_hits": resolved.env_hits } }),
        json!({ "name": "sqlite", "status": "ok", "summary": "SQLite path resolved", "details": { "database_file": resolved.paths.database_file } }),
    ]
}

fn count_checks(checks: &[Value]) -> Value {
    let mut ok = 0;
    let mut warn = 0;
    let mut error = 0;
    let mut info = 0;
    for check in checks {
        match check
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("info")
        {
            "ok" => ok += 1,
            "warn" => warn += 1,
            "error" => error += 1,
            _ => info += 1,
        }
    }
    json!({ "ok": ok, "warn": warn, "error": error, "info": info })
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

fn internal_io(err: std::io::Error) -> ExitError {
    ExitError::new(
        "internal_error",
        1,
        err.to_string(),
        "Check directory permissions for the awiki-cli workspace.",
    )
}

fn internal_anyhow(err: anyhow::Error) -> ExitError {
    ExitError::new(
        "internal_error",
        1,
        err.to_string(),
        "Run `awiki-cli doctor` to inspect the local workspace state.",
    )
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
