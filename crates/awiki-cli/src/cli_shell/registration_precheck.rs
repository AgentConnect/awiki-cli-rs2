//! Read-only admission before Core creates a durable registration attempt.
use crate::cli_http::http::{new_http_client_with_proxy_env, HttpRequest};
use crate::cli_output::ExitError;
use crate::cli_parser::ParsedCommand;
use crate::workspace_config::Resolved;
use serde_json::{json, Value};
use std::time::Duration;

fn unavailable() -> ExitError {
    ExitError::new(
        "registration_check_unavailable",
        5,
        "Registration precheck is unavailable.",
        "Retry after the User Service supports registration_check.",
    )
}

fn input(
    resolved: &Resolved,
    command: &ParsedCommand,
) -> Result<(String, String, Value, String), ExitError> {
    let request = crate::m_core_cli_adapter::identity::register_handle_request(command)?;
    let target = crate::m_core_cli_adapter::identity::register_plan_target(
        request.requested_handle.as_str(),
        &resolved.did_domain,
    )?;
    let handle = target.local_part;
    let domain = target.effective_domain;
    let mut params = json!({"handle": handle, "domain": domain, "check_invite": command.flags.contains_key("invite-code")});
    for (flag, field) in [
        ("phone", "phone"),
        ("email", "email"),
        ("invite-code", "invite_code"),
    ] {
        if let Some(value) = command.flags.get(flag) {
            params[field] = json!(value);
        }
    }
    let base = if resolved.user_service_endpoint.is_empty() {
        &resolved.service_base_url
    } else {
        &resolved.user_service_endpoint
    };
    let endpoint = format!("{}/user-service/v1/handle/rpc", base.trim_end_matches('/'));
    Ok((
        endpoint,
        resolved.ca_bundle.clone(),
        params,
        target.full_handle.as_str().to_owned(),
    ))
}

fn evaluate(envelope: &Value, expected: &str) -> Result<(), ExitError> {
    let value = &envelope["result"];
    let decision = value["decision"].as_str();
    let required = value["invite_required"].as_bool();
    let status = value["invite_status"].as_str();
    if envelope["jsonrpc"] != "2.0"
        || envelope["id"] != "registration-check"
        || !envelope["error"].is_null()
        || value["full_handle"] != expected
        || !matches!(decision, Some("register" | "existing" | "unavailable"))
        || required.is_none()
        || !matches!(
            status,
            Some("not_required" | "required" | "valid" | "invalid")
        )
        || (required == Some(false) && status != Some("not_required"))
        || (required == Some(true) && status == Some("not_required"))
        || (decision == Some("existing") && required != Some(false))
    {
        return Err(unavailable());
    }
    if decision == Some("existing") {
        return Ok(());
    }
    if decision == Some("unavailable") {
        return Err(ExitError::new(
            "handle_unavailable",
            5,
            "The Handle is unavailable.",
            "Choose another Handle.",
        ));
    }
    if required == Some(true) && status != Some("valid") {
        return Err(ExitError::new(
            "registration_invite_required",
            5,
            "A valid invitation is required for this new Handle.",
            "Provide invite_code through --verification-stdin and retry.",
        ));
    }
    Ok(())
}

fn check(
    (endpoint, ca, params, expected): (String, String, Value, String),
) -> Result<(), ExitError> {
    let client = new_http_client_with_proxy_env(&ca).map_err(|_| unavailable())?;
    let request = HttpRequest::new("POST", endpoint).header("Content-Type", "application/json")
        .body(json!({"jsonrpc":"2.0","id":"registration-check","method":"registration_check","params":params}).to_string())
        .timeout(Duration::from_secs(10));
    let response = client.execute(request).map_err(|_| unavailable())?;
    if response.status_code != 200 || response.body.len() > 65536 {
        return Err(unavailable());
    }
    let envelope = serde_json::from_slice(&response.body).map_err(|_| unavailable())?;
    evaluate(&envelope, &expected)
}

pub(super) fn run(resolved: &Resolved, command: &ParsedCommand) -> Result<(), ExitError> {
    check(input(resolved, command)?)
}
pub(super) async fn run_async(
    resolved: &Resolved,
    command: &ParsedCommand,
) -> Result<(), ExitError> {
    let input = input(resolved, command)?;
    tokio::task::spawn_blocking(move || check(input))
        .await
        .map_err(|_| unavailable())?
}

#[cfg(test)]
#[path = "registration_precheck_tests.rs"]
mod tests;
