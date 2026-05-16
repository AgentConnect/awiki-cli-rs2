use crate::config::Resolved;
use crate::runtime;
use serde_json::Value;

pub(super) fn refresh_listener_for_host_notify_change(
    resolved: &Resolved,
) -> anyhow::Result<(Value, Vec<String>)> {
    let status = runtime::current_listener_status(resolved);
    let runtime_resolved = runtime::resolve(resolved);
    if runtime_resolved.mode != "websocket" || !runtime_resolved.listener.enabled {
        return Ok((
            status,
            vec![
                "Host notify changes will apply the next time the websocket listener is enabled."
                    .to_string(),
            ],
        ));
    }
    if status.get("running").and_then(Value::as_bool) != Some(true) {
        return Ok((
            status,
            vec!["Host notify changes will apply the next time the listener starts.".to_string()],
        ));
    }

    runtime::stop_listener(resolved)
        .map_err(|err| anyhow::anyhow!("stop listener to apply host notify config: {err}"))?;
    let restarted = runtime::apply_runtime_policy(resolved)
        .map_err(|err| anyhow::anyhow!("restart listener to apply host notify config: {err}"))?;
    let mut warnings = vec!["Listener restarted to apply host notify configuration.".to_string()];
    if let Some(listener_warnings) = restarted.get("warnings").and_then(Value::as_array) {
        warnings.extend(
            listener_warnings
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string),
        );
    }
    Ok((restarted, warnings))
}
