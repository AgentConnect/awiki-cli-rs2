use serde::{Deserialize, Serialize};
use std::fmt;
use zeroize::Zeroize;

#[derive(Clone, PartialEq, Eq)]
pub struct SkillOnboardingToken(String);

impl SkillOnboardingToken {
    pub fn new(value: impl Into<String>) -> crate::ImResult<Self> {
        let mut value = value.into();
        let trimmed = value.trim();
        if trimmed.len() < 16 || trimmed.len() > 4096 || trimmed.chars().any(char::is_whitespace) {
            value.zeroize();
            return Err(crate::ImError::invalid_input(
                Some("token".to_owned()),
                "Skill onboarding token must be a single non-empty value",
            ));
        }
        let token = trimmed.to_owned();
        value.zeroize();
        Ok(Self(token))
    }

    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SkillOnboardingToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SkillOnboardingToken(<redacted>)")
    }
}

impl Drop for SkillOnboardingToken {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillClaimRequest {
    pub token: SkillOnboardingToken,
    pub service_base_url: String,
    pub expected_controller_handle: String,
    pub expected_agent_handle: String,
}

/// Scope required to resume a locally committed VNext Skill onboarding.
///
/// Resume never accepts or reuses the one-time onboarding Token. The exact
/// scope is checked against the schema-v2 journal before any remote operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillResumeRequest {
    pub service_base_url: String,
    pub expected_controller_handle: String,
    pub expected_agent_handle: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillClaimPhase {
    TokenVerified,
    IdentityPending,
    IdentityRegistered,
    ControllerGreetingPending,
    ControllerGreetingSent,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillClaimStatus {
    Completed,
    GreetingPending,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillClaimResult {
    pub phase: SkillClaimPhase,
    pub status: SkillClaimStatus,
    pub agent_did: crate::ids::Did,
    pub agent_handle: crate::ids::Handle,
    pub controller_handle: crate::ids::Handle,
    pub greeting_message_id: crate::ids::MessageId,
    pub retryable: bool,
    pub error_code: Option<String>,
}
