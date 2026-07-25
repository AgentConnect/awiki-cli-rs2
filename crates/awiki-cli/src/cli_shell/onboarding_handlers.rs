use super::App;
use crate::cli_output::ExitError;
use crate::cli_parser::ParsedCommand;
use serde_json::json;
use std::io::Read;
use zeroize::Zeroize;

const MAX_TOKEN_STDIN_BYTES: u64 = 4098;
const CLAIM_USAGE: &str = "awiki-cli onboarding claim --service-base-url <https-url> --expected-controller-handle <full-handle> --expected-agent-handle <full-handle> --token-stdin";

impl App {
    pub fn run_onboarding_claim(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let request = claim_request_from_stdin(command)?;
        let resolved = self.resolve_config_for_workspace()?;
        let core = crate::m_core_cli_adapter::build_im_core(&resolved)?;
        let result = core
            .onboarding()
            .claim(request)
            .map_err(|error| crate::m_core_cli_adapter::map_im_error(error, "onboarding claim"))?;
        self.render_skill_claim_result(&resolved, result)
    }

    pub async fn run_onboarding_claim_async(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        let request = claim_request_from_stdin(command)?;
        let resolved = self.resolve_config_for_workspace()?;
        let core = crate::m_core_cli_adapter::build_im_core_async(&resolved).await?;
        let result = core
            .onboarding()
            .claim_async(request)
            .await
            .map_err(|error| crate::m_core_cli_adapter::map_im_error(error, "onboarding claim"))?;
        self.render_skill_claim_result(&resolved, result)
    }

    fn render_skill_claim_result(
        &self,
        resolved: &crate::workspace_config::Resolved,
        result: im_core::SkillClaimResult,
    ) -> Result<(), ExitError> {
        if result.status == im_core::SkillClaimStatus::GreetingPending {
            return Err(greeting_pending_error(&result));
        }
        self.render_success(
            "awiki-cli onboarding claim",
            resolved,
            public_claim_value(&result),
            "Skill Agent registered and Controller greeting accepted",
            Vec::new(),
        )
    }
}

fn claim_request_from_stdin(
    command: &ParsedCommand,
) -> Result<im_core::SkillClaimRequest, ExitError> {
    if !command.args.is_empty() {
        return Err(invalid_claim_argument(
            "onboarding claim does not accept positional arguments.",
        ));
    }
    if command.flags.get("token-stdin").map(String::as_str) != Some("true") {
        return Err(invalid_claim_argument(
            "onboarding claim requires --token-stdin.",
        ));
    }
    let service_base_url = required_flag(command, "service-base-url")?;
    let expected_controller_handle = required_flag(command, "expected-controller-handle")?;
    let expected_agent_handle = required_flag(command, "expected-agent-handle")?;
    let token = read_skill_token(std::io::stdin().lock())?;
    Ok(im_core::SkillClaimRequest {
        token,
        service_base_url,
        expected_controller_handle,
        expected_agent_handle,
    })
}

fn required_flag(command: &ParsedCommand, name: &str) -> Result<String, ExitError> {
    command
        .flags
        .get(name)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| invalid_claim_argument(format!("onboarding claim requires --{name}.")))
}

fn read_skill_token(reader: impl Read) -> Result<im_core::SkillOnboardingToken, ExitError> {
    let mut raw = String::new();
    reader
        .take(MAX_TOKEN_STDIN_BYTES + 1)
        .read_to_string(&mut raw)
        .map_err(|_| invalid_claim_argument("unable to read a UTF-8 Token from stdin."))?;
    if raw.len() as u64 > MAX_TOKEN_STDIN_BYTES {
        raw.zeroize();
        return Err(invalid_claim_argument(
            "stdin Token exceeds the size limit.",
        ));
    }
    if raw.ends_with('\n') {
        raw.pop();
        if raw.ends_with('\r') {
            raw.pop();
        }
    }
    if raw.is_empty() || raw.contains(['\r', '\n']) {
        raw.zeroize();
        return Err(invalid_claim_argument(
            "stdin must contain exactly one non-empty Token line.",
        ));
    }
    im_core::SkillOnboardingToken::new(raw)
        .map_err(|_| invalid_claim_argument("stdin contains an invalid Skill onboarding Token."))
}

fn public_claim_value(result: &im_core::SkillClaimResult) -> serde_json::Value {
    json!({
        "phase": result.phase,
        "status": result.status,
        "agent_did": result.agent_did,
        "agent_handle": result.agent_handle,
        "controller_handle": result.controller_handle,
        "greeting_status": if result.status == im_core::SkillClaimStatus::Completed { "sent" } else { "pending" },
        "greeting_message_id": result.greeting_message_id,
        "retryable": result.retryable,
        "error_code": result.error_code,
    })
}

fn greeting_pending_error(result: &im_core::SkillClaimResult) -> ExitError {
    let mut error = ExitError::new(
        result
            .error_code
            .as_deref()
            .unwrap_or("skill_onboarding_greeting_pending"),
        5,
        "Skill Agent registered, but the Controller greeting is still pending.",
        "Retry the same claim command with the same authorized Token block.",
    );
    error.detail.retryable = true;
    error.detail.details = public_claim_value(result);
    error
}

fn invalid_claim_argument(message: impl Into<String>) -> ExitError {
    ExitError::new(
        "invalid_argument",
        2,
        message,
        format!("Usage: {CLAIM_USAGE}"),
    )
}

#[cfg(test)]
mod tests;
