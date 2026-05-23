use anp::group_e2ee::operations::{
    AbortCommitInput, AbortCommitOutput, AddMemberInput, CreateGroupInput, DecryptInput,
    DecryptOutput, EncryptInput, EncryptOutput, FinalizeCommitInput, FinalizeCommitOutput,
    GenerateKeyPackageInput, GroupKeyPackageOutput, GroupMlsOperationError,
    PreparedMlsCommitOutput, ProcessNoticeInput, ProcessNoticeOutput, ProcessWelcomeInput,
    ProcessWelcomeOutput, RecoverMemberInput, RemoveMemberInput, StatusInput, StatusOutput,
    UpdateMemberInput,
};

pub(crate) trait GroupMlsProvider {
    fn generate_key_package(
        &self,
        input: GenerateKeyPackageInput,
    ) -> crate::ImResult<GroupKeyPackageOutput>;

    fn create_group_prepare(
        &self,
        input: CreateGroupInput,
    ) -> crate::ImResult<PreparedMlsCommitOutput>;

    fn add_member_prepare(&self, input: AddMemberInput)
        -> crate::ImResult<PreparedMlsCommitOutput>;

    fn remove_member_prepare(
        &self,
        input: RemoveMemberInput,
    ) -> crate::ImResult<PreparedMlsCommitOutput>;

    fn update_member_prepare(
        &self,
        input: UpdateMemberInput,
    ) -> crate::ImResult<PreparedMlsCommitOutput>;

    fn recover_member_prepare(
        &self,
        input: RecoverMemberInput,
    ) -> crate::ImResult<PreparedMlsCommitOutput>;

    fn finalize_commit(&self, input: FinalizeCommitInput) -> crate::ImResult<FinalizeCommitOutput>;

    fn abort_commit(&self, input: AbortCommitInput) -> crate::ImResult<AbortCommitOutput>;

    fn process_welcome(&self, input: ProcessWelcomeInput) -> crate::ImResult<ProcessWelcomeOutput>;

    fn process_notice(&self, input: ProcessNoticeInput) -> crate::ImResult<ProcessNoticeOutput>;

    fn encrypt(&self, input: EncryptInput) -> crate::ImResult<EncryptOutput>;

    fn decrypt(&self, input: DecryptInput) -> crate::ImResult<DecryptOutput>;

    fn status(&self, input: StatusInput) -> crate::ImResult<StatusOutput>;
}

pub(crate) fn map_group_mls_error(error: GroupMlsOperationError) -> crate::ImError {
    match error.code.as_str() {
        "missing_field" | "invalid_field" | "owner_scope_mismatch" | "invalid_owner_scope" => {
            crate::ImError::invalid_input(None, error.message)
        }
        "group_not_found" | "pending_commit_not_found" => crate::ImError::LocalStateUnavailable {
            detail: format!("group MLS state unavailable: {}", error.message),
        },
        code => crate::ImError::Internal {
            message: format!("group MLS operation failed ({code}): {}", error.message),
        },
    }
}
