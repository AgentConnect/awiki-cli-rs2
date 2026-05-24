use std::cell::RefCell;

use anp::group_e2ee::operations::{
    AbortCommitInput, AbortCommitOutput, AddMemberInput, CreateGroupInput, DecryptInput,
    DecryptOutput, EncryptInput, EncryptOutput, FinalizeCommitInput, FinalizeCommitOutput,
    GenerateKeyPackageInput, GroupKeyPackageOutput, LeaveGroupInput, PreparedMlsCommitOutput,
    ProcessNoticeInput, ProcessNoticeOutput, ProcessWelcomeInput, ProcessWelcomeOutput,
    RecoverMemberInput, RemoveMemberInput, StatusInput, StatusOutput, UpdateMemberInput,
};

use super::provider::GroupMlsProvider;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FakeGroupMlsCall {
    GenerateKeyPackage,
    CreateGroupPrepare,
    AddMemberPrepare,
    RemoveMemberPrepare,
    LeavePrepare,
    UpdateMemberPrepare,
    RecoverMemberPrepare,
    FinalizeCommit,
    AbortCommit,
    ProcessWelcome,
    ProcessNotice,
    Encrypt,
    Decrypt,
    Status,
}

#[derive(Default)]
pub(crate) struct FakeGroupMlsProvider {
    calls: RefCell<Vec<FakeGroupMlsCall>>,
}

impl FakeGroupMlsProvider {
    pub(crate) fn calls(&self) -> Vec<FakeGroupMlsCall> {
        self.calls.borrow().clone()
    }

    fn record_unsupported<T>(&self, call: FakeGroupMlsCall) -> crate::ImResult<T> {
        self.calls.borrow_mut().push(call);
        Err(crate::ImError::unsupported("group-e2ee-fake-provider"))
    }
}

impl GroupMlsProvider for FakeGroupMlsProvider {
    fn generate_key_package(
        &self,
        _input: GenerateKeyPackageInput,
    ) -> crate::ImResult<GroupKeyPackageOutput> {
        self.record_unsupported(FakeGroupMlsCall::GenerateKeyPackage)
    }

    fn create_group_prepare(
        &self,
        _input: CreateGroupInput,
    ) -> crate::ImResult<PreparedMlsCommitOutput> {
        self.record_unsupported(FakeGroupMlsCall::CreateGroupPrepare)
    }

    fn add_member_prepare(
        &self,
        _input: AddMemberInput,
    ) -> crate::ImResult<PreparedMlsCommitOutput> {
        self.record_unsupported(FakeGroupMlsCall::AddMemberPrepare)
    }

    fn remove_member_prepare(
        &self,
        _input: RemoveMemberInput,
    ) -> crate::ImResult<PreparedMlsCommitOutput> {
        self.record_unsupported(FakeGroupMlsCall::RemoveMemberPrepare)
    }

    fn leave_prepare(&self, _input: LeaveGroupInput) -> crate::ImResult<PreparedMlsCommitOutput> {
        self.record_unsupported(FakeGroupMlsCall::LeavePrepare)
    }

    fn update_member_prepare(
        &self,
        _input: UpdateMemberInput,
    ) -> crate::ImResult<PreparedMlsCommitOutput> {
        self.record_unsupported(FakeGroupMlsCall::UpdateMemberPrepare)
    }

    fn recover_member_prepare(
        &self,
        _input: RecoverMemberInput,
    ) -> crate::ImResult<PreparedMlsCommitOutput> {
        self.record_unsupported(FakeGroupMlsCall::RecoverMemberPrepare)
    }

    fn finalize_commit(
        &self,
        _input: FinalizeCommitInput,
    ) -> crate::ImResult<FinalizeCommitOutput> {
        self.record_unsupported(FakeGroupMlsCall::FinalizeCommit)
    }

    fn abort_commit(&self, _input: AbortCommitInput) -> crate::ImResult<AbortCommitOutput> {
        self.record_unsupported(FakeGroupMlsCall::AbortCommit)
    }

    fn process_welcome(
        &self,
        _input: ProcessWelcomeInput,
    ) -> crate::ImResult<ProcessWelcomeOutput> {
        self.record_unsupported(FakeGroupMlsCall::ProcessWelcome)
    }

    fn process_notice(&self, _input: ProcessNoticeInput) -> crate::ImResult<ProcessNoticeOutput> {
        self.record_unsupported(FakeGroupMlsCall::ProcessNotice)
    }

    fn encrypt(&self, _input: EncryptInput) -> crate::ImResult<EncryptOutput> {
        self.record_unsupported(FakeGroupMlsCall::Encrypt)
    }

    fn decrypt(&self, _input: DecryptInput) -> crate::ImResult<DecryptOutput> {
        self.record_unsupported(FakeGroupMlsCall::Decrypt)
    }

    fn status(&self, _input: StatusInput) -> crate::ImResult<StatusOutput> {
        self.record_unsupported(FakeGroupMlsCall::Status)
    }
}
