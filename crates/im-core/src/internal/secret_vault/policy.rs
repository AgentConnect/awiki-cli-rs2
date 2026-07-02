use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SecretAccessPolicy {
    pub(crate) no_prompt: bool,
    pub(crate) user_presence_required: bool,
    pub(crate) exportable: bool,
    pub(crate) cache_ttl_seconds: Option<u64>,
}

impl SecretAccessPolicy {
    pub(crate) fn no_prompt_local_secret() -> Self {
        Self {
            no_prompt: true,
            user_presence_required: false,
            exportable: false,
            cache_ttl_seconds: None,
        }
    }

    pub(crate) fn validate_no_prompt(&self) -> crate::ImResult<()> {
        if !self.no_prompt || self.user_presence_required {
            return Err(crate::ImError::unsupported(
                "secret vault record requires user presence; awiki vault access must be no-prompt",
            ));
        }
        Ok(())
    }
}
