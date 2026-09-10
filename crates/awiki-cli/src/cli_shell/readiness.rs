use crate::workspace_config::Resolved;
use serde_json::{json, Value};

pub(super) fn inspect(resolved: &Resolved, identity: Option<&Value>) -> Value {
    let listener = match crate::host_runtime::listener_service_manager::status(resolved) {
        Ok(status) => crate::host_runtime::listener::to_value(status),
        Err(_) => json!({"status_unavailable": true}),
    };
    let mut view = classify(
        &resolved.runtime_mode,
        resolved.runtime_listener_enabled,
        identity,
        &listener,
    );
    view["workspace_initialized"] = json!(resolved.config_exists);
    // An argument array preserves an explicitly selected tenant without shell quoting.
    if let Some(command) = view["realtime"]["next_command"].as_str() {
        let mut args = vec!["awiki-cli".to_owned()];
        if let Some(tenant) = resolved
            .sources
            .get("active_tenant")
            .map(|s| s.value.as_str())
            .filter(|value| !value.is_empty())
        {
            args.extend(["--tenant".to_owned(), tenant.to_owned()]);
        }
        args.extend(command.split_whitespace().skip(1).map(str::to_owned));
        // Tenant IDs are validated by the tenant registry; quote any shell metacharacters.
        let display = args
            .iter()
            .map(|arg| {
                if arg
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || "_-.:/".contains(ch))
                {
                    arg.clone()
                } else {
                    format!("'{}'", arg.replace('\'', "'\"'\"'"))
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        view["realtime"]["next_command"] = json!(display);
        view["realtime"]["next_command_args"] = json!(args);
    }
    view
}

fn classify(mode: &str, enabled: bool, identity: Option<&Value>, listener: &Value) -> Value {
    let identity_ready = identity
        .and_then(|identity| identity["user_state"]["ready_for_messaging"].as_bool())
        .unwrap_or(false);
    let current_did = identity.and_then(|identity| identity["did"].as_str());
    let session_connected = current_did.is_some_and(|did| {
        listener["sessions"].as_array().is_some_and(|sessions| {
            sessions
                .iter()
                .any(|session| session["did"].as_str() == Some(did) && session["connected"] == true)
        })
    });
    let (state, summary, next_command) = if mode != "websocket" {
        (
            "on_demand",
            "HTTP mode uses on-demand commands; background realtime reception is not enabled.",
            None,
        )
    } else if !enabled {
        (
            "disabled",
            "Background realtime reception is disabled.",
            Some("awiki-cli runtime listener enable"),
        )
    } else if listener["status_unavailable"] == true {
        (
            "unknown",
            "Listener status could not be checked.",
            Some("awiki-cli runtime listener status"),
        )
    } else if listener["installed"] != true {
        ("not_installed", "Background realtime reception is not installed. Workspace initialization and identity registration do not start it.", Some("awiki-cli runtime listener install"))
    } else if listener["running"] != true {
        (
            "stopped",
            "The listener is installed but is not running.",
            Some("awiki-cli runtime listener start"),
        )
    } else if !identity_ready {
        (
            "identity_required",
            "The listener is running, but the selected identity is not ready for messaging.",
            Some("awiki-cli id status"),
        )
    } else if listener["bridge_available"] != true || !session_connected {
        (
            "disconnected",
            "The selected identity is not connected to the realtime listener.",
            Some("awiki-cli runtime listener status"),
        )
    } else if listener["reliable_sync"]["v2_subprotocol_negotiated"] != true
        || listener["reliable_sync"]["v2_bootstrap_completed"] != true
        || listener["reliable_sync"]["legacy_sync_used"] != false
    {
        (
            "synchronizing",
            "The listener is connected; reliable message synchronization is not yet ready.",
            Some("awiki-cli runtime listener status"),
        )
    } else {
        (
            "ready",
            "Background realtime reception is ready for the selected identity.",
            None,
        )
    };
    json!({
        "identity_ready": identity_ready,
        "realtime": {
            "ready": state == "ready",
            "state": state,
            "summary": summary,
            "next_command": next_command,
        },
    })
}

pub(super) fn append_warning(readiness: &Value, warnings: &mut Vec<String>) {
    let realtime = &readiness["realtime"];
    if let (Some(summary), Some(command)) = (
        realtime["summary"].as_str(),
        realtime["next_command"].as_str(),
    ) {
        warnings.push(format!("{summary} Next: `{command}`."));
    }
}

#[cfg(test)]
#[path = "readiness_tests.rs"]
mod tests;
