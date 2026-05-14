use super::{ensure_sqlite_schema, init_dirs, internal_anyhow, internal_io, App};
use crate::cli::ParsedCommand;
use crate::config::{self, Resolved};
use crate::output::ExitError;
use crate::runtime;
use serde_json::{json, Value};
use std::fs;

impl App {
    pub fn run_runtime_status(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        self.render_success(
            "awiki-cli runtime status",
            &resolved,
            json!({
                "runtime": runtime::runtime_value(&resolved),
                "listener": runtime::current_listener_status(&resolved),
            }),
            "Runtime status loaded",
            Vec::new(),
        )
    }

    pub fn run_runtime_apply(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        if self.globals.dry_run {
            return self.render_success(
                "awiki-cli runtime apply",
                &resolved,
                json!({
                    "plan": {
                        "action": "runtime_apply",
                        "runtime": runtime::runtime_value(&resolved),
                        "listener": runtime::current_listener_status(&resolved),
                    }
                }),
                "Dry run: runtime apply planned",
                Vec::new(),
            );
        }
        ensure_sqlite_schema(&resolved).map_err(internal_anyhow)?;
        let listener = runtime::apply_runtime_policy(&resolved).map_err(internal_anyhow)?;
        self.render_success(
            "awiki-cli runtime apply",
            &resolved,
            json!({ "runtime": runtime::runtime_value(&resolved), "listener": listener }),
            "Runtime state applied",
            Vec::new(),
        )
    }

    pub fn run_runtime_setup(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let mode = command
            .flags
            .get("mode")
            .cloned()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| resolved.runtime_mode.clone());
        validate_runtime_mode_for_setup(&mode)?;
        if self.globals.dry_run {
            return self.render_success(
                "awiki-cli runtime setup",
                &resolved,
                json!({
                    "plan": {
                        "action": "runtime_setup",
                        "mode": mode,
                        "workspace_home": resolved.paths.workspace_home_dir,
                        "runtime_dir": resolved.paths.state_dir,
                        "database_file": resolved.paths.database_file,
                        "writes": [resolved.paths.config_file, resolved.paths.database_file],
                    }
                }),
                "Dry run: runtime setup planned",
                Vec::new(),
            );
        }
        config::update_runtime_settings(&resolved.paths, &mode, &resolved.runtime_socket_path)
            .map_err(internal_anyhow)?;
        let resolved = self.resolve_config()?;
        for dir in init_dirs(&resolved) {
            fs::create_dir_all(dir).map_err(internal_io)?;
        }
        ensure_sqlite_schema(&resolved).map_err(internal_anyhow)?;
        let listener = runtime::apply_runtime_policy(&resolved).map_err(internal_anyhow)?;
        self.render_success(
            "awiki-cli runtime setup",
            &resolved,
            json!({ "action": "runtime_setup", "mode": mode, "paths": resolved.paths, "listener": listener }),
            "Runtime setup completed",
            Vec::new(),
        )
    }

    pub fn run_runtime_mode_get(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        self.render_success(
            "awiki-cli runtime mode get",
            &resolved,
            json!({ "runtime": runtime::runtime_value(&resolved) }),
            "Runtime mode loaded",
            Vec::new(),
        )
    }

    pub fn run_runtime_mode_set(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        if command.args.len() != 1 {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "runtime mode set requires exactly one mode.",
                "Usage: awiki-cli runtime mode set <http|websocket>",
            ));
        }
        let mode = command.args[0].trim().to_ascii_lowercase();
        validate_runtime_mode_for_set(&mode)?;
        let resolved = self.resolve_config()?;
        if self.globals.dry_run {
            return self.render_success(
                "awiki-cli runtime mode set",
                &resolved,
                json!({ "plan": { "action": "runtime_mode_set", "mode": mode, "config_file": resolved.paths.config_file } }),
                "Dry run: runtime mode change planned",
                Vec::new(),
            );
        }
        config::update_runtime_settings(&resolved.paths, &mode, &resolved.runtime_socket_path)
            .map_err(internal_anyhow)?;
        let resolved = self.resolve_config()?;
        ensure_sqlite_schema(&resolved).map_err(internal_anyhow)?;
        let listener = runtime::apply_runtime_policy(&resolved).map_err(internal_anyhow)?;
        self.render_success(
            "awiki-cli runtime mode set",
            &resolved,
            json!({ "action": "runtime_mode_set", "mode": mode, "listener": listener }),
            &format!("Runtime mode set to {mode}"),
            Vec::new(),
        )
    }

    pub fn run_runtime_listener_status(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        self.render_success(
            "awiki-cli runtime listener status",
            &resolved,
            json!({ "listener": runtime::current_listener_status(&resolved) }),
            "Listener status loaded",
            Vec::new(),
        )
    }

    pub fn run_runtime_listener_config_show(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        self.render_success(
            "awiki-cli runtime listener config show",
            &resolved,
            json!({ "listener": listener_config_snapshot(&resolved) }),
            "Listener config loaded",
            Vec::new(),
        )
    }

    pub fn run_runtime_listener_config_set(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let enabled = parse_optional_bool(command, "enabled")?;
        let auto_install = parse_optional_bool(command, "auto-install")?;
        let auto_start = parse_optional_bool(command, "auto-start")?;
        if enabled.is_none() && auto_install.is_none() && auto_start.is_none() {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "listener config set requires at least one changed flag.",
                "Use --enabled, --auto-install, or --auto-start.",
            ));
        }
        if self.globals.dry_run {
            return self.render_success(
                "awiki-cli runtime listener config set",
                &resolved,
                json!({ "plan": { "action": "listener_config_set", "enabled": enabled, "auto_install": auto_install, "auto_start": auto_start, "config_file": resolved.paths.config_file } }),
                "Dry run: listener config change planned",
                Vec::new(),
            );
        }
        config::update_runtime_listener_settings(
            &resolved.paths,
            enabled,
            auto_install,
            auto_start,
        )
        .map_err(internal_anyhow)?;
        let resolved = self.resolve_config()?;
        self.render_success(
            "awiki-cli runtime listener config set",
            &resolved,
            json!({ "listener": listener_config_snapshot(&resolved) }),
            "Listener config updated",
            Vec::new(),
        )
    }

    pub fn run_runtime_listener_enable(&self) -> Result<(), ExitError> {
        self.run_runtime_listener_enable_toggle(true, "awiki-cli runtime listener enable")
    }

    pub fn run_runtime_listener_disable(&self) -> Result<(), ExitError> {
        self.run_runtime_listener_enable_toggle(false, "awiki-cli runtime listener disable")
    }

    fn run_runtime_listener_enable_toggle(
        &self,
        enabled: bool,
        command_name: &str,
    ) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        if self.globals.dry_run {
            return self.render_success(
                command_name,
                &resolved,
                json!({ "plan": { "action": "listener_enable_toggle", "enabled": enabled, "config_file": resolved.paths.config_file } }),
                "Dry run: listener enablement change planned",
                Vec::new(),
            );
        }
        config::update_runtime_listener_settings(&resolved.paths, Some(enabled), None, None)
            .map_err(internal_anyhow)?;
        let resolved = self.resolve_config()?;
        let listener = runtime::apply_runtime_policy(&resolved).map_err(internal_anyhow)?;
        let summary = if enabled {
            "Listener enabled and runtime applied"
        } else {
            "Listener disabled and runtime applied"
        };
        self.render_success(
            command_name,
            &resolved,
            json!({ "listener": listener }),
            summary,
            Vec::new(),
        )
    }

    pub fn run_runtime_listener_install(&self) -> Result<(), ExitError> {
        self.run_runtime_listener_lifecycle(
            "awiki-cli runtime listener install",
            "listener_install",
            "Dry run: listener install planned",
            "Listener service installed",
            runtime::install_listener,
        )
    }

    pub fn run_runtime_listener_start(&self) -> Result<(), ExitError> {
        self.run_runtime_listener_lifecycle(
            "awiki-cli runtime listener start",
            "listener_start",
            "Dry run: listener start planned",
            "Listener started",
            runtime::start_listener,
        )
    }

    pub fn run_runtime_listener_stop(&self) -> Result<(), ExitError> {
        self.run_runtime_listener_lifecycle(
            "awiki-cli runtime listener stop",
            "listener_stop",
            "Dry run: listener stop planned",
            "Listener stopped",
            runtime::stop_listener,
        )
    }

    pub fn run_runtime_listener_restart(&self) -> Result<(), ExitError> {
        self.run_runtime_listener_lifecycle(
            "awiki-cli runtime listener restart",
            "listener_restart",
            "Dry run: listener restart planned",
            "Listener restarted",
            runtime::start_listener,
        )
    }

    pub fn run_runtime_listener_uninstall(&self) -> Result<(), ExitError> {
        self.run_runtime_listener_lifecycle(
            "awiki-cli runtime listener uninstall",
            "listener_uninstall",
            "Dry run: listener uninstall planned",
            "Listener service uninstalled",
            runtime::uninstall_listener,
        )
    }

    fn run_runtime_listener_lifecycle(
        &self,
        command_name: &str,
        action: &str,
        dry_summary: &str,
        summary: &str,
        execute: fn(&Resolved) -> anyhow::Result<Value>,
    ) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        if self.globals.dry_run {
            return self.render_success(
                command_name,
                &resolved,
                json!({ "plan": { "action": action, "mode": resolved.runtime_mode, "socket_path": resolved.runtime_socket_path } }),
                dry_summary,
                Vec::new(),
            );
        }
        let listener = execute(&resolved).map_err(internal_anyhow)?;
        self.render_success(
            command_name,
            &resolved,
            json!({ "listener": listener }),
            summary,
            Vec::new(),
        )
    }

    pub fn run_runtime_host_notify_config_show(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        self.render_success(
            "awiki-cli runtime host-notify config show",
            &resolved,
            json!({ "host_notify": runtime::host_notify_config_view(&resolved) }),
            "Host notify config loaded",
            Vec::new(),
        )
    }

    pub fn run_runtime_host_notify_config_set(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let Some(sink) = command
            .flags
            .get("sink")
            .filter(|value| !value.trim().is_empty())
        else {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "host-notify config set requires --sink.",
                "Use --sink noop|log|file|openclaw|hermes.",
            ));
        };
        let normalized_sink = config::normalize_host_notify_sink_for_write(sink).map_err(|_| {
            ExitError::new(
                "invalid_argument",
                2,
                "unsupported host notify sink",
                "Use --sink noop, log, file, openclaw, or hermes.",
            )
        })?;
        if self.globals.dry_run {
            return self.render_success(
                "awiki-cli runtime host-notify config set",
                &resolved,
                json!({ "plan": { "action": "host_notify_config_set", "sink": sink, "config_file": resolved.paths.config_file } }),
                "Dry run: host notify config change planned",
                Vec::new(),
            );
        }
        config::update_host_notify_sink(&resolved.paths, &normalized_sink)
            .map_err(internal_anyhow)?;
        let resolved = self.resolve_config()?;
        self.render_success(
            "awiki-cli runtime host-notify config set",
            &resolved,
            json!({
                "host_notify": runtime::host_notify_config_view(&resolved),
                "listener": runtime::current_listener_status(&resolved),
            }),
            "Host notify config updated",
            Vec::new(),
        )
    }

    pub fn run_runtime_host_notify_openclaw_set(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let hook_url = command.flags.get("hook-url").map(String::as_str);
        if hook_url.is_none() {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "openclaw set requires at least one changed flag.",
                "Use --hook-url.",
            ));
        }
        let hook_url = hook_url.unwrap_or_default();
        runtime::validate_openclaw_hook_url(hook_url).map_err(|err| {
            ExitError::new(
                "invalid_argument",
                2,
                err.to_string(),
                "Use a loopback OpenClaw hook URL such as http://127.0.0.1:18789/hooks/agent.",
            )
        })?;
        if self.globals.dry_run {
            return self.render_success(
                "awiki-cli runtime host-notify openclaw set",
                &resolved,
                json!({ "plan": { "action": "host_notify_openclaw_set", "hook_url": hook_url, "config_file": resolved.paths.config_file } }),
                "Dry run: OpenClaw host notify config change planned",
                Vec::new(),
            );
        }
        config::update_openclaw_settings(&resolved.paths, Some(hook_url))
            .map_err(internal_anyhow)?;
        let resolved = self.resolve_config()?;
        let openclaw = runtime::host_notify_config_view(&resolved)
            .get("openclaw")
            .cloned()
            .unwrap_or_else(|| json!({}));
        self.render_success(
            "awiki-cli runtime host-notify openclaw set",
            &resolved,
            json!({ "openclaw": openclaw, "listener": runtime::current_listener_status(&resolved) }),
            "OpenClaw host notify config updated",
            Vec::new(),
        )
    }

    pub fn run_runtime_host_notify_openclaw_set_token(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let token = command.flags.get("value").cloned().unwrap_or_default();
        if token.trim().is_empty() {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "openclaw set-token requires --value.",
                "Use --value <token>.",
            ));
        }
        if self.globals.dry_run {
            return self.render_success(
                "awiki-cli runtime host-notify openclaw set-token",
                &resolved,
                json!({ "plan": { "action": "host_notify_openclaw_set_token", "configured": true, "config_file": resolved.paths.config_file } }),
                "Dry run: OpenClaw token update planned",
                Vec::new(),
            );
        }
        config::set_openclaw_token(&resolved.paths, &token).map_err(internal_anyhow)?;
        let resolved = self.resolve_config()?;
        self.render_success(
            "awiki-cli runtime host-notify openclaw set-token",
            &resolved,
            json!({ "openclaw": { "token_configured": true }, "listener": runtime::current_listener_status(&resolved) }),
            "OpenClaw token updated",
            Vec::new(),
        )
    }

    pub fn run_runtime_host_notify_openclaw_clear_token(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        if self.globals.dry_run {
            return self.render_success(
                "awiki-cli runtime host-notify openclaw clear-token",
                &resolved,
                json!({ "plan": { "action": "host_notify_openclaw_clear_token", "config_file": resolved.paths.config_file } }),
                "Dry run: OpenClaw token clear planned",
                Vec::new(),
            );
        }
        config::clear_openclaw_token(&resolved.paths).map_err(internal_anyhow)?;
        let resolved = self.resolve_config()?;
        self.render_success(
            "awiki-cli runtime host-notify openclaw clear-token",
            &resolved,
            json!({ "openclaw": { "token_configured": false }, "listener": runtime::current_listener_status(&resolved) }),
            "OpenClaw token cleared",
            Vec::new(),
        )
    }
}

fn listener_config_snapshot(resolved: &Resolved) -> Value {
    json!({
        "enabled": resolved.runtime_listener_enabled,
        "auto_install": resolved.runtime_listener_auto_install,
        "auto_start": resolved.runtime_listener_auto_start,
    })
}

fn validate_runtime_mode_for_setup(mode: &str) -> Result<(), ExitError> {
    match mode.trim().to_ascii_lowercase().as_str() {
        "http" | "websocket" => Ok(()),
        _ => Err(ExitError::new(
            "invalid_argument",
            2,
            "runtime setup requires --mode http|websocket.",
            "Use runtime setup --mode websocket or runtime setup --mode http.",
        )),
    }
}

fn validate_runtime_mode_for_set(mode: &str) -> Result<(), ExitError> {
    match mode.trim().to_ascii_lowercase().as_str() {
        "http" | "websocket" => Ok(()),
        _ => Err(ExitError::new(
            "invalid_argument",
            2,
            "unsupported runtime mode",
            "Use runtime mode set http or runtime mode set websocket.",
        )),
    }
}

fn parse_optional_bool(command: &ParsedCommand, name: &str) -> Result<Option<bool>, ExitError> {
    command
        .flags
        .get(name)
        .map(|value| {
            value.parse::<bool>().map_err(|_| {
                ExitError::new(
                    "invalid_argument",
                    2,
                    format!("{name} must be true or false."),
                    "Use boolean values true or false.",
                )
            })
        })
        .transpose()
}
