use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretAccessPolicy {
    pub no_prompt: bool,
    pub user_presence_required: bool,
    pub exportable: bool,
    pub cache_ttl_seconds: Option<u64>,
}

impl SecretAccessPolicy {
    pub fn no_prompt_local_secret() -> Self {
        Self {
            no_prompt: true,
            user_presence_required: false,
            exportable: false,
            cache_ttl_seconds: None,
        }
    }

    pub fn validate_no_prompt(&self) -> crate::ImResult<()> {
        if !self.no_prompt || self.user_presence_required {
            return Err(crate::ImError::unsupported(
                "secret vault record requires user presence; awiki vault access must be no-prompt",
            ));
        }
        Ok(())
    }
}
