use anp::group_e2ee::operations::{
    AbortCommitInput, AbortCommitOutput, AddMemberInput, CreateGroupInput, DecryptInput,
    DecryptOutput, EncryptInput, EncryptOutput, FinalizeCommitInput, FinalizeCommitOutput,
    GenerateKeyPackageInput, GroupKeyPackageOutput, GroupMlsOperationError, LeaveGroupInput,
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

    fn leave_prepare(&self, input: LeaveGroupInput) -> crate::ImResult<PreparedMlsCommitOutput>;

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

impl<T> GroupMlsProvider for &T
where
    T: GroupMlsProvider + ?Sized,
{
    fn generate_key_package(
        &self,
        input: GenerateKeyPackageInput,
    ) -> crate::ImResult<GroupKeyPackageOutput> {
        (**self).generate_key_package(input)
    }

    fn create_group_prepare(
        &self,
        input: CreateGroupInput,
    ) -> crate::ImResult<PreparedMlsCommitOutput> {
        (**self).create_group_prepare(input)
    }

    fn add_member_prepare(
        &self,
        input: AddMemberInput,
    ) -> crate::ImResult<PreparedMlsCommitOutput> {
        (**self).add_member_prepare(input)
    }

    fn remove_member_prepare(
        &self,
        input: RemoveMemberInput,
    ) -> crate::ImResult<PreparedMlsCommitOutput> {
        (**self).remove_member_prepare(input)
    }

    fn leave_prepare(&self, input: LeaveGroupInput) -> crate::ImResult<PreparedMlsCommitOutput> {
        (**self).leave_prepare(input)
    }

    fn update_member_prepare(
        &self,
        input: UpdateMemberInput,
    ) -> crate::ImResult<PreparedMlsCommitOutput> {
        (**self).update_member_prepare(input)
    }

    fn recover_member_prepare(
        &self,
        input: RecoverMemberInput,
    ) -> crate::ImResult<PreparedMlsCommitOutput> {
        (**self).recover_member_prepare(input)
    }

    fn finalize_commit(&self, input: FinalizeCommitInput) -> crate::ImResult<FinalizeCommitOutput> {
        (**self).finalize_commit(input)
    }

    fn abort_commit(&self, input: AbortCommitInput) -> crate::ImResult<AbortCommitOutput> {
        (**self).abort_commit(input)
    }

    fn process_welcome(&self, input: ProcessWelcomeInput) -> crate::ImResult<ProcessWelcomeOutput> {
        (**self).process_welcome(input)
    }

    fn process_notice(&self, input: ProcessNoticeInput) -> crate::ImResult<ProcessNoticeOutput> {
        (**self).process_notice(input)
    }

    fn encrypt(&self, input: EncryptInput) -> crate::ImResult<EncryptOutput> {
        (**self).encrypt(input)
    }

    fn decrypt(&self, input: DecryptInput) -> crate::ImResult<DecryptOutput> {
        (**self).decrypt(input)
    }

    fn status(&self, input: StatusInput) -> crate::ImResult<StatusOutput> {
        (**self).status(input)
    }
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
