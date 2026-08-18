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

    pub async fn resume_async(
        &self,
        request: super::SkillResumeRequest,
    ) -> crate::ImResult<super::SkillClaimResult> {
        let mut remote =
            crate::internal::skill_onboarding::ProductionSkillOnboardingRemote::new(self.core);
        crate::internal::skill_onboarding::resume_with_remote(self.core, request, &mut remote).await
    }

    pub async fn recover_legacy_claim_async(
        &self,
        request: super::SkillClaimRequest,
    ) -> crate::ImResult<super::SkillClaimResult> {
        let mut remote =
            crate::internal::skill_onboarding::ProductionSkillOnboardingRemote::new(self.core);
        crate::internal::skill_onboarding::recover_legacy_claim_with_remote(
            self.core,
            request,
            &mut remote,
        )
        .await
    }

    /// Creates or resumes one additional Skill Agent identity without exposing
    /// the registration token outside Rust. The controller identity is used
    /// only for capability discovery and authenticated token issuance.
    pub async fn provision_agent_async(
        &self,
        request: super::SkillAgentProvisionRequest,
    ) -> crate::ImResult<super::SkillAgentProvisionResult> {
        let controller = self
            .core
            .client_async(request.controller_identity.clone())
            .await?;
        let mut remote = crate::internal::skill_onboarding::ProductionSkillProvisionRemote::new(
            self.core,
            &controller,
        );
        crate::internal::skill_onboarding::provision_with_remote(self.core, request, &mut remote)
            .await
    }

    /// Acknowledges that the Host durably committed its binding record. This
    /// deletes the encrypted one-time token while retaining the non-secret
    /// completion journal for idempotent replay.
    pub fn acknowledge_agent_provision(&self, operation_id: &str) -> crate::ImResult<()> {
        crate::internal::skill_onboarding::acknowledge_provision(self.core, operation_id)
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

    #[cfg(feature = "blocking")]
    pub fn resume(
        &self,
        request: super::SkillResumeRequest,
    ) -> crate::ImResult<super::SkillClaimResult> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| crate::ImError::Internal {
                message: format!("create Skill onboarding runtime: {error}"),
            })?;
        runtime.block_on(self.resume_async(request))
    }

    #[cfg(feature = "blocking")]
    pub fn recover_legacy_claim(
        &self,
        request: super::SkillClaimRequest,
    ) -> crate::ImResult<super::SkillClaimResult> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| crate::ImError::Internal {
                message: format!("create Skill onboarding recovery runtime: {error}"),
            })?;
        runtime.block_on(self.recover_legacy_claim_async(request))
    }

    #[cfg(not(feature = "blocking"))]
    pub fn claim(
        &self,
        _request: super::SkillClaimRequest,
    ) -> crate::ImResult<super::SkillClaimResult> {
        Err(crate::ImError::unsupported("sync-skill-onboarding"))
    }

    #[cfg(not(feature = "blocking"))]
    pub fn resume(
        &self,
        _request: super::SkillResumeRequest,
    ) -> crate::ImResult<super::SkillClaimResult> {
        Err(crate::ImError::unsupported("sync-skill-onboarding-resume"))
    }

    #[cfg(not(feature = "blocking"))]
    pub fn recover_legacy_claim(
        &self,
        _request: super::SkillClaimRequest,
    ) -> crate::ImResult<super::SkillClaimResult> {
        Err(crate::ImError::unsupported(
            "sync-skill-onboarding-legacy-recovery",
        ))
    }
}
