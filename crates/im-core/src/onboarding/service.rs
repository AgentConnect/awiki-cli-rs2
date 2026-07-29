pub struct SkillOnboardingService<'a> {
    core: &'a crate::core::ImCore,
}

impl<'a> SkillOnboardingService<'a> {
    pub(crate) fn new(core: &'a crate::core::ImCore) -> Self {
        Self { core }
    }

    pub async fn claim_async(
        &self,
        request: super::SkillClaimRequest,
    ) -> crate::ImResult<super::SkillClaimResult> {
        let mut remote =
            crate::internal::skill_onboarding::ProductionSkillOnboardingRemote::new(self.core);
        crate::internal::skill_onboarding::claim_with_remote(self.core, request, &mut remote).await
    }

    #[cfg(feature = "blocking")]
    pub fn claim(
        &self,
        request: super::SkillClaimRequest,
    ) -> crate::ImResult<super::SkillClaimResult> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| crate::ImError::Internal {
                message: format!("create Skill onboarding runtime: {error}"),
            })?;
        runtime.block_on(self.claim_async(request))
    }

    #[cfg(not(feature = "blocking"))]
    pub fn claim(
        &self,
        _request: super::SkillClaimRequest,
    ) -> crate::ImResult<super::SkillClaimResult> {
        Err(crate::ImError::unsupported("sync-skill-onboarding"))
    }
}
