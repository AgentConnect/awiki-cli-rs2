use anp::group_e2ee::operations::{
    self, AbortCommitInput, AbortCommitOutput, AddMemberInput, CreateGroupInput, DecryptInput,
    DecryptOutput, EncryptInput, EncryptOutput, FinalizeCommitInput, FinalizeCommitOutput,
    GenerateKeyPackageInput, GroupKeyPackageOutput, LeaveGroupInput, PreparedMlsCommitOutput,
    ProcessNoticeInput, ProcessNoticeOutput, ProcessWelcomeInput, ProcessWelcomeOutput,
    RecoverMemberInput, RemoveMemberInput, StatusInput, StatusOutput, UpdateMemberInput,
};
use anp::group_e2ee::storage::ImCoreSqliteGroupMlsStore;

use super::provider::{map_group_mls_error, GroupMlsProvider};

#[derive(Debug, Clone)]
pub(crate) struct NativeAnpMlsProvider {
    store: ImCoreSqliteGroupMlsStore,
}

impl NativeAnpMlsProvider {
    pub(crate) fn new(store: ImCoreSqliteGroupMlsStore) -> Self {
        Self { store }
    }
}

impl GroupMlsProvider for NativeAnpMlsProvider {
    fn generate_key_package(
        &self,
        input: GenerateKeyPackageInput,
    ) -> crate::ImResult<GroupKeyPackageOutput> {
        operations::generate_key_package(&self.store, input).map_err(map_group_mls_error)
    }

    fn create_group_prepare(
        &self,
        input: CreateGroupInput,
    ) -> crate::ImResult<PreparedMlsCommitOutput> {
        operations::create_group_prepare(&self.store, input).map_err(map_group_mls_error)
    }

    fn add_member_prepare(
        &self,
        input: AddMemberInput,
    ) -> crate::ImResult<PreparedMlsCommitOutput> {
        operations::add_member_prepare(&self.store, input).map_err(map_group_mls_error)
    }

    fn remove_member_prepare(
        &self,
        input: RemoveMemberInput,
    ) -> crate::ImResult<PreparedMlsCommitOutput> {
        operations::remove_member_prepare(&self.store, input).map_err(map_group_mls_error)
    }

    fn leave_prepare(&self, input: LeaveGroupInput) -> crate::ImResult<PreparedMlsCommitOutput> {
        operations::leave_prepare(&self.store, input).map_err(map_group_mls_error)
    }

    fn update_member_prepare(
        &self,
        input: UpdateMemberInput,
    ) -> crate::ImResult<PreparedMlsCommitOutput> {
        operations::update_member_prepare(&self.store, input).map_err(map_group_mls_error)
    }

    fn recover_member_prepare(
        &self,
        input: RecoverMemberInput,
    ) -> crate::ImResult<PreparedMlsCommitOutput> {
        operations::recover_member_prepare(&self.store, input).map_err(map_group_mls_error)
    }

    fn finalize_commit(&self, input: FinalizeCommitInput) -> crate::ImResult<FinalizeCommitOutput> {
        operations::finalize_commit(&self.store, input).map_err(map_group_mls_error)
    }

    fn abort_commit(&self, input: AbortCommitInput) -> crate::ImResult<AbortCommitOutput> {
        operations::abort_commit(&self.store, input).map_err(map_group_mls_error)
    }

    fn process_welcome(&self, input: ProcessWelcomeInput) -> crate::ImResult<ProcessWelcomeOutput> {
        operations::process_welcome(&self.store, input).map_err(map_group_mls_error)
    }

    fn process_notice(&self, input: ProcessNoticeInput) -> crate::ImResult<ProcessNoticeOutput> {
        operations::process_notice(&self.store, input).map_err(map_group_mls_error)
    }

    fn encrypt(&self, input: EncryptInput) -> crate::ImResult<EncryptOutput> {
        operations::encrypt(&self.store, input).map_err(map_group_mls_error)
    }

    fn decrypt(&self, input: DecryptInput) -> crate::ImResult<DecryptOutput> {
        operations::decrypt(&self.store, input).map_err(map_group_mls_error)
    }

    fn status(&self, input: StatusInput) -> crate::ImResult<StatusOutput> {
        operations::status(&self.store, input).map_err(map_group_mls_error)
    }
}
