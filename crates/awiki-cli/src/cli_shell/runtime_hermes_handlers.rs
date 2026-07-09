use super::{
    internal_anyhow, runtime_host_notify_refresh::refresh_listener_for_host_notify_change, App,
};
use crate::cli_output::ExitError;
use crate::cli_parser::ParsedCommand;
use crate::host_runtime;
use crate::workspace_config::{self, Resolved};
use rand::RngCore;
use serde_json::{json, Value};
use std::path::Path;

impl App {
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
        if !host_runtime::hermes_bridge::is_supported_deliver_target(&deliver) {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                format!("unsupported Hermes deliver target {deliver:?}"),
                format!(
                    "Use --deliver with one of: {}.",
                    host_runtime::hermes_bridge::supported_deliver_targets().join(", ")
                ),
            ));
        }
        let notify_url = resolve_hermes_notify_url(&resolved);
        let (_, secret_source) =
            host_runtime::hermes_host_notify::resolve_hermes_notify_secret_with_source(
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
        if let Ok(home) = host_runtime::hermes_bridge::resolve_hermes_home() {
            if let Ok(route_state) = host_runtime::hermes_bridge::inspect_route(
                &home,
                host_runtime::hermes_bridge::DEFAULT_WEBHOOK_ROUTE_NAME,
            ) {
                if let Some(object) = data.as_object_mut() {
                    object.insert("local_hermes".to_string(), json!(route_state));
                }
            }
        }
        let mut warnings = host_notify_guidance_warnings_for(&resolved, &deliver);
        let current_host_notify = host_runtime::resolve(&resolved).host_notify;
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
            host_runtime::host_notify_config_view(&resolved).map_err(internal_anyhow)?;
        let home = host_runtime::hermes_bridge::resolve_hermes_home().unwrap_or_else(|_| {
            std::path::Path::new("")
                .join(".hermes")
                .to_string_lossy()
                .into_owned()
        });
        let route_result = host_runtime::hermes_bridge::inspect_route(
            &home,
            host_runtime::hermes_bridge::DEFAULT_WEBHOOK_ROUTE_NAME,
        );
        let bridge_status = host_runtime::hermes_bridge::status_for(&resolved);
        let expected_deliver = resolve_hermes_deliver_target(&resolved, "");
        let host_notify = host_runtime::resolve(&resolved).host_notify;
        let (_, secret_source) =
            host_runtime::hermes_host_notify::resolve_hermes_notify_secret_with_source(
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
                host_runtime::hermes_bridge::deliver_display_name(&expected_deliver)
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

    pub fn run_runtime_host_notify_hermes_setup(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let notify_url_override = changed_flag(command, "notify-url");
        let notify_url = notify_url_override
            .as_deref()
            .map(str::trim)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| resolve_hermes_notify_url(&resolved));
        if notify_url.trim().is_empty() {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "hermes setup requires a notify URL.",
                "Use --notify-url or configure runtime.host_notify.hermes.notify_url first.",
            ));
        }
        host_runtime::hermes_bridge::validate_local_notify_url(&notify_url).map_err(|err| {
            ExitError::new(
                "invalid_argument",
                2,
                err.to_string(),
                "Use a local notify URL such as http://127.0.0.1:8765/notify/host-event for the fully managed Hermes flow.",
            )
        })?;
        let deliver_override = changed_flag(command, "deliver");
        let deliver = resolve_hermes_deliver_target(
            &resolved,
            deliver_override.as_deref().unwrap_or_default(),
        );
        if !host_runtime::hermes_bridge::is_supported_deliver_target(&deliver) {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                format!("unsupported Hermes deliver target {deliver:?}"),
                format!(
                    "Use --deliver with one of: {}.",
                    host_runtime::hermes_bridge::supported_deliver_targets().join(", ")
                ),
            ));
        }
        let (mut secret_value, mut secret_source) =
            resolve_hermes_notify_secret_for_setup(&resolved, &notify_url)?;
        if changed_flag(command, "secret").is_some() {
            let value = command.flags.get("secret").cloned().unwrap_or_default();
            if value.trim().is_empty() {
                return Err(ExitError::new(
                    "invalid_argument",
                    2,
                    "hermes setup requires a non-empty --secret when the flag is provided.",
                    "Use --secret <secret>.",
                ));
            }
            secret_value = value.trim().to_string();
            secret_source = "flag".to_string();
        } else if secret_value.trim().is_empty() {
            secret_value = generate_hermes_notify_secret();
            secret_source = "generated".to_string();
        }

        if self.globals.dry_run {
            let hermes_config_file = Path::new(&resolve_hermes_home_dir())
                .join("config.yaml")
                .to_string_lossy()
                .into_owned();
            return self.render_success(
                "awiki-cli runtime host-notify hermes setup",
                &resolved,
                json!({
                    "plan": {
                        "action": "host_notify_hermes_setup",
                        "notify_url": notify_url,
                        "secret_source": secret_source,
                        "deliver": deliver,
                        "previous_sink": host_runtime::resolve(&resolved).host_notify.sink,
                        "host_notify_enabled": true,
                        "awiki_config_file": resolved.paths.config_file,
                        "hermes_config_file": hermes_config_file,
                        "manages_local_hermes": true,
                        "starts_local_bridge": true,
                        "route_uses_home_channel": true,
                    }
                }),
                "Dry run: Hermes host notify setup planned",
                Vec::new(),
            );
        }

        workspace_config::configure_hermes_host_notify(
            &resolved.paths,
            notify_url_override.as_deref(),
            Some(&secret_value),
            deliver_override.as_deref(),
            true,
        )
        .map_err(|err| {
            ExitError::new(
                "internal_error",
                1,
                err.to_string(),
                "Check write permissions for config.yaml.",
            )
        })?;
        let resolved = self.resolve_config().map_err(|err| {
            ExitError::new(
                "internal_error",
                1,
                err.detail.message,
                "Run `awiki-cli config show` to inspect the updated configuration.",
            )
        })?;
        let route_state = host_runtime::hermes_bridge::ensure_route(
            host_runtime::hermes_bridge::EnsureRouteOptions {
                hermes_home: resolve_hermes_home_dir(),
                route_name: host_runtime::hermes_bridge::DEFAULT_WEBHOOK_ROUTE_NAME.to_string(),
                deliver: deliver.clone(),
                webhook_port: 0,
                prompt: String::new(),
            },
        )
        .map_err(|err| {
            ExitError::new(
                "internal_error",
                1,
                err.to_string(),
                "Check local Hermes installation and ~/.hermes/config.yaml permissions.",
            )
        })?;
        let host_notify_view = host_runtime::host_notify_config_view(&resolved).map_err(|err| {
            ExitError::new(
                "internal_error",
                1,
                err.to_string(),
                "Check host notify configuration and local OpenClaw route registry state.",
            )
        })?;
        let (listener_status, listener_warnings) =
            refresh_listener_for_host_notify_change(&resolved).map_err(|err| {
                ExitError::new(
                    "internal_error",
                    1,
                    err.to_string(),
                    "Hermes host notify setup was written, but the listener could not be restarted to apply it.",
                )
            })?;
        let (bridge_status, bridge_service_warnings) = apply_hermes_bridge_for_setup(&resolved)?;
        let mut warnings = listener_warnings;
        warnings.extend(host_notify_guidance_warnings_for(&resolved, &deliver));
        warnings.extend(route_state.warnings.clone());
        warnings.extend(bridge_status.warnings.clone());
        warnings.extend(bridge_service_warnings);
        if deliver != "log" && !route_state.home_channel_configured {
            if route_state.home_channel_key.is_empty() {
                warnings.push(format!(
                    "Hermes route is ready, but awiki-cli could not verify a home channel for {}. Set a home channel in Hermes before expecting delivery.",
                    host_runtime::hermes_bridge::deliver_display_name(&deliver)
                ));
            } else {
                warnings.push(format!(
                    "Hermes route is ready, but {} is still missing. Run /sethome in {} to complete delivery targeting.",
                    route_state.home_channel_key,
                    host_runtime::hermes_bridge::deliver_display_name(&deliver)
                ));
            }
        }
        self.render_success(
            "awiki-cli runtime host-notify hermes setup",
            &resolved,
            json!({
                "host_notify": host_notify_view,
                "local_hermes": route_state,
                "listener": listener_status,
                "bridge": bridge_status,
                "next_steps": [
                    format!(
                        "If you have not done it yet, send `/sethome` to Hermes from the desired {} chat.",
                        host_runtime::hermes_bridge::deliver_display_name(&deliver)
                    ),
                    "Use `awiki-cli runtime host-notify hermes status` to verify end-to-end readiness.".to_string(),
                ],
            }),
            "Hermes host notify setup completed",
            dedupe_strings(warnings),
        )
    }

    pub fn run_runtime_host_notify_hermes_bridge_service_run(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config().map_err(|err| {
            ExitError::new(
                "internal_error",
                1,
                err.detail.message,
                "Run `awiki-cli doctor` to inspect runtime configuration.",
            )
        })?;
        let bridge_config = host_runtime::hermes_bridge::resolve_bridge_config(&resolved)
            .map_err(|err| ExitError::new("internal_error", 1, err.to_string(), String::new()))?;
        let adapter_plan = host_runtime::hermes_bridge::adapter_command_plan_for(&bridge_config);
        host_runtime::hermes_bridge::run_bridge_service(&adapter_plan)
            .map_err(|err| ExitError::new("internal_error", 1, err.to_string(), String::new()))
    }

    pub fn run_runtime_host_notify_hermes_set(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let notify_url = changed_flag(command, "notify-url");
        let deliver = changed_flag(command, "deliver")
            .map(|value| resolve_hermes_deliver_target(&resolved, &value));
        if notify_url.is_none() && deliver.is_none() {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "hermes set requires at least one changed flag.",
                "Use --notify-url or --deliver.",
            ));
        }
        if self.globals.dry_run {
            return self.render_success(
                "awiki-cli runtime host-notify hermes set",
                &resolved,
                json!({ "plan": { "action": "host_notify_hermes_set", "notify_url": notify_url, "deliver": deliver, "config_file": resolved.paths.config_file } }),
                "Dry run: Hermes host notify config change planned",
                Vec::new(),
            );
        }
        if let Some(deliver) = deliver.as_deref() {
            if !host_runtime::hermes_bridge::is_supported_deliver_target(deliver) {
                return Err(ExitError::new(
                    "invalid_argument",
                    2,
                    format!("unsupported Hermes deliver target {deliver:?}"),
                    format!(
                        "Use --deliver with one of: {}.",
                        host_runtime::hermes_bridge::supported_deliver_targets().join(", ")
                    ),
                ));
            }
        }
        workspace_config::update_hermes_settings(
            &resolved.paths,
            notify_url.as_deref(),
            deliver.as_deref(),
        )
        .map_err(internal_anyhow)?;
        let resolved = self.resolve_config()?;
        let (listener, mut warnings) =
            refresh_listener_for_host_notify_change(&resolved).map_err(internal_anyhow)?;
        warnings.extend(host_notify_guidance_warnings_for(
            &resolved,
            &resolve_hermes_deliver_target(&resolved, ""),
        ));
        self.render_success(
            "awiki-cli runtime host-notify hermes set",
            &resolved,
            json!({
                "hermes": host_runtime::resolve(&resolved).host_notify.hermes.map(|hermes| json!(hermes)).unwrap_or_else(|| json!({})),
                "listener": listener,
            }),
            "Hermes host notify config updated",
            warnings,
        )
    }

    pub fn run_runtime_host_notify_hermes_set_secret(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        let value = command.flags.get("value").cloned().unwrap_or_default();
        if value.trim().is_empty() {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "hermes set-secret requires --value.",
                "Use --value <secret>.",
            ));
        }
        if self.globals.dry_run {
            return self.render_success(
                "awiki-cli runtime host-notify hermes set-secret",
                &resolved,
                json!({ "plan": { "action": "host_notify_hermes_set_secret", "configured": true, "config_file": resolved.paths.config_file } }),
                "Dry run: Hermes secret update planned",
                Vec::new(),
            );
        }
        workspace_config::set_hermes_secret(&resolved.paths, &value).map_err(internal_anyhow)?;
        let resolved = self.resolve_config()?;
        let (listener, mut warnings) =
            refresh_listener_for_host_notify_change(&resolved).map_err(internal_anyhow)?;
        warnings.extend(host_notify_guidance_warnings_for(
            &resolved,
            &resolve_hermes_deliver_target(&resolved, ""),
        ));
        self.render_success(
            "awiki-cli runtime host-notify hermes set-secret",
            &resolved,
            json!({
                "hermes": { "secret_configured": true },
                "listener": listener,
            }),
            "Hermes secret updated",
            warnings,
        )
    }

    pub fn run_runtime_host_notify_hermes_clear_secret(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config()?;
        if self.globals.dry_run {
            return self.render_success(
                "awiki-cli runtime host-notify hermes clear-secret",
                &resolved,
                json!({ "plan": { "action": "host_notify_hermes_clear_secret", "config_file": resolved.paths.config_file } }),
                "Dry run: Hermes secret clear planned",
                Vec::new(),
            );
        }
        workspace_config::clear_hermes_secret(&resolved.paths).map_err(internal_anyhow)?;
        let resolved = self.resolve_config()?;
        let (listener, warnings) =
            refresh_listener_for_host_notify_change(&resolved).map_err(internal_anyhow)?;
        self.render_success(
            "awiki-cli runtime host-notify hermes clear-secret",
            &resolved,
            json!({
                "hermes": { "secret_configured": false },
                "listener": listener,
            }),
            "Hermes secret cleared",
            warnings,
        )
    }
}

fn changed_flag(command: &ParsedCommand, name: &str) -> Option<String> {
    command
        .changed_flags
        .iter()
        .any(|flag| flag == name)
        .then(|| command.flags.get(name).cloned().unwrap_or_default())
}

fn apply_hermes_bridge_for_setup(
    resolved: &Resolved,
) -> Result<(host_runtime::hermes_bridge::BridgeStatus, Vec<String>), ExitError> {
    if host_runtime::hermes_bridge::systemd_service_supported() {
        let status = host_runtime::hermes_bridge::apply_service(resolved).map_err(|err| {
            ExitError::new(
                "internal_error",
                1,
                err.to_string(),
                "Hermes was configured, but the local Hermes bridge could not be started.",
            )
        })?;
        return Ok((status, Vec::new()));
    }
    Ok((
        host_runtime::hermes_bridge::status_for(resolved),
        vec![format!(
            "Hermes bridge service install/start requires {}=1 on Linux with a user systemd session; reporting passive bridge status.",
            host_runtime::hermes_bridge::ENABLE_SYSTEMD_SERVICE_ENV
        )],
    ))
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
    let config_file = resolved.paths.config_file.trim();
    if !config_file.is_empty() {
        let (file_config, exists, error) = workspace_config::read_file_config(config_file);
        if error.is_empty() && exists {
            let value = file_config.runtime.host_notify.hermes.deliver.trim();
            if !value.is_empty() {
                return value.to_ascii_lowercase();
            }
        }
    }
    host_runtime::hermes_bridge::normalize_deliver_target("")
}

fn resolve_hermes_notify_url(resolved: &Resolved) -> String {
    let value = resolved.host_notify_hermes_notify_url.trim();
    if !value.is_empty() {
        return value.to_string();
    }
    let config_file = resolved.paths.config_file.trim();
    if !config_file.is_empty() {
        let (file_config, exists, error) = workspace_config::read_file_config(config_file);
        if error.is_empty() && exists {
            let value = file_config.runtime.host_notify.hermes.notify_url.trim();
            if !value.is_empty() {
                return value.to_string();
            }
            let value = file_config.runtime.host_notify.webhook.notify_url.trim();
            if !value.is_empty() {
                return value.to_string();
            }
        }
    }
    host_runtime::hermes_bridge::DEFAULT_NOTIFY_URL.to_string()
}

fn resolve_hermes_notify_secret_for_setup(
    resolved: &Resolved,
    notify_url: &str,
) -> Result<(String, String), ExitError> {
    let config_file = resolved.paths.config_file.trim();
    if !config_file.is_empty() {
        let (file_config, exists, error) = workspace_config::read_file_config(config_file);
        if !error.is_empty() {
            return Err(ExitError::new(
                "internal_error",
                1,
                error,
                "Check awiki-cli host notify secret sources.",
            ));
        }
        if exists {
            let secret = file_config.runtime.host_notify.hermes.secret.trim();
            if !secret.is_empty() {
                return Ok((secret.to_string(), "config_file".to_string()));
            }
            let secret = file_config.runtime.host_notify.webhook.secret.trim();
            if !secret.is_empty() {
                return Ok((secret.to_string(), "config_file".to_string()));
            }
        }
    }
    if !notify_url.trim().is_empty() {
        if let Ok(secret) =
            std::env::var(host_runtime::hermes_host_notify::HERMES_NOTIFY_SECRET_ENV)
        {
            let secret = secret.trim();
            if !secret.is_empty() {
                return Ok((secret.to_string(), "environment".to_string()));
            }
        }
        if let Ok(secret) =
            std::env::var(host_runtime::hermes_host_notify::LEGACY_WEBHOOK_NOTIFY_SECRET_ENV)
        {
            let secret = secret.trim();
            if !secret.is_empty() {
                return Ok((secret.to_string(), "environment".to_string()));
            }
        }
    }
    Ok((String::new(), "unset".to_string()))
}

fn resolve_hermes_home_dir() -> String {
    host_runtime::hermes_bridge::resolve_hermes_home().unwrap_or_else(|_| {
        Path::new(&std::env::var("HOME").unwrap_or_default())
            .join(".hermes")
            .to_string_lossy()
            .into_owned()
    })
}

fn generate_hermes_notify_secret() -> String {
    let mut bytes = [0u8; 24];
    let mut rng = rand::rngs::OsRng;
    if rng.try_fill_bytes(&mut bytes).is_err() {
        return "awiki-hermes-secret".to_string();
    }
    hex_lower(&bytes)
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

pub(super) fn host_notify_guidance_warnings_for(
    resolved: &Resolved,
    deliver_override: &str,
) -> Vec<String> {
    let config = host_runtime::resolve(resolved).host_notify;
    if config.sink != "hermes" {
        return Vec::new();
    }
    let deliver = resolve_hermes_deliver_target(resolved, deliver_override);
    let home_channel_key = host_runtime::hermes_bridge::home_channel_env_key(&deliver);
    let target_warning = if deliver != "log" && !home_channel_key.is_empty() {
        format!(
            "For {} delivery, prefer setting {} (or using /sethome in Hermes) instead of hard-coding deliver_extra.chat_id in Hermes routes.",
            host_runtime::hermes_bridge::deliver_display_name(&deliver),
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
    let current_host_notify = host_runtime::resolve(resolved).host_notify;
    let current_deliver = resolve_hermes_deliver_target(resolved, "");
    let home_channel_key = host_runtime::hermes_bridge::home_channel_env_key(deliver);
    let mut targeting = Vec::new();
    if !home_channel_key.is_empty() {
        targeting.push(format!(
            "Prefer {home_channel_key} for the default delivery target."
        ));
    }
    if deliver != "log" {
        targeting.push(format!(
            "Or send /sethome or /set-home to Hermes from the desired {} chat.",
            host_runtime::hermes_bridge::deliver_display_name(deliver)
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
