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

    pub fn run_runtime_host_notify_enable(&self) -> Result<(), ExitError> {
        self.run_runtime_host_notify_enable_toggle(true, "awiki-cli runtime host-notify enable")
    }

    pub fn run_runtime_host_notify_disable(&self) -> Result<(), ExitError> {
        self.run_runtime_host_notify_enable_toggle(false, "awiki-cli runtime host-notify disable")
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

    fn run_runtime_host_notify_enable_toggle(
        &self,
        enabled: bool,
        command_name: &str,
    ) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        if self.globals.dry_run {
            return self.render_success(
                command_name,
                &resolved,
                json!({ "plan": { "action": "host_notify_enable_toggle", "enabled": enabled, "config_file": resolved.paths.config_file } }),
                "Dry run: host notify enablement change planned",
                Vec::new(),
            );
        }
        config::update_host_notify_enabled(&resolved.paths, enabled).map_err(internal_anyhow)?;
        let resolved = self.resolve_config()?;
        let host_notify = runtime::host_notify_config_view(&resolved).map_err(internal_anyhow)?;
        let summary = if enabled {
            "Host notify enabled"
        } else {
            "Host notify disabled"
        };
        self.render_success(
            command_name,
            &resolved,
            json!({
                "host_notify": host_notify,
                "listener": runtime::current_listener_status(&resolved),
            }),
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
        let host_notify = runtime::host_notify_config_view(&resolved).map_err(internal_anyhow)?;
        self.render_success(
            "awiki-cli runtime host-notify config show",
            &resolved,
            json!({ "host_notify": host_notify }),
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
        let host_notify = runtime::host_notify_config_view(&resolved).map_err(internal_anyhow)?;
        self.render_success(
            "awiki-cli runtime host-notify config set",
            &resolved,
            json!({
                "host_notify": host_notify,
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
            .map_err(internal_anyhow)?
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

    pub fn run_runtime_host_notify_openclaw_route_add(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let route = resolve_openclaw_route_from_flags(command)?;
        let route_registry_path = runtime::openclaw_routes::routes_path(&resolved.paths);
        if self.globals.dry_run {
            return self.render_success(
                "awiki-cli runtime host-notify openclaw route add",
                &resolved,
                json!({
                    "plan": {
                        "action": "host_notify_openclaw_route_add",
                        "route": route,
                        "route_registry_path": route_registry_path,
                    }
                }),
                "Dry run: OpenClaw route add planned",
                Vec::new(),
            );
        }

        let (route, added, routes) =
            runtime::openclaw_routes::add_route(&resolved.paths, route).map_err(internal_anyhow)?;
        let mut warnings = Vec::new();
        let mut data = json!({
            "route": route,
            "routes": routes,
            "route_registry_path": route_registry_path,
        });
        if added {
            match runtime::openclaw_webhook::send_route_confirmation(&resolved, &route) {
                Ok(confirmation) => {
                    if let Some(object) = data.as_object_mut() {
                        object.insert("confirmation".to_string(), confirmation);
                    }
                }
                Err(warning) => warnings.push(warning),
            }
        }
        let summary = if added {
            "OpenClaw route added"
        } else {
            "OpenClaw route already exists"
        };
        self.render_success(
            "awiki-cli runtime host-notify openclaw route add",
            &resolved,
            data,
            summary,
            warnings,
        )
    }

    pub fn run_runtime_host_notify_openclaw_route_list(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let routes =
            runtime::openclaw_routes::load_routes(&resolved.paths).map_err(internal_anyhow)?;
        self.render_success(
            "awiki-cli runtime host-notify openclaw route list",
            &resolved,
            json!({ "routes": routes }),
            "OpenClaw routes loaded",
            Vec::new(),
        )
    }

    pub fn run_runtime_host_notify_openclaw_route_remove(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let route = resolve_openclaw_route_from_flags(command)?;
        let route_registry_path = runtime::openclaw_routes::routes_path(&resolved.paths);
        if self.globals.dry_run {
            return self.render_success(
                "awiki-cli runtime host-notify openclaw route remove",
                &resolved,
                json!({
                    "plan": {
                        "action": "host_notify_openclaw_route_remove",
                        "route": route,
                        "route_registry_path": route_registry_path,
                    }
                }),
                "Dry run: OpenClaw route remove planned",
                Vec::new(),
            );
        }

        let (route, removed, routes) =
            runtime::openclaw_routes::remove_route(&resolved.paths, route)
                .map_err(internal_anyhow)?;
        let summary = if removed {
            "OpenClaw route removed"
        } else {
            "OpenClaw route not found"
        };
        self.render_success(
            "awiki-cli runtime host-notify openclaw route remove",
            &resolved,
            json!({
                "route": route,
                "routes": routes,
                "route_registry_path": route_registry_path,
            }),
            summary,
            Vec::new(),
        )
    }

    pub fn run_runtime_host_notify_hermes_guide(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let deliver = resolve_hermes_deliver_target(
            &resolved,
            command
                .flags
                .get("deliver")
                .map(String::as_str)
                .unwrap_or_default(),
        );
        if !runtime::hermes_bridge::is_supported_deliver_target(&deliver) {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                format!("unsupported Hermes deliver target {deliver:?}"),
                format!(
                    "Use --deliver with one of: {}.",
                    runtime::hermes_bridge::supported_deliver_targets().join(", ")
                ),
            ));
        }
        let notify_url = resolve_hermes_notify_url(&resolved);
        let (_, secret_source) =
            runtime::hermes_host_notify::resolve_hermes_notify_secret_with_source(
                Some(&resolved),
                &notify_url,
            );
        let mut data = json!({
            "hermes_guide": build_hermes_host_notify_guide_view(
                &resolved,
                &notify_url,
                &deliver,
                &secret_source,
            )
        });
        if let Ok(home) = runtime::hermes_bridge::resolve_hermes_home() {
            if let Ok(route_state) = runtime::hermes_bridge::inspect_route(
                &home,
                runtime::hermes_bridge::DEFAULT_WEBHOOK_ROUTE_NAME,
            ) {
                if let Some(object) = data.as_object_mut() {
                    object.insert("local_hermes".to_string(), json!(route_state));
                }
            }
        }
        let mut warnings = host_notify_guidance_warnings_for(&resolved, &deliver);
        let current_host_notify = runtime::resolve(&resolved).host_notify;
        if current_host_notify.sink != "hermes" {
            warnings.push(format!(
                "Current host notify sink is {:?}. Run `awiki-cli runtime host-notify hermes setup` to switch awiki-cli over to the fully managed local Hermes flow.",
                current_host_notify.sink
            ));
        }
        if secret_source == "unset" {
            warnings.push("awiki-cli does not have a Hermes notify secret yet. `awiki-cli runtime host-notify hermes setup` will generate and persist one automatically.".to_string());
        }
        if deliver == "log" {
            warnings.push("This guide is using `deliver: \"log\"` for probe-only verification. Switch to a real messaging platform such as `feishu` or `telegram` for end-user delivery.".to_string());
        }
        self.render_success(
            "awiki-cli runtime host-notify hermes guide",
            &resolved,
            data,
            "Hermes host notify guide generated",
            warnings,
        )
    }

    pub fn run_runtime_host_notify_hermes_status(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let host_notify_view =
            runtime::host_notify_config_view(&resolved).map_err(internal_anyhow)?;
        let home = runtime::hermes_bridge::resolve_hermes_home().unwrap_or_else(|_| {
            std::path::Path::new("")
                .join(".hermes")
                .to_string_lossy()
                .into_owned()
        });
        let route_result = runtime::hermes_bridge::inspect_route(
            &home,
            runtime::hermes_bridge::DEFAULT_WEBHOOK_ROUTE_NAME,
        );
        let bridge_status = runtime::hermes_bridge::status_for(&resolved);
        let expected_deliver = resolve_hermes_deliver_target(&resolved, "");
        let host_notify = runtime::resolve(&resolved).host_notify;
        let (_, secret_source) =
            runtime::hermes_host_notify::resolve_hermes_notify_secret_with_source(
                Some(&resolved),
                &resolve_hermes_notify_url(&resolved),
            );
        let route_state = route_result.as_ref().ok();
        let readiness = json!({
            "awiki_sink_is_hermes": host_notify.sink == "hermes",
            "awiki_host_notify_enabled": host_notify.enabled,
            "awiki_secret_configured": secret_source != "unset",
            "hermes_route_configured": route_state.is_some_and(|state| state.route_configured),
            "hermes_route_matches_deliver": route_state.is_some_and(|state| state.deliver == expected_deliver),
            "hermes_route_uses_home_channel": route_state.is_some_and(|state| state.deliver_uses_home_channel),
            "home_channel_configured": route_state.is_some_and(|state| expected_deliver == "log" || state.home_channel_configured),
            "bridge_running": bridge_status.running,
            "bridge_available": bridge_status.bridge_available,
        });
        let mut data = json!({
            "host_notify": host_notify_view,
            "readiness": readiness,
        });
        let mut warnings = host_notify_guidance_warnings_for(&resolved, &expected_deliver);
        match route_result {
            Ok(route_state) => {
                warnings.extend(route_state.warnings.clone());
                if let Some(object) = data.as_object_mut() {
                    object.insert("local_hermes".to_string(), json!(route_state));
                }
            }
            Err(err) => warnings.push(format!("Failed to inspect local Hermes config: {err}")),
        }
        warnings.extend(bridge_status.warnings.clone());
        if let Some(object) = data.as_object_mut() {
            object.insert("bridge".to_string(), json!(bridge_status));
        }
        let route_state = data.get("local_hermes");
        let ready = host_notify.sink == "hermes"
            && host_notify.enabled
            && secret_source != "unset"
            && route_state.is_some_and(|state| state["route_configured"].as_bool() == Some(true))
            && route_state.is_some_and(|state| {
                state["deliver"].as_str().unwrap_or_default() == expected_deliver
            })
            && (expected_deliver == "log"
                || route_state.is_some_and(|state| {
                    state["deliver_uses_home_channel"].as_bool() == Some(true)
                        && state["home_channel_configured"].as_bool() == Some(true)
                }))
            && bridge_status.running
            && bridge_status.bridge_available;
        if let Some(object) = data.as_object_mut() {
            object.insert("ready".to_string(), json!(ready));
        }
        let summary = if ready {
            format!(
                "Hermes host notify is ready for awiki -> Hermes -> {} delivery",
                runtime::hermes_bridge::deliver_display_name(&expected_deliver)
            )
        } else {
            "Hermes host notify readiness loaded".to_string()
        };
        self.render_success(
            "awiki-cli runtime host-notify hermes status",
            &resolved,
            data,
            &summary,
            dedupe_strings(warnings),
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

fn resolve_openclaw_route_from_flags(
    command: &ParsedCommand,
) -> Result<runtime::openclaw_routes::Route, ExitError> {
    runtime::openclaw_routes::resolve_route_input(
        command
            .flags
            .get("channel")
            .map(String::as_str)
            .unwrap_or(""),
        command.flags.get("to").map(String::as_str).unwrap_or(""),
        command
            .flags
            .get("session-key")
            .map(String::as_str)
            .unwrap_or(""),
    )
    .map_err(|err| {
        ExitError::new(
            "invalid_argument",
            2,
            err.to_string(),
            "Use --channel and --to, or use --session-key.",
        )
    })
}

fn resolve_hermes_deliver_target(resolved: &Resolved, override_value: &str) -> String {
    let value = override_value.trim();
    if !value.is_empty() {
        return value.to_ascii_lowercase();
    }
    let value = resolved.host_notify_hermes_deliver.trim();
    if !value.is_empty() {
        return value.to_ascii_lowercase();
    }
    runtime::hermes_bridge::normalize_deliver_target("")
}

fn resolve_hermes_notify_url(resolved: &Resolved) -> String {
    let value = resolved.host_notify_hermes_notify_url.trim();
    if !value.is_empty() {
        return value.to_string();
    }
    runtime::hermes_bridge::DEFAULT_NOTIFY_URL.to_string()
}

fn host_notify_guidance_warnings_for(resolved: &Resolved, deliver_override: &str) -> Vec<String> {
    let config = runtime::resolve(resolved).host_notify;
    if config.sink != "hermes" {
        return Vec::new();
    }
    let deliver = resolve_hermes_deliver_target(resolved, deliver_override);
    let home_channel_key = runtime::hermes_bridge::home_channel_env_key(&deliver);
    let target_warning = if deliver != "log" && !home_channel_key.is_empty() {
        format!(
            "For {} delivery, prefer setting {} (or using /sethome in Hermes) instead of hard-coding deliver_extra.chat_id in Hermes routes.",
            runtime::hermes_bridge::deliver_display_name(&deliver),
            home_channel_key
        )
    } else {
        "Prefer the platform home channel (or /sethome in Hermes) instead of hard-coding deliver_extra.chat_id in Hermes routes.".to_string()
    };
    vec![
        "Hermes sink only forwards notifications to the Hermes adapter. Final delivery targets are configured in Hermes, not in awiki-cli.".to_string(),
        target_warning,
        "`awiki-cli runtime host-notify hermes setup` now also updates the local Hermes notify route and starts the local bridge automatically.".to_string(),
    ]
}

fn build_hermes_host_notify_guide_view(
    resolved: &Resolved,
    notify_url: &str,
    deliver: &str,
    secret_source: &str,
) -> Value {
    let current_host_notify = runtime::resolve(resolved).host_notify;
    let current_deliver = resolve_hermes_deliver_target(resolved, "");
    let home_channel_key = runtime::hermes_bridge::home_channel_env_key(deliver);
    let mut targeting = Vec::new();
    if !home_channel_key.is_empty() {
        targeting.push(format!(
            "Prefer {home_channel_key} for the default delivery target."
        ));
    }
    if deliver != "log" {
        targeting.push(format!(
            "Or send /sethome or /set-home to Hermes from the desired {} chat.",
            runtime::hermes_bridge::deliver_display_name(deliver)
        ));
    }
    targeting.push(
        "Avoid hard-coding deliver_extra.chat_id unless you explicitly want a fixed destination."
            .to_string(),
    );
    let route_yaml = format!(
        "platforms:\n  webhook:\n    enabled: true\n    extra:\n      port: 8644\n      secret: \"${{HERMES_WEBHOOK_SECRET}}\"\n      routes:\n        notify:\n          secret: \"${{HERMES_ROUTE_SECRET}}\"\n          events: []\n          prompt: \"{{notify_payload}}\"\n          skills: [\"notify\"]\n          deliver: {deliver:?}\n"
    );
    let adapter_command = "python3 scripts/hermes_notify_adapter.py \\\n  --host 0.0.0.0 \\\n  --port 8765 \\\n  --notify-secret \"<NOTIFY_SECRET>\" \\\n  --hermes-webhook-url \"http://127.0.0.1:8644/webhooks/notify\" \\\n  --hermes-route-secret \"<HERMES_ROUTE_SECRET>\" \\\n  --log-level INFO";
    let mut setup_command = "awiki-cli runtime host-notify hermes setup".to_string();
    if deliver != current_deliver {
        setup_command.push_str(" --deliver ");
        setup_command.push_str(deliver);
    }
    json!({
        "delivery_model": "awiki-cli only forwards host notify events to the Hermes adapter. Final delivery targets are configured in Hermes.",
        "awiki_cli": {
            "current": {
                "enabled": current_host_notify.enabled,
                "sink": current_host_notify.sink,
                "notify_url": notify_url,
                "deliver": current_deliver,
                "secret_configured": secret_source != "unset",
                "secret_source": secret_source,
            },
            "recommended_setup_command": setup_command,
            "verify_commands": [
                "awiki-cli runtime host-notify config show",
                "awiki-cli runtime host-notify hermes status",
            ],
        },
        "hermes": {
            "notify_route_name": "notify",
            "webhook_port": 8644,
            "webhook_secret_env": "HERMES_WEBHOOK_SECRET",
            "route_secret_env": "HERMES_ROUTE_SECRET",
            "recommended_route": route_yaml,
            "adapter_notify_url": "http://127.0.0.1:8765/notify/host-event",
            "adapter_healthcheck": "curl -sS http://127.0.0.1:8765/healthz",
            "adapter_run_command": adapter_command,
            "deliver_target": deliver,
            "awiki_expected_url": notify_url,
            "managed_by_setup": "awiki-cli runtime host-notify hermes setup will write the local Hermes notify route and restart the local bridge for you.",
            "targeting": targeting,
        }
    })
}

fn dedupe_strings(values: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut result = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() || !seen.insert(trimmed.to_string()) {
            continue;
        }
        result.push(trimmed.to_string());
    }
    result
}
