mod dto;
mod service;

#[cfg(feature = "group-e2ee")]
pub(crate) use service::authoritative_group_e2ee_classification;

pub use dto::{
    GroupAdmissionMode, GroupCreateRequest, GroupDiscoverability, GroupInitialMemberRequest,
    GroupJoinRequest, GroupKeyPackagePublishRequest, GroupKeyPackagePublishResult,
    GroupKeyPackagePurpose, GroupLeaveRequest, GroupListRequest, GroupMember, GroupMemberLimit,
    GroupMemberMutationRequest, GroupMemberRef, GroupMemberResolution, GroupMemberRole,
    GroupMembersRequest, GroupMessageSecurityProfile, GroupMessagesRequest, GroupPolicyPatch,
    GroupProfilePatch, GroupReadResult, GroupSecurityRequirement, GroupSnapshot, GroupSummary,
    GroupUpdatePolicyRequest, GroupUpdateProfileRequest, GroupUpdateRequest, GroupUpdateResult,
};
pub use service::GroupService;
