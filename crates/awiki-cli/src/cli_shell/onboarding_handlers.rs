use super::App;
use crate::cli_output::ExitError;
use crate::cli_parser::ParsedCommand;
use serde_json::json;
use std::io::Read;
use zeroize::Zeroize;

const MAX_TOKEN_STDIN_BYTES: u64 = 4098;
const CLAIM_USAGE: &str = "awiki-cli onboarding claim --service-base-url <https-url> --expected-controller-handle <full-handle> --expected-agent-handle <full-handle> --token-stdin";
const RESUME_USAGE: &str = "awiki-cli onboarding resume --service-base-url <https-url> --expected-controller-handle <full-handle> --expected-agent-handle <full-handle>";
const RECOVER_LEGACY_CLAIM_USAGE: &str = "awiki-cli onboarding recover-legacy-claim --service-base-url <https-url> --expected-controller-handle <full-handle> --expected-agent-handle <full-handle> --token-stdin";

impl App {
    pub fn run_onboarding_claim(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let request = claim_request_from_stdin(command)?;
        let resolved = self.resolve_config_for_workspace()?;
        let core = crate::m_core_cli_adapter::build_im_core(&resolved)?;
        let result = core.onboarding().claim(request).map_err(map_claim_error)?;
        self.render_skill_claim_result(
            &resolved,
            result,
            "awiki-cli onboarding claim",
            "Skill Agent registered and Controller greeting accepted",
            "awiki-cli onboarding resume",
        )
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
            .map_err(map_claim_error)?;
        self.render_skill_claim_result(
            &resolved,
            result,
            "awiki-cli onboarding claim",
            "Skill Agent registered and Controller greeting accepted",
            "awiki-cli onboarding resume",
        )
    }

    pub fn run_onboarding_resume(&self, command: &ParsedCommand) -> Result<(), ExitError> {
        let request = resume_request(command)?;
        let resolved = self.resolve_config_for_workspace()?;
        let core = crate::m_core_cli_adapter::build_im_core(&resolved)?;
        let result = core
            .onboarding()
            .resume(request)
            .map_err(map_resume_error)?;
        self.render_skill_claim_result(
            &resolved,
            result,
            "awiki-cli onboarding resume",
            "Skill Agent onboarding resumed",
            "awiki-cli onboarding resume",
        )
    }

    pub async fn run_onboarding_resume_async(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        let request = resume_request(command)?;
        let resolved = self.resolve_config_for_workspace()?;
        let core = crate::m_core_cli_adapter::build_im_core_async(&resolved).await?;
        let result = core
            .onboarding()
            .resume_async(request)
            .await
            .map_err(map_resume_error)?;
        self.render_skill_claim_result(
            &resolved,
            result,
            "awiki-cli onboarding resume",
            "Skill Agent onboarding resumed",
            "awiki-cli onboarding resume",
        )
    }

    pub async fn run_onboarding_recover_legacy_claim_async(
        &self,
        command: &ParsedCommand,
    ) -> Result<(), ExitError> {
        let request = onboarding_request_from_stdin(
            command,
            "onboarding recover-legacy-claim",
            RECOVER_LEGACY_CLAIM_USAGE,
        )?;
        let resolved = self.resolve_config_for_workspace()?;
        let core = crate::m_core_cli_adapter::build_im_core_async(&resolved).await?;
        let result = core
            .onboarding()
            .recover_legacy_claim_async(request)
            .await
            .map_err(map_legacy_claim_recovery_error)?;
        self.render_skill_claim_result(
            &resolved,
            result,
            "awiki-cli onboarding recover-legacy-claim",
            "Legacy Skill claim recovered and migrated to a VNext device identity",
            "awiki-cli onboarding recover-legacy-claim",
        )
    }

    pub async fn run_onboarding_migrate_legacy_async(&self) -> Result<(), ExitError> {
        let resolved = self.resolve_config_for_workspace()?;
        let core = crate::m_core_cli_adapter::build_im_core_async(&resolved).await?;
        let selector = crate::m_core_cli_adapter::cli_identity_selector(&self.globals.identity);
        if self.globals.dry_run {
            let status = core
                .identities()
                .legacy_upgrade_status(selector)
                .map_err(|error| {
                    crate::m_core_cli_adapter::map_im_error(error, "onboarding migrate-legacy")
                })?;
            return self.render_success(
                "awiki-cli onboarding migrate-legacy",
                &resolved,
                json!({"status": status, "dry_run": true}),
                "Legacy Skill identity migration inspected",
                Vec::new(),
            );
        }
        let status = core
            .identities()
            .upgrade_legacy_identity_async(selector)
            .await
            .map_err(|error| {
                crate::m_core_cli_adapter::map_im_error(error, "onboarding migrate-legacy")
            })?;
        if let im_core::identity::LegacyUpgradeStatus::RetryRequired { code, .. } = &status {
            return Err(legacy_migration_retry_error(code, &status));
        }
        self.render_success(
            "awiki-cli onboarding migrate-legacy",
            &resolved,
            json!({"status": status, "dry_run": false}),
            "Legacy Skill identity migrated to a VNext device identity",
            Vec::new(),
        )
    }

    fn render_skill_claim_result(
        &self,
        resolved: &crate::workspace_config::Resolved,
        result: im_core::SkillClaimResult,
        command_name: &str,
        summary: &str,
        retry_command: &str,
    ) -> Result<(), ExitError> {
        if result.status == im_core::SkillClaimStatus::GreetingPending {
            return Err(greeting_pending_error(&result, retry_command));
        }
        self.render_success(
            command_name,
            resolved,
            public_claim_value(&result),
            summary,
            Vec::new(),
        )
    }
}

fn claim_request_from_stdin(
    command: &ParsedCommand,
) -> Result<im_core::SkillClaimRequest, ExitError> {
    onboarding_request_from_stdin(command, "onboarding claim", CLAIM_USAGE)
}

fn resume_request(command: &ParsedCommand) -> Result<im_core::SkillResumeRequest, ExitError> {
    let operation = "onboarding resume";
    if !command.args.is_empty() {
        return Err(invalid_onboarding_argument(
            format!("{operation} does not accept positional arguments."),
            RESUME_USAGE,
        ));
    }
    Ok(im_core::SkillResumeRequest {
        service_base_url: required_flag(command, operation, RESUME_USAGE, "service-base-url")?,
        expected_controller_handle: required_flag(
            command,
            operation,
            RESUME_USAGE,
            "expected-controller-handle",
        )?,
        expected_agent_handle: required_flag(
            command,
            operation,
            RESUME_USAGE,
            "expected-agent-handle",
        )?,
    })
}

fn onboarding_request_from_stdin(
    command: &ParsedCommand,
    operation: &str,
    usage: &str,
) -> Result<im_core::SkillClaimRequest, ExitError> {
    if !command.args.is_empty() {
        return Err(invalid_onboarding_argument(
            format!("{operation} does not accept positional arguments."),
            usage,
        ));
    }
    if command.flags.get("token-stdin").map(String::as_str) != Some("true") {
        return Err(invalid_onboarding_argument(
            format!("{operation} requires --token-stdin."),
            usage,
        ));
    }
    let service_base_url = required_flag(command, operation, usage, "service-base-url")?;
    let expected_controller_handle =
        required_flag(command, operation, usage, "expected-controller-handle")?;
    let expected_agent_handle = required_flag(command, operation, usage, "expected-agent-handle")?;
    let token = read_skill_token_for_usage(std::io::stdin().lock(), usage)?;
    Ok(im_core::SkillClaimRequest {
        token,
        service_base_url,
        expected_controller_handle,
        expected_agent_handle,
    })
}

fn required_flag(
    command: &ParsedCommand,
    operation: &str,
    usage: &str,
    name: &str,
) -> Result<String, ExitError> {
    command
        .flags
        .get(name)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            invalid_onboarding_argument(format!("{operation} requires --{name}."), usage)
        })
}

#[cfg(test)]
fn read_skill_token(reader: impl Read) -> Result<im_core::SkillOnboardingToken, ExitError> {
    read_skill_token_for_usage(reader, CLAIM_USAGE)
}

fn read_skill_token_for_usage(
    reader: impl Read,
    usage: &str,
) -> Result<im_core::SkillOnboardingToken, ExitError> {
    let mut raw = String::new();
    reader
        .take(MAX_TOKEN_STDIN_BYTES + 1)
        .read_to_string(&mut raw)
        .map_err(|_| {
            invalid_onboarding_argument("unable to read a UTF-8 Token from stdin.", usage)
        })?;
    if raw.len() as u64 > MAX_TOKEN_STDIN_BYTES {
        raw.zeroize();
        return Err(invalid_onboarding_argument(
            "stdin Token exceeds the size limit.",
            usage,
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
        return Err(invalid_onboarding_argument(
            "stdin must contain exactly one non-empty Token line.",
            usage,
        ));
    }
    im_core::SkillOnboardingToken::new(raw).map_err(|_| {
        invalid_onboarding_argument("stdin contains an invalid Skill onboarding Token.", usage)
    })
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

fn greeting_pending_error(result: &im_core::SkillClaimResult, retry_command: &str) -> ExitError {
    let mut error = ExitError::new(
        result
            .error_code
            .as_deref()
            .unwrap_or("skill_onboarding_greeting_pending"),
        5,
        "Skill Agent registered, but the Controller greeting is still pending.",
        format!("Retry `{retry_command}` with the same workspace and exact scope fields; no Token is accepted."),
    );
    error.detail.retryable = true;
    error.detail.details = public_claim_value(result);
    error
}

fn legacy_migration_retry_error(
    code: &str,
    status: &im_core::identity::LegacyUpgradeStatus,
) -> ExitError {
    let mut error = ExitError::new(
        "skill_legacy_migration_retry_required",
        5,
        format!("Legacy Skill identity migration requires retry: {code}."),
        "Retry `awiki-cli onboarding migrate-legacy` with the same workspace and identity.",
    );
    error.detail.retryable = true;
    error.detail.details = json!({"status": status});
    error
}

fn map_claim_error(error: im_core::ImError) -> ExitError {
    if matches!(
        &error,
        im_core::ImError::SkillOnboarding { code, .. }
            if code == "skill_onboarding_legacy_claim_recovery_required"
    ) {
        return ExitError::new(
            "skill_onboarding_legacy_claim_recovery_required",
            3,
            "A V1 Skill claim is pending in this workspace and cannot be replaced safely.",
            "Run `awiki-cli onboarding recover-legacy-claim` with the original authorized Token block and scope fields; do not delete the journal or start a new claim.",
        );
    }
    if let im_core::ImError::SkillOnboarding {
        code,
        phase,
        retryable,
    } = &error
    {
        if phase == "device_prekey" || phase == "controller_greeting" {
            return onboarding_resume_error(code, phase, *retryable);
        }
    }
    crate::m_core_cli_adapter::map_im_error(error, "onboarding claim")
}

fn map_resume_error(error: im_core::ImError) -> ExitError {
    if let im_core::ImError::SkillOnboarding {
        code,
        phase,
        retryable,
    } = error
    {
        return onboarding_resume_error(&code, &phase, retryable);
    }
    crate::m_core_cli_adapter::map_im_error(error, "onboarding resume")
}

fn onboarding_resume_error(code: &str, phase: &str, retryable: bool) -> ExitError {
    let mut mapped = ExitError::new(
        code,
        if retryable { 5 } else { 3 },
        "Skill Agent onboarding stopped before the local VNext journal completed.",
        "Retry `awiki-cli onboarding resume` with the same workspace and exact scope fields; no Token is accepted.",
    );
    mapped.detail.retryable = retryable;
    mapped.detail.details = json!({"phase": phase});
    mapped
}

fn map_legacy_claim_recovery_error(error: im_core::ImError) -> ExitError {
    if let im_core::ImError::SkillOnboarding {
        code,
        phase,
        retryable,
    } = error
    {
        let blocked = code == "blocked_requires_operator_reconciliation";
        let mut mapped = ExitError::new(
            &code,
            if retryable { 5 } else { 3 },
            if blocked {
                "The V1 Skill claim is missing required recovery material and cannot continue safely."
            } else {
                "Legacy Skill claim recovery stopped before same-DID migration completed."
            },
            if retryable {
                "Retry `awiki-cli onboarding recover-legacy-claim` with the same workspace and original Token block."
                    .to_owned()
            } else {
                "Preserve the workspace and V1 journal, do not run a new claim, and request operator reconciliation with the original Token block."
                    .to_owned()
            },
        );
        mapped.detail.retryable = retryable;
        mapped.detail.details = json!({"phase": phase});
        return mapped;
    }
    crate::m_core_cli_adapter::map_im_error(error, "onboarding recover-legacy-claim")
}

fn invalid_onboarding_argument(message: impl Into<String>, usage: &str) -> ExitError {
    ExitError::new("invalid_argument", 2, message, format!("Usage: {usage}"))
}

#[cfg(test)]
mod tests;
