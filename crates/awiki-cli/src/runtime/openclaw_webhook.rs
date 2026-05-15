use crate::config::Resolved;
use crate::transportcfg::{new_http_client, HttpRequest};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;

use super::openclaw_routes::Route;

const FIXED_HOOK_NAME: &str = "AWiki";

#[derive(Debug, Clone, Serialize)]
struct HookRequest {
    message: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    name: String,
    #[serde(rename = "wakeMode", skip_serializing_if = "String::is_empty")]
    wake_mode: String,
    deliver: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    channel: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    to: String,
}

#[derive(Debug, Clone, Deserialize)]
struct HookResponse {
    #[serde(default)]
    ok: bool,
    #[serde(default, rename = "runId")]
    run_id: String,
}

pub fn send_route_confirmation(
    resolved: &Resolved,
    route: &Route,
) -> Result<serde_json::Value, String> {
    let settings = super::effective_openclaw_settings(resolved);
    super::validate_openclaw_hook_url(&settings.hook_url).map_err(|err| {
        format!("route was added, but the confirmation message could not be prepared: {err}")
    })?;
    let request = HookRequest {
        message: build_route_confirmation_message(route),
        name: FIXED_HOOK_NAME.to_string(),
        wake_mode: "now".to_string(),
        deliver: true,
        channel: route.channel.clone(),
        to: route.to.clone(),
    };
    send_hook_request(resolved, &settings.hook_url, &settings.token, request)
        .map(|run_id| json!({ "accepted": true, "run_id": run_id }))
        .map_err(|err| {
            format!(
                "route was added, but the confirmation message was not accepted by OpenClaw: {err}"
            )
        })
}

fn send_hook_request(
    resolved: &Resolved,
    hook_url: &str,
    token: &str,
    request: HookRequest,
) -> anyhow::Result<String> {
    let raw = serde_json::to_vec(&request)
        .map_err(|err| anyhow::anyhow!("marshal openclaw hook payload: {err}"))?;
    let mut http_request = HttpRequest::new("POST", hook_url)
        .header("Content-Type", "application/json")
        .body(raw)
        .timeout(Duration::from_secs(15));
    let token = token.trim();
    if !token.is_empty() {
        http_request = http_request.header("Authorization", format!("Bearer {token}"));
    }
    let client = new_http_client(&resolved.ca_bundle)
        .map_err(|err| anyhow::anyhow!("build openclaw hook request: {err}"))?;
    let response = client
        .execute(http_request)
        .map_err(|err| anyhow::anyhow!("send openclaw hook request: {err}"))?;
    let raw_body = limit_body(&response.body, 4096);
    if !(200..300).contains(&response.status_code) {
        let body = String::from_utf8_lossy(&raw_body).trim().to_string();
        if body.is_empty() {
            anyhow::bail!("openclaw hook failed status={}", response.status_code);
        }
        anyhow::bail!(
            "openclaw hook failed status={}: {}",
            response.status_code,
            body
        );
    }
    let payload: HookResponse = serde_json::from_slice(&raw_body)
        .map_err(|err| anyhow::anyhow!("parse openclaw hook response: {err}"))?;
    if !payload.ok {
        anyhow::bail!("openclaw hook was not accepted");
    }
    let run_id = payload.run_id.trim();
    if run_id.is_empty() {
        anyhow::bail!("openclaw hook response did not include runId");
    }
    Ok(run_id.to_string())
}

fn limit_body(body: &[u8], limit: usize) -> Vec<u8> {
    body.iter().copied().take(limit).collect()
}

fn build_route_confirmation_message(route: &Route) -> String {
    [
        "AWiki notifications are now configured for this conversation.".to_string(),
        "Future AWiki message notifications will be delivered here.".to_string(),
        format!("channel={}", route.channel),
        format!("to={}", route.to),
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_confirmation_message_matches_go_contract() {
        let message = build_route_confirmation_message(&Route {
            channel: "telegram".to_string(),
            to: "123456".to_string(),
        });
        assert_eq!(
            message,
            "AWiki notifications are now configured for this conversation.\nFuture AWiki message notifications will be delivered here.\nchannel=telegram\nto=123456"
        );
    }
}
