use crate::dto::{
    attachment::{
        DartAttachmentSendResult, DartDownloadedAttachment, DartDownloadedAttachmentDestination,
        DartUploadedAttachment,
    },
    auth::{DartAuthScope, DartAuthStatus, DartSessionBundle, DartSessionUpdate},
    directory::{DartDirectoryResolution, DartRelationStatus},
    email::{
        DartEmailAccount, DartEmailAttachmentContent, DartEmailAttachmentMetadata,
        DartEmailAttribute, DartEmailMarkReadResult, DartEmailMessage, DartEmailMessageSummary,
        DartEmailMessageSummaryPage, DartEmailNotification, DartEmailNotificationPage,
        DartSendEmailResult,
    },
    group::{
        DartGroupMember, DartGroupReadResult, DartGroupRebindRecoveryItem,
        DartGroupRebindRecoverySummary, DartGroupSnapshot, DartGroupSummary,
    },
    identity::{
        DartActiveSyncAccountBinding, DartAuthorizedJoinActivationProgress,
        DartDaemonSubkeyAuthorizationRevokeResult, DartDaemonSubkeyPublicPackage,
        DartDefaultIdentityChange, DartDeleteLocalIdentityResult, DartDeviceJoinApprovalPrompt,
        DartDeviceJoinAuthorizationStatus, DartDeviceJoinAuthorizedDeviceSummary,
        DartDeviceJoinPhase, DartDeviceJoinProgress, DartDeviceJoinRegistrySnapshot,
        DartDeviceJoinRemoteState, DartDeviceJoinRequestNotice, DartDeviceJoinRole,
        DartDeviceJoinSessionSummary, DartDeviceJoinSide,
        DartDeviceRegistryAuthorizedDeviceSummary, DartDeviceRevokeResult, DartDeviceRevokeStatus,
        DartHandleRecoveryAccountEpochReceipt, DartHandleRecoveryErrorCode,
        DartHandleRecoveryImpact, DartHandleRecoveryKeyState, DartHandleRecoveryOperationLifecycle,
        DartHandleRecoveryOperationSummary, DartHandleRecoveryOtpResult, DartHandleRecoveryPhase,
        DartHandleRecoveryProgress, DartHandleRecoveryResetReference,
        DartHandleRecoveryTransitionSourceKind, DartHandleRegistrationJoinMode,
        DartHandleRegistrationJoinRequiredPreparation, DartHandleRegistrationResult,
        DartIdentityCustodyBackend, DartIdentityCustodyState, DartIdentityCustodyStatus,
        DartIdentityDeviceMode, DartIdentityDeviceReadiness, DartIdentityDeviceRole,
        DartIdentityDeviceSummary, DartIdentitySecretStorageBackend, DartIdentitySummary,
        DartIdentityVaultMigrationReport, DartIdentityVaultStatus,
        DartIdentityVaultVerificationReport, DartLegacyRegistryEpochAdoptionAuthority,
        DartLegacyUpgradeStatus, DartRootKeyTransferError, DartRootKeyTransferPreparation,
        DartRootKeyTransferRecipientSummary, DartRootKeyTransferSendResult,
    },
    message::{
        DartCommittedIncomingMessage, DartCommittedMessageSource, DartConversation,
        DartConversationAlias, DartConversationAliasSource, DartConversationIdentity,
        DartConversationIdentityScope, DartConversationListSnapshot,
        DartConversationMigrationState, DartConversationPage, DartConversationResolutionState,
        DartConversationSnapshotItem, DartConversationSnapshotMessage,
        DartConversationSnapshotMessageBody, DartConversationStorageThreadRef,
        DartConversationStorePatch, DartMarkReadResult, DartMarkThreadReadResult, DartMessage,
        DartMessageBodyView, DartMessageDirection, DartMessageMetadata,
        DartMessageMetadataAttribute, DartMessagePage, DartMessageSyncDiagnostics,
        DartMessageSyncDirtyDomain, DartMessageSyncMode, DartMessageSyncOutcome,
        DartMessageSyncRetryState, DartMessageSyncStatus, DartReadWatermark, DartSendMessageResult,
        DartSyncDeltaResult, DartSyncThreadAfterResult, DartThreadMessageStorePatch,
    },
    profile::DartUserProfile,
    realtime::{DartRealtimeEvent, DartRealtimeStatus, DartRealtimeSyncHint, DartSyncDomain},
    secure::{
        DartDirectSecurePrepareResult, DartDirectSecureRepairResult, DartDirectSecureState,
        DartDirectSecureStatus, DartGroupSecureLocalReadiness, DartGroupSecurePendingWork,
        DartGroupSecurePrepareResult, DartGroupSecureRepairResult, DartGroupSecureState,
        DartGroupSecureStatus, DartSecureDelivery, DartSecureOutboxEntry, DartSecureOutboxResult,
        DartSecureOutboxStatus, DartSecureProblem, DartSecureProblemCode,
    },
};

impl From<im_core::identity::LegacyRegistryEpochAdoptionAuthority>
    for DartLegacyRegistryEpochAdoptionAuthority
{
    fn from(value: im_core::identity::LegacyRegistryEpochAdoptionAuthority) -> Self {
        Self {
            owner_identity_id: value.owner_identity_id.as_str().to_owned(),
            account_user_id: value.account_user_id,
            current_did: value.current_did.as_str().to_owned(),
            binding_generation: value.binding_generation,
            protocol_device_id: value.protocol_device_id.as_str().to_owned(),
            device_auth_generation: value.device_auth_generation,
            provenance_id: value.provenance_id,
        }
    }
}

impl From<im_core::identity::HandleRecoveryOtpResult> for DartHandleRecoveryOtpResult {
    fn from(value: im_core::identity::HandleRecoveryOtpResult) -> Self {
        Self {
            owner_identity_id: value.owner_identity_id.as_str().to_owned(),
            full_handle: value.full_handle,
            operation_id: value.operation_id,
            accepted: value.accepted,
            retry_after_seconds: value.retry_after_seconds,
            retry_at: value.retry_at,
        }
    }
}

impl From<im_core::identity::HandleRecoveryProgress> for DartHandleRecoveryProgress {
    fn from(value: im_core::identity::HandleRecoveryProgress) -> Self {
        Self {
            operation_id: value.operation_id,
            owner_identity_id: value.owner_identity_id.as_str().to_owned(),
            account_user_id: value.account_user_id,
            full_handle: value.full_handle,
            local_previous_did: value.local_previous_did.map(|did| did.as_str().to_owned()),
            current_did: value.current_did.as_str().to_owned(),
            binding_generation: value.binding_generation,
            state_root_fingerprint: value.state_root_fingerprint,
            phase: match value.phase {
                im_core::identity::HandleRecoveryPhase::AwaitingFactor => {
                    DartHandleRecoveryPhase::AwaitingFactor
                }
                im_core::identity::HandleRecoveryPhase::ReadyToCommit => {
                    DartHandleRecoveryPhase::ReadyToCommit
                }
                im_core::identity::HandleRecoveryPhase::RemoteOutcomeUnknown => {
                    DartHandleRecoveryPhase::RemoteOutcomeUnknown
                }
                im_core::identity::HandleRecoveryPhase::RemoteCommitted => {
                    DartHandleRecoveryPhase::RemoteCommitted
                }
                im_core::identity::HandleRecoveryPhase::IdentityTransitionPending => {
                    DartHandleRecoveryPhase::IdentityTransitionPending
                }
                im_core::identity::HandleRecoveryPhase::Applied => DartHandleRecoveryPhase::Applied,
                im_core::identity::HandleRecoveryPhase::QuarantinedKeyUnavailable => {
                    DartHandleRecoveryPhase::QuarantinedKeyUnavailable
                }
            },
            impact: value.impact.into(),
            reset_reference: value.reset_reference.map(Into::into),
            failure_code: value.failure_code.map(Into::into),
        }
    }
}

impl From<im_core::identity::HandleRecoveryImpact> for DartHandleRecoveryImpact {
    fn from(value: im_core::identity::HandleRecoveryImpact) -> Self {
        Self {
            local_ordinary_data_will_migrate: value.local_ordinary_data_will_migrate,
            other_devices_must_rejoin: value.other_devices_must_rejoin,
            unsupported_e2ee_group_count: value.unsupported_e2ee_group_count,
            unsupported_did_only_group_count: value.unsupported_did_only_group_count,
        }
    }
}

impl From<im_core::identity::HandleRecoveryResetReference> for DartHandleRecoveryResetReference {
    fn from(value: im_core::identity::HandleRecoveryResetReference) -> Self {
        Self {
            account_user_id: value.account_user_id,
            owner_identity_id: value.owner_identity_id,
            previous_did: value.previous_did.as_str().to_owned(),
            current_did: value.current_did.as_str().to_owned(),
            binding_generation: value.binding_generation,
            handle: value.handle,
            source_kind: match value.source_kind {
                im_core::identity::HandleRecoveryTransitionSourceKind::Initiator => {
                    DartHandleRecoveryTransitionSourceKind::Initiator
                }
                im_core::identity::HandleRecoveryTransitionSourceKind::JoinedDevice => {
                    DartHandleRecoveryTransitionSourceKind::JoinedDevice
                }
            },
            source_id: value.source_id,
        }
    }
}

impl From<im_core::identity::AuthorizedJoinActivationProgress>
    for DartAuthorizedJoinActivationProgress
{
    fn from(value: im_core::identity::AuthorizedJoinActivationProgress) -> Self {
        Self {
            join: value.join.into(),
            reset_reference: value.reset_reference.map(Into::into),
        }
    }
}

impl From<im_core::identity::HandleRecoveryErrorCode> for DartHandleRecoveryErrorCode {
    fn from(value: im_core::identity::HandleRecoveryErrorCode) -> Self {
        match value {
            im_core::identity::HandleRecoveryErrorCode::FactorRetryRequired => {
                Self::FactorRetryRequired
            }
            im_core::identity::HandleRecoveryErrorCode::ResultAbsent => Self::ResultAbsent,
            im_core::identity::HandleRecoveryErrorCode::OutcomeUnknown => Self::OutcomeUnknown,
            im_core::identity::HandleRecoveryErrorCode::LocalKeyUnavailable => {
                Self::LocalKeyUnavailable
            }
            im_core::identity::HandleRecoveryErrorCode::LocalTransitionPending => {
                Self::LocalTransitionPending
            }
            im_core::identity::HandleRecoveryErrorCode::LocalMigrationUnsupported => {
                Self::LocalMigrationUnsupported
            }
            im_core::identity::HandleRecoveryErrorCode::UnknownEpoch => Self::UnknownEpoch,
        }
    }
}

impl From<im_core::identity::HandleRecoveryOperationSummary>
    for DartHandleRecoveryOperationSummary
{
    fn from(value: im_core::identity::HandleRecoveryOperationSummary) -> Self {
        Self {
            operation_id: value.operation_id,
            owner_identity_id: value.owner_identity_id.as_str().to_owned(),
            account_user_id: value.account_user_id,
            full_handle: value.full_handle,
            lifecycle_class: match value.lifecycle_class {
                im_core::identity::HandleRecoveryOperationLifecycle::PreCommit => {
                    DartHandleRecoveryOperationLifecycle::PreCommit
                }
                im_core::identity::HandleRecoveryOperationLifecycle::RemoteUnresolved => {
                    DartHandleRecoveryOperationLifecycle::RemoteUnresolved
                }
                im_core::identity::HandleRecoveryOperationLifecycle::RemoteCommitted => {
                    DartHandleRecoveryOperationLifecycle::RemoteCommitted
                }
                im_core::identity::HandleRecoveryOperationLifecycle::LocalTransitionPending => {
                    DartHandleRecoveryOperationLifecycle::LocalTransitionPending
                }
                im_core::identity::HandleRecoveryOperationLifecycle::Applied => {
                    DartHandleRecoveryOperationLifecycle::Applied
                }
                im_core::identity::HandleRecoveryOperationLifecycle::DiscardedPreAttempt => {
                    DartHandleRecoveryOperationLifecycle::DiscardedPreAttempt
                }
                im_core::identity::HandleRecoveryOperationLifecycle::QuarantinedKeyUnavailable => {
                    DartHandleRecoveryOperationLifecycle::QuarantinedKeyUnavailable
                }
                im_core::identity::HandleRecoveryOperationLifecycle::SupersededByStateChange => {
                    DartHandleRecoveryOperationLifecycle::SupersededByStateChange
                }
                im_core::identity::HandleRecoveryOperationLifecycle::FailedTerminal => {
                    DartHandleRecoveryOperationLifecycle::FailedTerminal
                }
            },
            commit_attempted: value.commit_attempted,
            key_state: match value.key_state {
                im_core::identity::HandleRecoveryKeyState::Available => {
                    DartHandleRecoveryKeyState::Available
                }
                im_core::identity::HandleRecoveryKeyState::TemporarilyLocked => {
                    DartHandleRecoveryKeyState::TemporarilyLocked
                }
                im_core::identity::HandleRecoveryKeyState::PermanentlyUnavailable => {
                    DartHandleRecoveryKeyState::PermanentlyUnavailable
                }
                im_core::identity::HandleRecoveryKeyState::DestroyedPreAttempt => {
                    DartHandleRecoveryKeyState::DestroyedPreAttempt
                }
            },
            intent_hash: value.intent_hash,
            state_root_fingerprint: value.state_root_fingerprint,
            superseded_by_operation_id: value.superseded_by_operation_id,
            last_error_code: value.last_error_code,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

impl From<im_core::identity::HandleRecoveryAccountEpochReceipt>
    for DartHandleRecoveryAccountEpochReceipt
{
    fn from(value: im_core::identity::HandleRecoveryAccountEpochReceipt) -> Self {
        Self {
            receipt_schema_version: value.receipt_schema_version,
            source_kind: match value.source_kind {
                im_core::identity::HandleRecoveryTransitionSourceKind::Initiator => {
                    DartHandleRecoveryTransitionSourceKind::Initiator
                }
                im_core::identity::HandleRecoveryTransitionSourceKind::JoinedDevice => {
                    DartHandleRecoveryTransitionSourceKind::JoinedDevice
                }
            },
            source_id: value.source_id,
            account_user_id: value.account_user_id,
            owner_identity_id: value.owner_identity_id.as_str().to_owned(),
            full_handle: value.full_handle,
            local_previous_did: value.local_previous_did.as_str().to_owned(),
            current_did: value.current_did.as_str().to_owned(),
            binding_generation: value.binding_generation,
            current_device_id: value.current_device_id.as_str().to_owned(),
            device_auth_generation: value.device_auth_generation,
            registry_version: value.registry_version,
            state_root_fingerprint: value.state_root_fingerprint,
            applied_at: value.applied_at,
            metadata_json: value.metadata_json,
        }
    }
}

impl From<im_core::identity::DeviceRevokeResult> for DartDeviceRevokeResult {
    fn from(value: im_core::identity::DeviceRevokeResult) -> Self {
        Self {
            did: value.did.as_str().to_owned(),
            target_device_id: value.target_device_id.as_str().to_owned(),
            status: match value.status {
                im_core::identity::DeviceRevokeStatus::Revoked => DartDeviceRevokeStatus::Revoked,
            },
        }
    }
}

impl From<im_core::identity::RootKeyTransferSendResult> for DartRootKeyTransferSendResult {
    fn from(value: im_core::identity::RootKeyTransferSendResult) -> Self {
        Self {
            did: value.did.as_str().to_owned(),
            sender_device_id: value.sender_device_id.as_str().to_owned(),
            recipient_device_id: value.recipient_device_id.as_str().to_owned(),
            message_id: value.message_id.as_str().to_owned(),
            accepted_at: value.accepted_at,
        }
    }
}

impl From<im_core::identity::RootKeyTransferPreparation> for DartRootKeyTransferPreparation {
    fn from(value: im_core::identity::RootKeyTransferPreparation) -> Self {
        let serde_json::Value::String(authorization_handle) =
            serde_json::to_value(value.authorization_handle)
                .expect("root transfer authorization handle serialization is infallible")
        else {
            unreachable!("root transfer authorization handle must serialize as a string")
        };
        Self {
            authorization_handle,
            recipient: value.recipient.into(),
            expires_at: value.expires_at,
        }
    }
}

impl From<im_core::identity::RootKeyTransferRecipientSummary>
    for DartRootKeyTransferRecipientSummary
{
    fn from(value: im_core::identity::RootKeyTransferRecipientSummary) -> Self {
        Self {
            did: value.did.as_str().to_owned(),
            device_id: value.device_id.as_str().to_owned(),
            signing_key_id: value.signing_key_id,
            e2ee_key_id: value.e2ee_key_id,
            registry_version: value.registry_version,
        }
    }
}

impl From<im_core::identity::RootKeyTransferError> for DartRootKeyTransferError {
    fn from(value: im_core::identity::RootKeyTransferError) -> Self {
        Self {
            code: value.to_string(),
            retryable: value.retryable,
        }
    }
}

impl DartRootKeyTransferError {
    pub(crate) fn invalid_request() -> Self {
        Self {
            code: "root_transfer.invalid_request".to_owned(),
            retryable: false,
        }
    }

    pub(crate) fn authorization_invalid() -> Self {
        Self {
            code: "root_transfer.authorization_invalid".to_owned(),
            retryable: false,
        }
    }

    pub(crate) fn temporarily_unavailable() -> Self {
        Self {
            code: "root_transfer.temporarily_unavailable".to_owned(),
            retryable: true,
        }
    }
}

impl From<im_core::identity::DeviceJoinSessionView> for DartDeviceJoinSessionSummary {
    fn from(value: im_core::identity::DeviceJoinSessionView) -> Self {
        Self {
            join_session_id: value.join_session_id,
            did: value.did.as_str().to_owned(),
            protocol_device_id: value.protocol_device_id.as_str().to_owned(),
            side: match value.side {
                im_core::identity::DeviceJoinSide::NewDevice => DartDeviceJoinSide::NewDevice,
                im_core::identity::DeviceJoinSide::Admin => DartDeviceJoinSide::Admin,
            },
            phase: match value.phase {
                im_core::identity::DeviceJoinLocalPhase::Pending => DartDeviceJoinPhase::Pending,
                im_core::identity::DeviceJoinLocalPhase::ChallengePrepared => {
                    DartDeviceJoinPhase::ChallengePrepared
                }
                im_core::identity::DeviceJoinLocalPhase::ResponsePrepared => {
                    DartDeviceJoinPhase::ResponsePrepared
                }
                im_core::identity::DeviceJoinLocalPhase::ResponseVerified => {
                    DartDeviceJoinPhase::ResponseVerified
                }
                im_core::identity::DeviceJoinLocalPhase::ApprovalPrepared => {
                    DartDeviceJoinPhase::ApprovalPrepared
                }
                im_core::identity::DeviceJoinLocalPhase::Authorized => {
                    DartDeviceJoinPhase::Authorized
                }
                im_core::identity::DeviceJoinLocalPhase::Cancelled => {
                    DartDeviceJoinPhase::Cancelled
                }
                im_core::identity::DeviceJoinLocalPhase::Expired => DartDeviceJoinPhase::Expired,
            },
            expires_at: value.expires_at,
        }
    }
}

impl From<im_core::identity::DeviceJoinAuthorizedDeviceSummary>
    for DartDeviceJoinAuthorizedDeviceSummary
{
    fn from(value: im_core::identity::DeviceJoinAuthorizedDeviceSummary) -> Self {
        Self {
            protocol_device_id: value.protocol_device_id.as_str().to_owned(),
            signing_key_id: value.signing_key_id,
            e2ee_key_id: value.e2ee_key_id,
            status: match value.status {
                im_core::identity::DeviceJoinAuthorizationStatus::Active => {
                    DartDeviceJoinAuthorizationStatus::Active
                }
                im_core::identity::DeviceJoinAuthorizationStatus::Revoked => {
                    DartDeviceJoinAuthorizationStatus::Revoked
                }
            },
            role: dart_device_join_role(value.role),
            management_ready: value.management_ready,
            is_current: value.is_current,
        }
    }
}

impl From<im_core::identity::DeviceRegistryAuthorizedDeviceSummary>
    for DartDeviceRegistryAuthorizedDeviceSummary
{
    fn from(value: im_core::identity::DeviceRegistryAuthorizedDeviceSummary) -> Self {
        Self {
            protocol_device_id: value.protocol_device_id.as_str().to_owned(),
            signing_key_id: value.signing_key_id,
            e2ee_key_id: value.e2ee_key_id,
            status: match value.status {
                im_core::identity::DeviceJoinAuthorizationStatus::Active => {
                    DartDeviceJoinAuthorizationStatus::Active
                }
                im_core::identity::DeviceJoinAuthorizationStatus::Revoked => {
                    DartDeviceJoinAuthorizationStatus::Revoked
                }
            },
            role: dart_device_join_role(value.role),
            management_ready: value.management_ready,
            is_current: value.is_current,
            auth_generation: value.auth_generation,
        }
    }
}

impl From<im_core::identity::DeviceJoinRequestNotice> for DartDeviceJoinRequestNotice {
    fn from(value: im_core::identity::DeviceJoinRequestNotice) -> Self {
        Self {
            event_id: value.event_id,
            join_session_id: value.join_session_id,
            did: value.did.as_str().to_owned(),
            protocol_device_id: value.protocol_device_id.as_str().to_owned(),
            candidate_key_fingerprint: value.candidate_key_fingerprint,
            issued_at: value.issued_at,
            expires_at: value.expires_at,
            state: dart_device_join_remote_state(value.state),
            claimed_by_current_device: value.claimed_by_current_device,
            can_start_verification: value.can_start_verification,
        }
    }
}

impl From<im_core::identity::DeviceJoinRegistrySnapshot> for DartDeviceJoinRegistrySnapshot {
    fn from(value: im_core::identity::DeviceJoinRegistrySnapshot) -> Self {
        Self {
            did: value.did.as_str().to_owned(),
            registry_version: value.registry_version,
            devices: value.devices.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<im_core::identity::DeviceJoinProgress> for DartDeviceJoinProgress {
    fn from(value: im_core::identity::DeviceJoinProgress) -> Self {
        Self {
            session: value.session.into(),
            remote_state: dart_device_join_remote_state(value.remote_state),
            sas: value.sas,
            authorized_device: value.authorized_device.map(Into::into),
        }
    }
}

impl From<im_core::identity::DeviceJoinApprovalPrompt> for DartDeviceJoinApprovalPrompt {
    fn from(value: im_core::identity::DeviceJoinApprovalPrompt) -> Self {
        Self {
            approval_handle: value.approval_handle,
            join_session_id: value.join_session_id,
            sas: value.sas,
            expires_at: value.expires_at,
        }
    }
}

fn dart_device_join_remote_state(
    value: im_core::identity::DeviceJoinRemoteState,
) -> DartDeviceJoinRemoteState {
    match value {
        im_core::identity::DeviceJoinRemoteState::Pending => DartDeviceJoinRemoteState::Pending,
        im_core::identity::DeviceJoinRemoteState::ChallengeSent => {
            DartDeviceJoinRemoteState::ChallengeSent
        }
        im_core::identity::DeviceJoinRemoteState::ResponseVerified => {
            DartDeviceJoinRemoteState::ResponseVerified
        }
        im_core::identity::DeviceJoinRemoteState::Consumed => DartDeviceJoinRemoteState::Consumed,
        im_core::identity::DeviceJoinRemoteState::Cancelled => DartDeviceJoinRemoteState::Cancelled,
        im_core::identity::DeviceJoinRemoteState::Rejected => DartDeviceJoinRemoteState::Rejected,
        im_core::identity::DeviceJoinRemoteState::Expired => DartDeviceJoinRemoteState::Expired,
    }
}

fn dart_device_join_role(value: im_core::identity::DeviceJoinRole) -> DartDeviceJoinRole {
    match value {
        im_core::identity::DeviceJoinRole::Member => DartDeviceJoinRole::Member,
        im_core::identity::DeviceJoinRole::Admin => DartDeviceJoinRole::Admin,
    }
}

impl From<im_core::identity::IdentitySummary> for DartIdentitySummary {
    fn from(value: im_core::identity::IdentitySummary) -> Self {
        Self {
            id: value.id.as_str().to_string(),
            did: value.did.as_str().to_string(),
            handle: value.handle.map(|handle| handle.as_str().to_string()),
            display_name: value.display_name,
            local_alias: value.local_alias,
            device_id: value.device_id,
            is_default: value.is_default,
            ready_for_auth: value.readiness.ready_for_auth,
            ready_for_messaging: value.readiness.ready_for_messaging,
            missing: value
                .readiness
                .missing
                .into_iter()
                .map(identity_missing_item_to_string)
                .collect(),
        }
    }
}

impl From<im_core::identity::ActiveSyncAccountBinding> for DartActiveSyncAccountBinding {
    fn from(value: im_core::identity::ActiveSyncAccountBinding) -> Self {
        Self {
            owner_identity_id: value.owner_identity_id,
            account_id: value.account_id,
            current_did: value.current_did,
            protocol_device_id: value.protocol_device_id,
            identity_generation: value.identity_generation,
            device_auth_generation: value.device_auth_generation,
        }
    }
}

impl From<im_core::identity::IdentityDeviceSummary> for DartIdentityDeviceSummary {
    fn from(value: im_core::identity::IdentityDeviceSummary) -> Self {
        Self {
            identity: value.identity.into(),
            mode: match value.mode {
                im_core::identity::IdentityDeviceMode::Legacy => DartIdentityDeviceMode::Legacy,
                im_core::identity::IdentityDeviceMode::VNext => DartIdentityDeviceMode::VNext,
            },
            protocol_device_id: value
                .protocol_device_id
                .map(|device_id| device_id.as_str().to_owned()),
            role: value.role.map(|role| match role {
                im_core::identity::IdentityDeviceRole::Member => DartIdentityDeviceRole::Member,
                im_core::identity::IdentityDeviceRole::Admin => DartIdentityDeviceRole::Admin,
            }),
            signing_key_id: value.signing_key_id,
            e2ee_key_id: value.e2ee_key_id,
            readiness: match value.readiness {
                im_core::identity::IdentityDeviceReadiness::Legacy => {
                    DartIdentityDeviceReadiness::Legacy
                }
                im_core::identity::IdentityDeviceReadiness::MemberReady => {
                    DartIdentityDeviceReadiness::MemberReady
                }
                im_core::identity::IdentityDeviceReadiness::AdminAwaitingRoot => {
                    DartIdentityDeviceReadiness::AdminAwaitingRoot
                }
                im_core::identity::IdentityDeviceReadiness::AdminReady => {
                    DartIdentityDeviceReadiness::AdminReady
                }
                im_core::identity::IdentityDeviceReadiness::Blocked => {
                    DartIdentityDeviceReadiness::Blocked
                }
            },
            blocked_reason: value.blocked_reason,
        }
    }
}

impl From<im_core::IdentitySecretStoragePolicy>
    for crate::dto::config::DartIdentitySecretStoragePolicy
{
    fn from(value: im_core::IdentitySecretStoragePolicy) -> Self {
        match value {
            im_core::IdentitySecretStoragePolicy::FileCompat => Self::FileCompat,
            im_core::IdentitySecretStoragePolicy::VaultPreferred => Self::VaultPreferred,
            im_core::IdentitySecretStoragePolicy::VaultRequired => Self::VaultRequired,
        }
    }
}

impl From<im_core::identity::IdentityCustodyBackend> for DartIdentityCustodyBackend {
    fn from(value: im_core::identity::IdentityCustodyBackend) -> Self {
        match value {
            im_core::identity::IdentityCustodyBackend::AnpIdentity => Self::AnpIdentity,
            im_core::identity::IdentityCustodyBackend::LegacyFileCompat => Self::LegacyFileCompat,
            im_core::identity::IdentityCustodyBackend::LegacyVault => Self::LegacyVault,
        }
    }
}

impl From<im_core::identity::IdentityCustodyState> for DartIdentityCustodyState {
    fn from(value: im_core::identity::IdentityCustodyState) -> Self {
        match value {
            im_core::identity::IdentityCustodyState::Creating => Self::Creating,
            im_core::identity::IdentityCustodyState::Active => Self::Active,
            im_core::identity::IdentityCustodyState::Enrolling => Self::Enrolling,
            im_core::identity::IdentityCustodyState::Revoked => Self::Revoked,
            im_core::identity::IdentityCustodyState::Legacy => Self::Legacy,
            im_core::identity::IdentityCustodyState::Unavailable => Self::Unavailable,
        }
    }
}

impl From<im_core::identity::IdentityCustodyStatus> for DartIdentityCustodyStatus {
    fn from(value: im_core::identity::IdentityCustodyStatus) -> Self {
        Self {
            identity: value.identity.into(),
            backend: value.backend.into(),
            state: value.state.into(),
            ready: value.ready,
            root_control_available: value.root_control_available,
            pending_operation: value.pending_operation,
            store_id: value.store_id,
            custody_identity_id: value.custody_identity_id,
            missing: value.missing,
            warnings: value.warnings,
        }
    }
}

impl From<im_core::identity::IdentitySecretStorageBackend> for DartIdentitySecretStorageBackend {
    fn from(value: im_core::identity::IdentitySecretStorageBackend) -> Self {
        match value {
            im_core::identity::IdentitySecretStorageBackend::FileCompat => Self::FileCompat,
            im_core::identity::IdentitySecretStorageBackend::Vault => Self::Vault,
        }
    }
}

impl From<im_core::identity::IdentityVaultStatus> for DartIdentityVaultStatus {
    fn from(value: im_core::identity::IdentityVaultStatus) -> Self {
        Self {
            identity: value.identity.into(),
            storage_policy: value.storage_policy.into(),
            selected_backend: value.selected_backend.into(),
            vault_available: value.vault_available,
            vault_metadata_present: value.vault_metadata_present,
            vault_metadata_verified: value.vault_metadata_verified,
            workspace_id: value.workspace_id,
            device_id: value.device_id,
            plaintext_compat_retained: value.plaintext_compat_retained,
            missing: value.missing,
            warnings: value.warnings,
        }
    }
}

impl From<im_core::identity::LegacyUpgradeStatus> for DartLegacyUpgradeStatus {
    fn from(value: im_core::identity::LegacyUpgradeStatus) -> Self {
        match value {
            im_core::identity::LegacyUpgradeStatus::Idle => Self::Idle,
            im_core::identity::LegacyUpgradeStatus::Running => Self::Running,
            im_core::identity::LegacyUpgradeStatus::RetryRequired { identity_id, code } => {
                Self::RetryRequired { identity_id, code }
            }
            im_core::identity::LegacyUpgradeStatus::Completed => Self::Completed,
        }
    }
}

impl From<im_core::identity::IdentityVaultMigrationReport> for DartIdentityVaultMigrationReport {
    fn from(value: im_core::identity::IdentityVaultMigrationReport) -> Self {
        Self {
            identity: value.identity.into(),
            status: value.status.into(),
            migrated: value.migrated,
            verified: value.verified,
            plaintext_compat_retained: value.plaintext_compat_retained,
            warnings: value.warnings,
        }
    }
}

impl From<im_core::identity::IdentityVaultVerificationReport>
    for DartIdentityVaultVerificationReport
{
    fn from(value: im_core::identity::IdentityVaultVerificationReport) -> Self {
        Self {
            identity: value.identity.into(),
            status: value.status.into(),
            verified: value.verified,
            warnings: value.warnings,
        }
    }
}

impl From<im_core::email::EmailAttribute> for DartEmailAttribute {
    fn from(value: im_core::email::EmailAttribute) -> Self {
        Self {
            key: value.key,
            value: value.value,
        }
    }
}

impl From<im_core::email::EmailAccount> for DartEmailAccount {
    fn from(value: im_core::email::EmailAccount) -> Self {
        Self {
            mailbox_address: value
                .mailbox_address
                .map(|address| address.as_str().to_string()),
            display_name: value.display_name,
            status: value.status,
            attributes: value.attributes.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<im_core::email::EmailMessageSummary> for DartEmailMessageSummary {
    fn from(value: im_core::email::EmailMessageSummary) -> Self {
        Self {
            id: value.id.as_str().to_string(),
            folder: value.folder.map(|folder| folder.as_str().to_string()),
            from: value
                .from
                .into_iter()
                .map(|address| address.as_str().to_string())
                .collect(),
            to: value
                .to
                .into_iter()
                .map(|address| address.as_str().to_string())
                .collect(),
            cc: value
                .cc
                .into_iter()
                .map(|address| address.as_str().to_string())
                .collect(),
            subject: value.subject,
            preview: value.preview,
            received_at: value.received_at,
            sent_at: value.sent_at,
            unread: value.unread,
            has_attachments: value.has_attachments,
            attachment_count: value.attachment_count,
            attributes: value.attributes.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<im_core::ids::Page<im_core::email::EmailMessageSummary>> for DartEmailMessageSummaryPage {
    fn from(value: im_core::ids::Page<im_core::email::EmailMessageSummary>) -> Self {
        Self {
            items: value.items.into_iter().map(Into::into).collect(),
            next_cursor: value.next_cursor.map(|cursor| cursor.as_str().to_string()),
            has_more: value.has_more,
        }
    }
}

impl From<im_core::email::EmailAttachmentMetadata> for DartEmailAttachmentMetadata {
    fn from(value: im_core::email::EmailAttachmentMetadata) -> Self {
        Self {
            index: value.index,
            filename: value.filename,
            content_type: value.content_type,
            size: value.size,
        }
    }
}

impl From<im_core::email::EmailMessage> for DartEmailMessage {
    fn from(value: im_core::email::EmailMessage) -> Self {
        Self {
            summary: value.summary.into(),
            body_text: value.body_text,
            body_html: value.body_html,
            attachments: value.attachments.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<im_core::email::EmailAttachmentContent> for DartEmailAttachmentContent {
    fn from(value: im_core::email::EmailAttachmentContent) -> Self {
        Self {
            message_id: value.message_id.as_str().to_string(),
            attachment_index: value.attachment_index,
            filename: value.filename,
            content_type: value.content_type,
            size: value.size,
            bytes: value.bytes,
        }
    }
}

impl From<im_core::email::EmailMarkReadResult> for DartEmailMarkReadResult {
    fn from(value: im_core::email::EmailMarkReadResult) -> Self {
        Self {
            updated: value.updated,
        }
    }
}

impl From<im_core::email::SendEmailResult> for DartSendEmailResult {
    fn from(value: im_core::email::SendEmailResult) -> Self {
        Self {
            accepted: value.accepted,
            message_id: value.message_id.map(|id| id.as_str().to_string()),
            warnings: value.warnings,
        }
    }
}

impl From<im_core::email::EmailNotification> for DartEmailNotification {
    fn from(value: im_core::email::EmailNotification) -> Self {
        Self {
            id: value.id.as_str().to_string(),
            mailbox_address: value
                .mailbox_address
                .map(|address| address.as_str().to_string()),
            from_addr: value.from_addr,
            subject: value.subject,
            preview: value.preview,
            has_attachments: value.has_attachments,
            received_at: value.received_at,
            attributes: value.attributes.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<im_core::ids::Page<im_core::email::EmailNotification>> for DartEmailNotificationPage {
    fn from(value: im_core::ids::Page<im_core::email::EmailNotification>) -> Self {
        Self {
            items: value.items.into_iter().map(Into::into).collect(),
            next_cursor: value.next_cursor.map(|cursor| cursor.as_str().to_string()),
            has_more: value.has_more,
        }
    }
}

impl From<im_core::identity::DefaultIdentityChange> for DartDefaultIdentityChange {
    fn from(value: im_core::identity::DefaultIdentityChange) -> Self {
        Self {
            previous: value.previous.map(Into::into),
            next: value.next.into(),
            requires_default_identity_write: value.requires_default_identity_write,
            warnings: value.warnings,
        }
    }
}

impl From<im_core::identity::DeleteLocalIdentityResult> for DartDeleteLocalIdentityResult {
    fn from(value: im_core::identity::DeleteLocalIdentityResult) -> Self {
        Self {
            deleted: value.deleted.into(),
            was_default: value.was_default,
            next_default: value.next_default.map(Into::into),
            warnings: value.warnings,
        }
    }
}

impl From<im_core::identity::DaemonSubkeyPublicPackage> for DartDaemonSubkeyPublicPackage {
    fn from(value: im_core::identity::DaemonSubkeyPublicPackage) -> Self {
        Self {
            schema: value.schema,
            user_did: value.user_did.as_str().to_owned(),
            verification_method: value.verification_method,
            key_type: value.key_type,
            key_algorithm: value.key_algorithm,
            public_key_multibase: value.public_key_multibase,
        }
    }
}

impl From<im_core::identity::DaemonSubkeyAuthorizationRevokeResult>
    for DartDaemonSubkeyAuthorizationRevokeResult
{
    fn from(value: im_core::identity::DaemonSubkeyAuthorizationRevokeResult) -> Self {
        Self {
            user_did: value.user_did.as_str().to_string(),
            verification_method: value.verification_method,
            updated: value.updated,
        }
    }
}

impl From<im_core::identity::HandleRegistrationResult> for DartHandleRegistrationResult {
    fn from(value: im_core::identity::HandleRegistrationResult) -> Self {
        Self {
            identity: value.identity.map(Into::into),
            account_id: value.account_id,
            handle: value.handle.as_str().to_string(),
            method: registration_method_to_string(value.method),
            state: registration_state_to_string(value.state),
            join_required: value.join_required.map(Into::into),
            default_identity_change: value.default_identity_change.map(Into::into),
            warnings: value.warnings,
        }
    }
}

fn registration_method_to_string(value: im_core::identity::RegistrationMethod) -> String {
    match value {
        im_core::identity::RegistrationMethod::Phone => "phone".to_string(),
        im_core::identity::RegistrationMethod::Email => "email".to_string(),
        im_core::identity::RegistrationMethod::AlreadyVerified => "already_verified".to_string(),
    }
}

fn registration_state_to_string(value: im_core::identity::HandleRegistrationState) -> String {
    match value {
        im_core::identity::HandleRegistrationState::OtpSent => "otp_sent".to_string(),
        im_core::identity::HandleRegistrationState::EmailSent => "email_sent".to_string(),
        im_core::identity::HandleRegistrationState::EmailPending => "email_pending".to_string(),
        im_core::identity::HandleRegistrationState::Registered => "registered".to_string(),
        im_core::identity::HandleRegistrationState::JoinRequired => "join_required".to_string(),
    }
}

impl From<im_core::identity::HandleRegistrationJoinRequiredPreparation>
    for DartHandleRegistrationJoinRequiredPreparation
{
    fn from(value: im_core::identity::HandleRegistrationJoinRequiredPreparation) -> Self {
        Self {
            preparation_id: value.preparation_id,
            mode: match value.mode {
                im_core::identity::HandleRegistrationJoinMode::Ordinary => {
                    DartHandleRegistrationJoinMode::Ordinary
                }
                im_core::identity::HandleRegistrationJoinMode::HandleRecoveryRebind => {
                    DartHandleRegistrationJoinMode::HandleRecoveryRebind
                }
            },
            requires_user_presence: value.requires_user_presence,
            expected_did: value.expected_did.as_str().to_owned(),
            full_handle: value.full_handle.as_str().to_owned(),
        }
    }
}

fn identity_missing_item_to_string(value: im_core::identity::IdentityMissingItem) -> String {
    match value {
        im_core::identity::IdentityMissingItem::DidDocument => "did_document".to_string(),
        im_core::identity::IdentityMissingItem::PrivateKey => "private_key".to_string(),
        im_core::identity::IdentityMissingItem::AuthState => "auth_state".to_string(),
        im_core::identity::IdentityMissingItem::Handle => "handle".to_string(),
        im_core::identity::IdentityMissingItem::MessageEndpoint => "message_endpoint".to_string(),
        im_core::identity::IdentityMissingItem::Other(value) => value,
    }
}

impl From<im_core::auth::AuthScope> for DartAuthScope {
    fn from(value: im_core::auth::AuthScope) -> Self {
        match value {
            im_core::auth::AuthScope::UserProfile => Self::UserProfile,
            im_core::auth::AuthScope::Messaging => Self::Messaging,
            im_core::auth::AuthScope::GroupMessaging => Self::GroupMessaging,
        }
    }
}

impl From<im_core::auth::AuthStatus> for DartAuthStatus {
    fn from(value: im_core::auth::AuthStatus) -> Self {
        Self {
            subject: value.subject.as_str().to_string(),
            has_session: value.has_session,
            expires_at: value.expires_at,
            needs_refresh: value.needs_refresh,
            warnings: value.warnings,
        }
    }
}

impl From<im_core::auth::SessionBundle> for DartSessionBundle {
    fn from(value: im_core::auth::SessionBundle) -> Self {
        Self {
            subject: value.subject.as_str().to_string(),
            scope: value.scope.into(),
            expires_at: value.expires_at,
            refreshed: value.refreshed,
            bearer_token: value.bearer_token,
        }
    }
}

impl From<im_core::auth::SessionUpdate> for DartSessionUpdate {
    fn from(value: im_core::auth::SessionUpdate) -> Self {
        Self {
            subject: value.subject.as_str().to_string(),
            previous_expires_at: value.previous_expires_at,
            new_expires_at: value.new_expires_at,
            refreshed: value.refreshed,
            bearer_token: value.bearer_token,
        }
    }
}

impl From<im_core::identity::Profile> for DartUserProfile {
    fn from(value: im_core::identity::Profile) -> Self {
        let full_handle = value.handle.map(|handle| handle.as_str().to_string());
        Self {
            subject: value.subject.as_str().to_string(),
            handle: full_handle.clone(),
            full_handle,
            display_name: value.display_name,
            bio: value.bio,
            description: value.description,
            tags: value.tags,
            markdown: value.markdown,
            avatar_uri: value.avatar_uri,
            avatar_url: value.avatar_url,
            profile_uri: value.profile_uri,
            subject_type: value.subject_type,
            agent_kind: value.agent_kind,
            agent_capabilities: value.agent_capabilities,
            updated_at: value.updated_at,
            profile_version: value.profile_version,
            version_id: value.version_id,
            ttl: value.ttl,
        }
    }
}

impl From<im_core::directory::PublicProfile> for DartUserProfile {
    fn from(value: im_core::directory::PublicProfile) -> Self {
        value.profile.into()
    }
}

impl From<im_core::directory::DirectoryResolution> for DartDirectoryResolution {
    fn from(value: im_core::directory::DirectoryResolution) -> Self {
        Self {
            input: value.input,
            did: value.did.as_str().to_string(),
            handle: value.handle.map(|handle| handle.as_str().to_string()),
            conversation_id: value.conversation_id,
            profile: value.profile.map(Into::into),
            warnings: value.warnings,
        }
    }
}

impl From<im_core::directory::DisplayProfile> for crate::dto::directory::DartDisplayProfile {
    fn from(value: im_core::directory::DisplayProfile) -> Self {
        Self {
            did: value.did.map(|did| did.as_str().to_string()),
            handle: value.handle.map(|handle| handle.as_str().to_string()),
            display_name: value.display_name,
            avatar_uri: value.avatar_uri,
            avatar_url: value.avatar_url,
            profile_uri: value.profile_uri,
            subject_type: value.subject_type,
            cache_hit: value.cache_hit,
            is_stale: value.is_stale,
            legacy_fallback: value.legacy_fallback,
            warnings: value.warnings,
        }
    }
}

impl From<im_core::directory::RelationshipStatus> for DartRelationStatus {
    fn from(value: im_core::directory::RelationshipStatus) -> Self {
        Self {
            peer: value.peer.as_str().to_string(),
            did: value.did.as_str().to_string(),
            is_following: value.is_following,
            is_follower: value.is_follower,
            is_friend: value.is_friend,
            is_blocked: value.is_blocked,
            is_blocked_by: value.is_blocked_by,
            is_contact: value.is_contact,
            messaged: value.messaged,
            relationship: value.relationship,
            display_name: None,
            warnings: value.warnings,
        }
    }
}

impl From<im_core::messages::MessageDirection> for DartMessageDirection {
    fn from(value: im_core::messages::MessageDirection) -> Self {
        match value {
            im_core::messages::MessageDirection::Outgoing => Self::Outgoing,
            im_core::messages::MessageDirection::Incoming => Self::Incoming,
            im_core::messages::MessageDirection::Unknown => Self::Unknown,
        }
    }
}

impl From<im_core::messages::MessageBodyView> for DartMessageBodyView {
    fn from(value: im_core::messages::MessageBodyView) -> Self {
        match value {
            im_core::messages::MessageBodyView::Text { text, kind } => Self {
                text: Some(text),
                kind: Some(message_kind_to_string(kind)),
                payload_json: None,
                unsupported_content_type: None,
            },
            im_core::messages::MessageBodyView::Payload { payload } => Self {
                text: None,
                kind: Some("payload".to_string()),
                payload_json: Some(payload.to_string()),
                unsupported_content_type: None,
            },
            im_core::messages::MessageBodyView::Unsupported { content_type } => Self {
                text: None,
                kind: None,
                payload_json: None,
                unsupported_content_type: content_type,
            },
        }
    }
}

fn message_kind_to_string(value: im_core::messages::MessageKind) -> String {
    match value {
        im_core::messages::MessageKind::Text => "text".to_string(),
        im_core::messages::MessageKind::Markdown => "markdown".to_string(),
    }
}

impl From<im_core::messages::MessageMetadata> for DartMessageMetadata {
    fn from(value: im_core::messages::MessageMetadata) -> Self {
        Self {
            operation_id: value.operation_id,
            delivery_state: value.delivery_state,
            send_state: value
                .send_state
                .map(|state| message_send_state_to_string(state.state)),
            retryable: value.retry_plan.as_ref().map(|plan| plan.retryable),
            retry_action: value
                .retry_plan
                .map(|plan| message_retry_action_to_string(plan.action)),
            server_sequence: value.server_sequence,
            content_type: value.content_type,
            conversation_identity: value.conversation_identity.map(Into::into),
            attributes: value.attributes.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<im_core::messages::ConversationIdentity> for DartConversationIdentity {
    fn from(value: im_core::messages::ConversationIdentity) -> Self {
        Self {
            conversation_id: value.conversation_id,
            canonical_thread_kind: value.canonical_thread_kind,
            canonical_thread_id: value.canonical_thread_id,
            storage_thread_ref: value.storage_thread_ref.into(),
            aliases: value.aliases.into_iter().map(Into::into).collect(),
            identity_scope: value.identity_scope.into(),
            migration_state: value.migration_state.into(),
        }
    }
}

impl From<im_core::messages::ConversationStorageThreadRef> for DartConversationStorageThreadRef {
    fn from(value: im_core::messages::ConversationStorageThreadRef) -> Self {
        Self {
            kind: value.kind,
            id: value.id,
        }
    }
}

impl From<im_core::messages::ConversationAlias> for DartConversationAlias {
    fn from(value: im_core::messages::ConversationAlias) -> Self {
        Self {
            kind: value.kind,
            id: value.id,
            source: value.source.into(),
        }
    }
}

impl From<im_core::messages::ConversationAliasSource> for DartConversationAliasSource {
    fn from(value: im_core::messages::ConversationAliasSource) -> Self {
        match value {
            im_core::messages::ConversationAliasSource::LegacyDirectDid => Self::LegacyDirectDid,
            im_core::messages::ConversationAliasSource::OldFlutterSortedDirect => {
                Self::OldFlutterSortedDirect
            }
            im_core::messages::ConversationAliasSource::PeerScopeStorage => Self::PeerScopeStorage,
            im_core::messages::ConversationAliasSource::GroupStorage => Self::GroupStorage,
            im_core::messages::ConversationAliasSource::ThreadStorage => Self::ThreadStorage,
            im_core::messages::ConversationAliasSource::Unknown => Self::Unknown,
        }
    }
}

impl From<im_core::messages::ConversationIdentityScope> for DartConversationIdentityScope {
    fn from(value: im_core::messages::ConversationIdentityScope) -> Self {
        match value {
            im_core::messages::ConversationIdentityScope::Direct => Self::Direct,
            im_core::messages::ConversationIdentityScope::Group => Self::Group,
            im_core::messages::ConversationIdentityScope::Thread => Self::Thread,
            im_core::messages::ConversationIdentityScope::Mail => Self::Mail,
            im_core::messages::ConversationIdentityScope::Unknown => Self::Unknown,
        }
    }
}

impl From<im_core::messages::ConversationMigrationState> for DartConversationMigrationState {
    fn from(value: im_core::messages::ConversationMigrationState) -> Self {
        match value {
            im_core::messages::ConversationMigrationState::Canonical => Self::Canonical,
            im_core::messages::ConversationMigrationState::AliasResolved => Self::AliasResolved,
            im_core::messages::ConversationMigrationState::LegacyInput => Self::LegacyInput,
            im_core::messages::ConversationMigrationState::Unknown => Self::Unknown,
        }
    }
}

fn message_send_state_to_string(value: im_core::messages::MessageSendStateKind) -> String {
    match value {
        im_core::messages::MessageSendStateKind::Pending => "pending".to_string(),
        im_core::messages::MessageSendStateKind::Accepted => "accepted".to_string(),
        im_core::messages::MessageSendStateKind::Sent => "sent".to_string(),
        im_core::messages::MessageSendStateKind::StoredLocally => "stored_locally".to_string(),
        im_core::messages::MessageSendStateKind::Failed => "failed".to_string(),
    }
}

fn message_retry_action_to_string(value: im_core::messages::MessageRetryAction) -> String {
    match value {
        im_core::messages::MessageRetryAction::None => "none".to_string(),
        im_core::messages::MessageRetryAction::RetryDirectText => "retry_direct_text".to_string(),
        im_core::messages::MessageRetryAction::RetryGroupText => "retry_group_text".to_string(),
        im_core::messages::MessageRetryAction::RetryDirectPayload => {
            "retry_direct_payload".to_string()
        }
        im_core::messages::MessageRetryAction::RetryGroupPayload => {
            "retry_group_payload".to_string()
        }
    }
}

impl From<im_core::messages::MessageMetadataAttribute> for DartMessageMetadataAttribute {
    fn from(value: im_core::messages::MessageMetadataAttribute) -> Self {
        Self {
            key: value.key,
            value: value.value,
        }
    }
}

impl From<im_core::messages::MessageTarget> for crate::dto::message::DartMessageTarget {
    fn from(value: im_core::messages::MessageTarget) -> Self {
        match value {
            im_core::messages::MessageTarget::Direct(peer) => Self::Direct {
                peer: peer.as_str().to_string(),
            },
            im_core::messages::MessageTarget::Group(group) => Self::Group {
                group: group.as_str().to_string(),
            },
        }
    }
}

impl From<im_core::messages::Message> for DartMessage {
    fn from(value: im_core::messages::Message) -> Self {
        let conversation_id = value
            .metadata
            .conversation_identity
            .as_ref()
            .map(|identity| identity.conversation_id.clone())
            .unwrap_or_default();
        let sender_peer_persona_id = value
            .metadata
            .attributes
            .iter()
            .find(|attribute| attribute.key == "sender_peer_persona_id")
            .map(|attribute| attribute.value.trim().to_owned())
            .filter(|value| !value.is_empty());
        let sender_did_snapshot = value.sender.as_str().to_owned();
        let (thread_kind, thread_id) = thread_ref_parts(value.thread);
        Self {
            id: value.id.as_str().to_string(),
            conversation_id,
            sender_peer_persona_id,
            sender_did_snapshot,
            thread_kind,
            thread_id,
            direction: value.direction.into(),
            sender: value.sender.as_str().to_string(),
            receiver: value.receiver.map(|receiver| receiver.as_str().to_string()),
            group: value.group.map(|group| group.as_str().to_string()),
            body: value.body.into(),
            sent_at: value.sent_at,
            received_at: value.received_at,
            metadata: value.metadata.into(),
        }
    }
}

fn thread_ref_parts(value: im_core::messages::ThreadRef) -> (String, String) {
    match value {
        im_core::messages::ThreadRef::Direct(peer) => {
            ("direct".to_string(), peer.as_str().to_string())
        }
        im_core::messages::ThreadRef::Group(group) => {
            ("group".to_string(), group.as_str().to_string())
        }
        im_core::messages::ThreadRef::Thread(id) => ("thread".to_string(), id.as_str().to_string()),
    }
}

impl From<im_core::ids::Page<im_core::messages::Message>> for DartMessagePage {
    fn from(value: im_core::ids::Page<im_core::messages::Message>) -> Self {
        Self {
            items: value.items.into_iter().map(Into::into).collect(),
            next_cursor: value.next_cursor.map(|cursor| cursor.as_str().to_string()),
            has_more: value.has_more,
        }
    }
}

impl From<im_core::messages::Conversation> for DartConversation {
    fn from(value: im_core::messages::Conversation) -> Self {
        let (thread_kind, thread_id) = thread_ref_parts(value.thread);
        Self {
            conversation_id: value.conversation_id,
            peer_persona_id: value.peer_persona_id,
            canonical_group_did: value.canonical_group_did,
            resolution_state: value.resolution_state.into(),
            thread_kind,
            thread_id,
            conversation_identity: value.conversation_identity.map(Into::into),
            title: value.title,
            participants: value
                .participants
                .into_iter()
                .map(|peer| peer.as_str().to_string())
                .collect(),
            last_message: value.last_message.map(Into::into),
            unread_count: value.unread_count,
            unread_mention_count: value.unread_mention_count,
            first_unread_mention_message_id: value
                .first_unread_mention_message_id
                .map(|message_id| message_id.as_str().to_string()),
            message_count: value.message_count,
            last_message_at: value.last_message_at,
            activity_at: value.activity_at,
        }
    }
}

impl From<im_core::messages::ConversationResolutionState> for DartConversationResolutionState {
    fn from(value: im_core::messages::ConversationResolutionState) -> Self {
        match value {
            im_core::messages::ConversationResolutionState::Resolved => Self::Resolved,
            im_core::messages::ConversationResolutionState::LegacyUnresolved => {
                Self::LegacyUnresolved
            }
            im_core::messages::ConversationResolutionState::BlockedConflict => {
                Self::BlockedConflict
            }
        }
    }
}

impl From<im_core::ids::Page<im_core::messages::Conversation>> for DartConversationPage {
    fn from(value: im_core::ids::Page<im_core::messages::Conversation>) -> Self {
        Self {
            items: value.items.into_iter().map(Into::into).collect(),
            next_cursor: value.next_cursor.map(|cursor| cursor.as_str().to_string()),
            has_more: value.has_more,
        }
    }
}

impl From<im_core::messages::ConversationListSnapshot> for DartConversationListSnapshot {
    fn from(value: im_core::messages::ConversationListSnapshot) -> Self {
        Self {
            format_version: value.format_version,
            im_schema_version: value.im_schema_version,
            owner_identity_id: value.owner_identity_id,
            owner_did: value.owner_did,
            generated_at_ms: value.generated_at_ms,
            summary_version: value.summary_version,
            unread_total: value.unread_total,
            items: value.items.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<im_core::messages::ConversationStorePatch> for DartConversationStorePatch {
    fn from(value: im_core::messages::ConversationStorePatch) -> Self {
        match value {
            im_core::messages::ConversationStorePatch::Reset {
                owner_identity_id,
                owner_did,
                version,
                unread_total,
                items,
            } => DartConversationStorePatch::Reset {
                owner_identity_id,
                owner_did,
                version,
                unread_total,
                items: items.into_iter().map(Into::into).collect(),
            },
            im_core::messages::ConversationStorePatch::Upsert {
                owner_identity_id,
                owner_did,
                version,
                unread_total,
                item,
                index,
            } => DartConversationStorePatch::Upsert {
                owner_identity_id,
                owner_did,
                version,
                unread_total,
                item: item.into(),
                index,
            },
            im_core::messages::ConversationStorePatch::Remove {
                owner_identity_id,
                owner_did,
                version,
                unread_total,
                conversation_id,
            } => DartConversationStorePatch::Remove {
                owner_identity_id,
                owner_did,
                version,
                unread_total,
                conversation_id,
            },
            im_core::messages::ConversationStorePatch::Reorder {
                owner_identity_id,
                owner_did,
                version,
                unread_total,
                conversation_id,
                index,
            } => DartConversationStorePatch::Reorder {
                owner_identity_id,
                owner_did,
                version,
                unread_total,
                conversation_id,
                index,
            },
            im_core::messages::ConversationStorePatch::RepairRequired {
                owner_identity_id,
                owner_did,
                version,
                unread_total,
                reason,
            } => DartConversationStorePatch::RepairRequired {
                owner_identity_id,
                owner_did,
                version,
                unread_total,
                reason,
            },
        }
    }
}

impl From<im_core::messages::ThreadMessageStorePatch> for DartThreadMessageStorePatch {
    fn from(value: im_core::messages::ThreadMessageStorePatch) -> Self {
        match value {
            im_core::messages::ThreadMessageStorePatch::Reset {
                owner_identity_id,
                owner_did,
                version,
                thread_kind,
                thread_id,
                conversation_identity,
                items,
            } => DartThreadMessageStorePatch::Reset {
                owner_identity_id,
                owner_did,
                version,
                thread_kind,
                thread_id,
                conversation_identity: conversation_identity.map(Into::into),
                items: items.into_iter().map(Into::into).collect(),
            },
            im_core::messages::ThreadMessageStorePatch::Upsert {
                owner_identity_id,
                owner_did,
                version,
                thread_kind,
                thread_id,
                conversation_identity,
                message,
                index,
            } => DartThreadMessageStorePatch::Upsert {
                owner_identity_id,
                owner_did,
                version,
                thread_kind,
                thread_id,
                conversation_identity: conversation_identity.map(Into::into),
                message: message.into(),
                index,
            },
            im_core::messages::ThreadMessageStorePatch::Remove {
                owner_identity_id,
                owner_did,
                version,
                thread_kind,
                thread_id,
                conversation_identity,
                message_id,
            } => DartThreadMessageStorePatch::Remove {
                owner_identity_id,
                owner_did,
                version,
                thread_kind,
                thread_id,
                conversation_identity: conversation_identity.map(Into::into),
                message_id,
            },
            im_core::messages::ThreadMessageStorePatch::RepairRequired {
                owner_identity_id,
                owner_did,
                version,
                thread_kind,
                thread_id,
                conversation_identity,
                reason,
            } => DartThreadMessageStorePatch::RepairRequired {
                owner_identity_id,
                owner_did,
                version,
                thread_kind,
                thread_id,
                conversation_identity: conversation_identity.map(Into::into),
                reason,
            },
        }
    }
}

impl From<im_core::messages::ConversationSnapshotItem> for DartConversationSnapshotItem {
    fn from(value: im_core::messages::ConversationSnapshotItem) -> Self {
        Self {
            conversation_id: value.conversation_id,
            peer_persona_id: value.peer_persona_id,
            canonical_group_did: value.canonical_group_did,
            resolution_state: value.resolution_state.into(),
            thread_kind: value.thread_kind,
            thread_id: value.thread_id,
            title: value.title,
            conversation_identity: value.conversation_identity.map(Into::into),
            participants: value.participants,
            last_message: value.last_message.map(Into::into),
            unread_count: value.unread_count,
            unread_mention_count: value.unread_mention_count,
            first_unread_mention_message_id: value.first_unread_mention_message_id,
            message_count: value.message_count,
            last_message_at: value.last_message_at,
            activity_at: value.activity_at,
        }
    }
}

impl From<im_core::messages::ConversationSnapshotMessage> for DartConversationSnapshotMessage {
    fn from(value: im_core::messages::ConversationSnapshotMessage) -> Self {
        Self {
            id: value.id,
            thread_kind: value.thread_kind,
            thread_id: value.thread_id,
            conversation_identity: value.conversation_identity.map(Into::into),
            direction: value.direction,
            sender: value.sender,
            receiver: value.receiver,
            group: value.group,
            body: value.body.into(),
            sent_at: value.sent_at,
            received_at: value.received_at,
            server_sequence: value.server_sequence,
            content_type: value.content_type,
            attributes: value.attributes.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<im_core::messages::ConversationSnapshotMessageBody>
    for DartConversationSnapshotMessageBody
{
    fn from(value: im_core::messages::ConversationSnapshotMessageBody) -> Self {
        Self {
            text: value.text,
            kind: value.kind,
            payload_json: value.payload_json,
            unsupported_content_type: value.unsupported_content_type,
        }
    }
}

impl From<im_core::messages::SendMessageResult> for DartSendMessageResult {
    fn from(value: im_core::messages::SendMessageResult) -> Self {
        Self {
            message: value.message.into(),
            delivery_state: delivery_state_to_string(value.delivery),
            warnings: value.warnings,
        }
    }
}

impl From<im_core::attachments::UploadedAttachment> for DartUploadedAttachment {
    fn from(value: im_core::attachments::UploadedAttachment) -> Self {
        Self {
            attachment_id: value.attachment_id,
            filename: value.filename,
            mime_type: value.mime_type,
            size_bytes: value.size_bytes,
            size: value.size,
            digest_b64u: value.digest_b64u,
            object_uri: value.object_uri,
            object_encryption_mode: value.object_encryption_mode,
            plaintext_size_bytes: value.plaintext_size_bytes,
        }
    }
}

impl From<im_core::attachments::AttachmentSendResult> for DartAttachmentSendResult {
    fn from(value: im_core::attachments::AttachmentSendResult) -> Self {
        Self {
            message: value.message.into(),
            target_kind: value.target_kind,
            target_did: value.target_did,
            attachment: value.attachment.into(),
            // `AttachmentSendResult.manifest` is already redacted by im-core for
            // E2EE attachments; keep exposing only that public projection.
            manifest_json: value.manifest.to_string(),
        }
    }
}

impl From<im_core::attachments::DownloadedAttachment> for DartDownloadedAttachment {
    fn from(value: im_core::attachments::DownloadedAttachment) -> Self {
        Self {
            attachment_id: value.attachment_id,
            filename: value.filename,
            mime_type: value.mime_type,
            size_bytes: value.size_bytes,
            destination: value.destination.into(),
            warnings: value.warnings,
        }
    }
}

impl From<im_core::attachments::DownloadedAttachmentDestination>
    for DartDownloadedAttachmentDestination
{
    fn from(value: im_core::attachments::DownloadedAttachmentDestination) -> Self {
        match value {
            im_core::attachments::DownloadedAttachmentDestination::LocalFile(path) => {
                Self::LocalFile {
                    path: path.display().to_string(),
                }
            }
            im_core::attachments::DownloadedAttachmentDestination::Memory(bytes) => {
                Self::Memory { bytes }
            }
        }
    }
}

fn delivery_state_to_string(value: im_core::messages::DeliveryState) -> String {
    match value {
        im_core::messages::DeliveryState::Accepted => "accepted".to_string(),
        im_core::messages::DeliveryState::Sent => "sent".to_string(),
        im_core::messages::DeliveryState::StoredLocally => "stored_locally".to_string(),
        im_core::messages::DeliveryState::Failed { reason } => format!("failed:{reason}"),
    }
}

impl From<im_core::secure::DirectSecureState> for DartDirectSecureState {
    fn from(value: im_core::secure::DirectSecureState) -> Self {
        match value {
            im_core::secure::DirectSecureState::Ready => Self::Ready,
            im_core::secure::DirectSecureState::Preparing => Self::Preparing,
            im_core::secure::DirectSecureState::WaitingForPeer => Self::WaitingForPeer,
            im_core::secure::DirectSecureState::NeedsRepair => Self::NeedsRepair,
            im_core::secure::DirectSecureState::Unavailable => Self::Unavailable,
            im_core::secure::DirectSecureState::Unknown => Self::Unknown,
        }
    }
}

impl From<im_core::secure::DirectSecureStatus> for DartDirectSecureStatus {
    fn from(value: im_core::secure::DirectSecureStatus) -> Self {
        Self {
            peer: value.peer.as_str().to_string(),
            resolved_peer: value.resolved_peer.map(|peer| peer.as_str().to_string()),
            state: value.state.into(),
            can_send_secure: value.can_send_secure,
            pending_outbox_count: value.pending_outbox_count,
            problem: value.problem.map(Into::into),
            warnings: value.warnings,
        }
    }
}

impl From<im_core::secure::DirectSecurePrepareResult> for DartDirectSecurePrepareResult {
    fn from(value: im_core::secure::DirectSecurePrepareResult) -> Self {
        Self {
            peer: value.peer.as_str().to_string(),
            state: value.state.into(),
            can_send_secure: value.can_send_secure,
            warnings: value.warnings,
        }
    }
}

impl From<im_core::secure::DirectSecureRepairResult> for DartDirectSecureRepairResult {
    fn from(value: im_core::secure::DirectSecureRepairResult) -> Self {
        Self {
            peer: value.peer.as_str().to_string(),
            state: value.state.into(),
            repaired: value.repaired,
            problem: value.problem.map(Into::into),
            warnings: value.warnings,
        }
    }
}

impl From<im_core::secure::GroupSecureState> for DartGroupSecureState {
    fn from(value: im_core::secure::GroupSecureState) -> Self {
        match value {
            im_core::secure::GroupSecureState::Ready => Self::Ready,
            im_core::secure::GroupSecureState::Syncing => Self::Syncing,
            im_core::secure::GroupSecureState::NeedsRepair => Self::NeedsRepair,
            im_core::secure::GroupSecureState::WaitingForMembershipUpdate => {
                Self::WaitingForMembershipUpdate
            }
            im_core::secure::GroupSecureState::MissingLocalState => Self::MissingLocalState,
            im_core::secure::GroupSecureState::Unavailable => Self::Unavailable,
            im_core::secure::GroupSecureState::Unknown => Self::Unknown,
        }
    }
}

impl From<im_core::secure::GroupSecureLocalReadiness> for DartGroupSecureLocalReadiness {
    fn from(value: im_core::secure::GroupSecureLocalReadiness) -> Self {
        Self {
            has_local_state: value.has_local_state,
            has_active_membership: value.has_active_membership,
        }
    }
}

impl From<im_core::secure::GroupSecurePendingWork> for DartGroupSecurePendingWork {
    fn from(value: im_core::secure::GroupSecurePendingWork) -> Self {
        Self {
            pending_notices: value.pending_notices,
            pending_commits: value.pending_commits,
        }
    }
}

impl From<im_core::secure::GroupSecureStatus> for DartGroupSecureStatus {
    fn from(value: im_core::secure::GroupSecureStatus) -> Self {
        Self {
            group: value.group.as_str().to_string(),
            state: value.state.into(),
            can_send_secure: value.can_send_secure,
            local_readiness: value.local_readiness.into(),
            pending_work: value.pending_work.into(),
            problem: value.problem.map(Into::into),
            warnings: value.warnings,
        }
    }
}

impl From<im_core::secure::GroupSecurePrepareResult> for DartGroupSecurePrepareResult {
    fn from(value: im_core::secure::GroupSecurePrepareResult) -> Self {
        Self {
            group: value.group.as_str().to_string(),
            state: value.state.into(),
            can_send_secure: value.can_send_secure,
            warnings: value.warnings,
        }
    }
}

impl From<im_core::secure::GroupSecureRepairResult> for DartGroupSecureRepairResult {
    fn from(value: im_core::secure::GroupSecureRepairResult) -> Self {
        Self {
            group: value.group.as_str().to_string(),
            state: value.state.into(),
            repaired: value.repaired,
            added_devices: value.added_devices,
            removed_devices: value.removed_devices,
            remaining_devices: value.remaining_devices,
            problem: value.problem.map(Into::into),
            warnings: value.warnings,
        }
    }
}

impl From<im_core::secure::SecureOutboxStatus> for DartSecureOutboxStatus {
    fn from(value: im_core::secure::SecureOutboxStatus) -> Self {
        match value {
            im_core::secure::SecureOutboxStatus::Queued => Self::Queued,
            im_core::secure::SecureOutboxStatus::Sending => Self::Sending,
            im_core::secure::SecureOutboxStatus::Failed => Self::Failed,
            im_core::secure::SecureOutboxStatus::Sent => Self::Sent,
            im_core::secure::SecureOutboxStatus::Dropped => Self::Dropped,
        }
    }
}

impl From<im_core::secure::SecureOutboxEntry> for DartSecureOutboxEntry {
    fn from(value: im_core::secure::SecureOutboxEntry) -> Self {
        Self {
            id: value.id.as_str().to_string(),
            target: value.target.into(),
            message_kind: value.message_kind,
            status: value.status.into(),
            attempt_count: value.attempt_count,
            last_error: value.last_error.map(Into::into),
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

impl From<im_core::secure::SecureOutboxResult> for DartSecureOutboxResult {
    fn from(value: im_core::secure::SecureOutboxResult) -> Self {
        Self {
            id: value.id.as_str().to_string(),
            status: value.status.into(),
            delivery: value.delivery.map(Into::into),
            warnings: value.warnings,
        }
    }
}

impl From<im_core::secure::SecureDelivery> for DartSecureDelivery {
    fn from(value: im_core::secure::SecureDelivery) -> Self {
        Self {
            message_id: value.message_id.map(|id| id.as_str().to_string()),
            state: delivery_state_to_string(value.state),
        }
    }
}

impl From<im_core::secure::SecureProblem> for DartSecureProblem {
    fn from(value: im_core::secure::SecureProblem) -> Self {
        Self {
            code: value.code.into(),
            message: value.message,
            retryable: value.retryable,
        }
    }
}

impl From<im_core::secure::SecureProblemCode> for DartSecureProblemCode {
    fn from(value: im_core::secure::SecureProblemCode) -> Self {
        match value {
            im_core::secure::SecureProblemCode::IdentityNotReady => Self::IdentityNotReady,
            im_core::secure::SecureProblemCode::PeerNotFound => Self::PeerNotFound,
            im_core::secure::SecureProblemCode::PeerKeysUnavailable => Self::PeerKeysUnavailable,
            im_core::secure::SecureProblemCode::SessionNeedsRepair => Self::SessionNeedsRepair,
            im_core::secure::SecureProblemCode::GroupStateUnavailable => {
                Self::GroupStateUnavailable
            }
            im_core::secure::SecureProblemCode::LocalStateUnavailable => {
                Self::LocalStateUnavailable
            }
            im_core::secure::SecureProblemCode::TransportUnavailable => Self::TransportUnavailable,
            im_core::secure::SecureProblemCode::Unsupported => Self::Unsupported,
            im_core::secure::SecureProblemCode::Unknown => Self::Unknown,
        }
    }
}

impl From<im_core::messages::MarkReadResult> for DartMarkReadResult {
    fn from(value: im_core::messages::MarkReadResult) -> Self {
        Self {
            updated_count: value.updated_count,
            message_ids: value
                .message_ids
                .into_iter()
                .map(|id| id.as_str().to_string())
                .collect(),
            warnings: value.warnings,
        }
    }
}

impl From<im_core::messages::MarkThreadReadResult> for DartMarkThreadReadResult {
    fn from(value: im_core::messages::MarkThreadReadResult) -> Self {
        Self {
            updated_count: value.updated_count,
            remote_acknowledged: value.remote_acknowledged,
            partial: value.partial,
            fallback_used: value.fallback_used,
            pending_remote_ack: value.pending_remote_ack,
            effective_watermark: value.effective_watermark.map(Into::into),
            legacy_message_ids: value
                .legacy_message_ids
                .into_iter()
                .map(|id| id.as_str().to_string())
                .collect(),
            warnings: value.warnings,
        }
    }
}

impl From<im_core::messages::ReadWatermark> for DartReadWatermark {
    fn from(value: im_core::messages::ReadWatermark) -> Self {
        Self {
            last_read_message_id: value.last_read_message_id.map(|id| id.as_str().to_string()),
            last_read_thread_seq: value.last_read_thread_seq,
            read_at: value.read_at.map(|value| value.to_rfc3339()),
        }
    }
}

impl From<im_core::messages::SyncDeltaResult> for DartSyncDeltaResult {
    fn from(value: im_core::messages::SyncDeltaResult) -> Self {
        Self {
            events_applied: value.events_applied,
            pages_fetched: value.pages_fetched,
            last_applied_event_seq: value.last_applied_event_seq,
            has_more: value.has_more,
            snapshot_required: value.snapshot_required,
            retention_floor_event_seq: value.retention_floor_event_seq,
            warnings: value.warnings,
        }
    }
}

impl From<im_core::messages::MessageSyncStatus> for DartMessageSyncStatus {
    fn from(value: im_core::messages::MessageSyncStatus) -> Self {
        match value {
            im_core::messages::MessageSyncStatus::Idle => Self::Idle,
            im_core::messages::MessageSyncStatus::Changed => Self::Changed,
            im_core::messages::MessageSyncStatus::RecoveryRequired => Self::RecoveryRequired,
            im_core::messages::MessageSyncStatus::RetryableFailure => Self::RetryableFailure,
            im_core::messages::MessageSyncStatus::AuthRevoked => Self::AuthRevoked,
        }
    }
}

impl From<im_core::messages::CommittedIncomingMessage> for DartCommittedIncomingMessage {
    fn from(value: im_core::messages::CommittedIncomingMessage) -> Self {
        debug_assert_eq!(value.source, "live_delta");
        Self {
            event_id: value.event_id,
            logical_message_id: value.logical_message_id,
            source: DartCommittedMessageSource::LiveDelta,
            direction: value.direction.into(),
            message: value.message.into(),
        }
    }
}

impl From<im_core::messages::MessageSyncOutcome> for DartMessageSyncOutcome {
    fn from(value: im_core::messages::MessageSyncOutcome) -> Self {
        Self {
            status: value.status.into(),
            events_applied: value.events_applied,
            pages_fetched: value.pages_fetched,
            messages_hydrated: value.messages_hydrated,
            duplicates_skipped: value.duplicates_skipped,
            changed_conversation_ids: value.changed_conversation_ids,
            committed_incoming_messages: value
                .committed_incoming_messages
                .into_iter()
                .map(Into::into)
                .collect(),
            error_code: value.error_code,
            warnings: value.warnings,
        }
    }
}

impl From<im_core::messages::MessageSyncMode> for DartMessageSyncMode {
    fn from(value: im_core::messages::MessageSyncMode) -> Self {
        match value {
            im_core::messages::MessageSyncMode::Uninitialized => Self::Uninitialized,
            im_core::messages::MessageSyncMode::Idle => Self::Idle,
            im_core::messages::MessageSyncMode::Recovering => Self::Recovering,
            im_core::messages::MessageSyncMode::Retryable => Self::Retryable,
            im_core::messages::MessageSyncMode::Blocked => Self::Blocked,
        }
    }
}

impl From<im_core::messages::MessageSyncDirtyDomain> for DartMessageSyncDirtyDomain {
    fn from(value: im_core::messages::MessageSyncDirtyDomain) -> Self {
        match value {
            im_core::messages::MessageSyncDirtyDomain::Messages => Self::Messages,
            im_core::messages::MessageSyncDirtyDomain::ReadState => Self::ReadState,
        }
    }
}

impl From<im_core::messages::MessageSyncRetryState> for DartMessageSyncRetryState {
    fn from(value: im_core::messages::MessageSyncRetryState) -> Self {
        match value {
            im_core::messages::MessageSyncRetryState::None => Self::None,
            im_core::messages::MessageSyncRetryState::Pending => Self::Pending,
            im_core::messages::MessageSyncRetryState::InFlight => Self::InFlight,
            im_core::messages::MessageSyncRetryState::Scheduled => Self::Scheduled,
            im_core::messages::MessageSyncRetryState::PermanentFailure => Self::PermanentFailure,
        }
    }
}

impl From<im_core::messages::MessageSyncDiagnostics> for DartMessageSyncDiagnostics {
    fn from(value: im_core::messages::MessageSyncDiagnostics) -> Self {
        Self {
            last_success_at: value.last_success_at,
            mode: value.mode.into(),
            pending_mutation_count: value.pending_mutation_count,
            dirty_domains: value.dirty_domains.into_iter().map(Into::into).collect(),
            retry_state: value.retry_state.into(),
            next_retry_at: value.next_retry_at,
        }
    }
}

impl From<im_core::messages::SyncThreadAfterResult> for DartSyncThreadAfterResult {
    fn from(value: im_core::messages::SyncThreadAfterResult) -> Self {
        Self {
            messages: value.messages.into_iter().map(Into::into).collect(),
            next_after_server_seq: value.next_after_server_seq,
            has_more: value.has_more,
            warnings: value.warnings,
        }
    }
}

impl From<im_core::groups::GroupSummary> for DartGroupSummary {
    fn from(value: im_core::groups::GroupSummary) -> Self {
        let conversation_id = format!("group:{}", value.did.as_str());
        Self {
            conversation_id,
            id: value.id,
            did: value.did.as_str().to_string(),
            name: value.name,
            display_name: value.display_name,
            avatar_uri: value.avatar_uri,
            my_role: value.my_role,
            membership_status: value.membership_status,
            member_count: value.member_count,
            last_message_at: value.last_message_at,
        }
    }
}

impl From<im_core::groups::GroupSnapshot> for DartGroupSnapshot {
    fn from(value: im_core::groups::GroupSnapshot) -> Self {
        let conversation_id = format!("group:{}", value.did.as_str());
        Self {
            conversation_id,
            id: value.id,
            did: value.did.as_str().to_string(),
            name: value.name,
            display_name: value.display_name,
            description: value.description,
            avatar_uri: value.avatar_uri,
            my_role: value.my_role,
            membership_status: value.membership_status,
            member_count: value.member_count,
            last_message_at: value.last_message_at,
        }
    }
}

impl From<im_core::groups::GroupMember> for DartGroupMember {
    fn from(value: im_core::groups::GroupMember) -> Self {
        Self {
            membership_id: value.membership_id,
            peer_persona_id: value.peer_persona_id,
            did: value.did.map(|did| did.as_str().to_string()),
            credential_did: value.credential_did.map(|did| did.as_str().to_string()),
            handle: value.handle.map(|handle| handle.as_str().to_string()),
            role: value.role,
            status: value.status,
            joined_at: value.joined_at,
            subject_type: value.subject_type,
        }
    }
}

impl From<im_core::groups::GroupReadResult> for DartGroupReadResult {
    fn from(value: im_core::groups::GroupReadResult) -> Self {
        Self {
            group: value.group.map(Into::into),
            groups: value.groups.into_iter().map(Into::into).collect(),
            members: value.members.into_iter().map(Into::into).collect(),
            messages: value.messages.into(),
            total: value.total,
            next_cursor: value.next_cursor.map(|cursor| cursor.as_str().to_owned()),
            has_more: value.has_more,
            page_group_did: value.page_group.map(|group| group.as_str().to_owned()),
            group_state_version: value.group_state_version,
            source: value.source,
            warnings: value.warnings,
        }
    }
}

impl From<im_core::groups::GroupRebindRecoveryItem> for DartGroupRebindRecoveryItem {
    fn from(value: im_core::groups::GroupRebindRecoveryItem) -> Self {
        Self {
            group_did: value.group.as_str().to_owned(),
            layer: value.layer,
            phase: value.phase,
            blocked: value.blocked,
        }
    }
}

impl From<im_core::groups::GroupRebindRecoverySummary> for DartGroupRebindRecoverySummary {
    fn from(value: im_core::groups::GroupRebindRecoverySummary) -> Self {
        Self {
            processed: value.processed,
            completed: value.completed,
            pending: value.pending,
            blocked: value.blocked,
            send_paused_group_dids: value
                .send_paused_groups
                .into_iter()
                .map(|group| group.as_str().to_owned())
                .collect(),
            items: value.items.into_iter().map(Into::into).collect(),
            warnings: value.warnings,
        }
    }
}

impl From<im_core::realtime::RealtimeStatus> for DartRealtimeStatus {
    fn from(value: im_core::realtime::RealtimeStatus) -> Self {
        Self {
            connected: value.connected,
            state: realtime_state_to_string(value.state),
            subscriptions: value
                .subscriptions
                .into_iter()
                .map(realtime_subscription_to_string)
                .collect(),
            last_error: value.last_error,
            warnings: Vec::new(),
        }
    }
}

pub(crate) fn realtime_event_to_dart(value: im_core::realtime::ImEvent) -> DartRealtimeEvent {
    use im_core::realtime::{GroupUpdateKind, HostNotificationKind, ImEvent, MessageUpdateKind};

    let empty = || DartRealtimeEvent {
        kind: String::new(),
        state: None,
        reason: None,
        message: None,
        message_id: None,
        thread_kind: None,
        thread_id: None,
        update_kind: None,
        group: None,
        notification_id: None,
        title: None,
        body: None,
        source: None,
        host_kind: None,
        content_type: None,
        notification_type: None,
        sync: None,
    };

    match value {
        ImEvent::ConnectionStateChanged(event) => {
            let mut out = empty();
            out.kind = "connection_state_changed".to_string();
            out.state = Some(realtime_state_to_string(event.state));
            out.reason = event.reason;
            out
        }
        ImEvent::MessageReceived(event) => {
            let mut out = empty();
            out.kind = "message_received".to_string();
            out.message = Some(event.message.into());
            out.sync = event.sync.map(Into::into);
            out
        }
        ImEvent::MessageUpdated(event) => {
            let (thread_kind, thread_id) = thread_ref_parts(event.thread);
            let mut out = empty();
            out.kind = "message_updated".to_string();
            out.message_id = Some(event.message_id.as_str().to_string());
            out.thread_kind = Some(thread_kind);
            out.thread_id = Some(thread_id);
            out.update_kind = Some(
                match event.update_kind {
                    MessageUpdateKind::Read => "read",
                    MessageUpdateKind::DeliveryStateChanged => "delivery_state_changed",
                    MessageUpdateKind::Unknown => "unknown",
                }
                .to_string(),
            );
            out.sync = event.sync.map(Into::into);
            out
        }
        ImEvent::GroupUpdated(event) => {
            let mut out = empty();
            out.kind = "group_updated".to_string();
            out.group = Some(event.group.as_str().to_string());
            out.update_kind = Some(
                match event.update_kind {
                    GroupUpdateKind::Created => "created",
                    GroupUpdateKind::Updated => "updated",
                    GroupUpdateKind::MemberAdded => "member_added",
                    GroupUpdateKind::MemberRemoved => "member_removed",
                    GroupUpdateKind::MessageAdded => "message_added",
                    GroupUpdateKind::Unknown => "unknown",
                }
                .to_string(),
            );
            out.sync = event.sync.map(Into::into);
            out
        }
        ImEvent::SystemNotificationChanged(event) => {
            let mut out = empty();
            out.kind = "system_notification_changed".to_string();
            out.notification_id = Some(event.notification.event_id);
            out.notification_type = Some(
                match event.notification.kind {
                    im_core::system_notifications::SystemNotificationKind::JoinRequested => {
                        "awiki.device.join-requested.v1"
                    }
                    im_core::system_notifications::SystemNotificationKind::JoinClaimed => {
                        "awiki.device.join-claimed.v1"
                    }
                    im_core::system_notifications::SystemNotificationKind::JoinResponseVerified => {
                        "awiki.device.join-response-verified.v1"
                    }
                    im_core::system_notifications::SystemNotificationKind::JoinCompleted => {
                        "awiki.device.join-completed.v1"
                    }
                    im_core::system_notifications::SystemNotificationKind::JoinCancelled => {
                        "awiki.device.join-cancelled.v1"
                    }
                    im_core::system_notifications::SystemNotificationKind::JoinRejected => {
                        "awiki.device.join-rejected.v1"
                    }
                    im_core::system_notifications::SystemNotificationKind::JoinExpired => {
                        "awiki.device.join-expired.v1"
                    }
                }
                .to_owned(),
            );
            out.sync = event.sync.map(Into::into);
            out
        }
        ImEvent::LocalNotification(event) => {
            let mut out = empty();
            out.kind = "local_notification".to_string();
            out.notification_id = event.notification_id;
            out.title = event.title;
            out.body = event.body;
            out.source = event.source;
            out
        }
        ImEvent::HostNotification(event) => {
            let mut out = empty();
            out.kind = "host_notification".to_string();
            out.host_kind = Some(
                match event.event_type {
                    HostNotificationKind::DirectMessage => "direct_message",
                    HostNotificationKind::GroupMessage => "group_message",
                    HostNotificationKind::GroupState => "group_state",
                    HostNotificationKind::Mail => "mail",
                    HostNotificationKind::Unknown => "unknown",
                }
                .to_string(),
            );
            out.title = event.title;
            out.body = event.body;
            if let Some(thread) = event.thread {
                let (thread_kind, thread_id) = thread_ref_parts(thread);
                out.thread_kind = Some(thread_kind);
                out.thread_id = Some(thread_id);
            }
            out
        }
        ImEvent::UnknownNotification(event) => {
            let mut out = empty();
            out.kind = "unknown_notification".to_string();
            out.reason = Some(event.reason);
            out.content_type = event.content_type;
            out.notification_type = event.notification_type;
            out.sync = event.sync.map(Into::into);
            out
        }
    }
}

impl From<im_core::realtime::RealtimeSyncHint> for DartRealtimeSyncHint {
    fn from(value: im_core::realtime::RealtimeSyncHint) -> Self {
        Self {
            domains: value.domains.into_iter().map(Into::into).collect(),
            reason: value.reason,
            sync_dirty: value.sync_dirty,
            gap_detected: value.gap_detected,
            has_unknown_domain: value.has_unknown_domain,
        }
    }
}

impl From<im_core::realtime::SyncDomain> for DartSyncDomain {
    fn from(value: im_core::realtime::SyncDomain) -> Self {
        match value {
            im_core::realtime::SyncDomain::Message => Self::Message,
            im_core::realtime::SyncDomain::Profile => Self::Profile,
            im_core::realtime::SyncDomain::AgentInventory => Self::AgentInventory,
            im_core::realtime::SyncDomain::AgentStatus => Self::AgentStatus,
            im_core::realtime::SyncDomain::DeviceRegistry => Self::DeviceRegistry,
        }
    }
}

pub(crate) fn realtime_state_to_string(
    value: im_core::realtime::RealtimeConnectionState,
) -> String {
    match value {
        im_core::realtime::RealtimeConnectionState::Disconnected => "disconnected".to_string(),
        im_core::realtime::RealtimeConnectionState::Connecting => "connecting".to_string(),
        im_core::realtime::RealtimeConnectionState::Connected => "connected".to_string(),
        im_core::realtime::RealtimeConnectionState::Reconnecting => "reconnecting".to_string(),
        im_core::realtime::RealtimeConnectionState::Closed => "closed".to_string(),
    }
}

fn realtime_subscription_to_string(value: im_core::realtime::RealtimeSubscription) -> String {
    match value {
        im_core::realtime::RealtimeSubscription::Messages => "messages".to_string(),
        im_core::realtime::RealtimeSubscription::Groups => "groups".to_string(),
        im_core::realtime::RealtimeSubscription::Notifications => "notifications".to_string(),
        im_core::realtime::RealtimeSubscription::HostNotifications => {
            "host_notifications".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        realtime_event_to_dart, DartActiveSyncAccountBinding, DartGroupSummary,
        DartHandleRegistrationJoinMode, DartHandleRegistrationResult, DartRelationStatus,
        DartSyncDomain,
    };
    use im_core::{
        directory::RelationshipStatus,
        ids::{Did, GroupRef, MessageId, PeerRef, ThreadId},
        messages::{
            Message, MessageBodyView, MessageDirection, MessageKind, MessageMetadata, ThreadRef,
        },
        realtime::{
            ConnectionStateChanged, GroupUpdateKind, GroupUpdatedEvent, HostNotificationEvent,
            HostNotificationKind, ImEvent, LocalNotificationEvent, MessageReceivedEvent,
            MessageUpdateKind, MessageUpdatedEvent, RealtimeConnectionState, RealtimeSyncHint,
            SystemNotificationChangedEvent, UnknownNotificationEvent,
        },
        system_notifications::{
            SystemNotificationKind, SystemNotificationSnapshot, SystemNotificationState,
        },
    };

    #[test]
    fn registration_recovery_join_maps_to_opaque_typed_dart_result() {
        let mapped =
            DartHandleRegistrationResult::from(im_core::identity::HandleRegistrationResult {
                identity: None,
                account_id: None,
                handle: im_core::ids::Handle::parse("alice.example.test", "").unwrap(),
                method: im_core::identity::RegistrationMethod::Phone,
                state: im_core::identity::HandleRegistrationState::JoinRequired,
                join_required: Some(
                    im_core::identity::HandleRegistrationJoinRequiredPreparation {
                        preparation_id: "regjoin_opaque".to_owned(),
                        mode: im_core::identity::HandleRegistrationJoinMode::HandleRecoveryRebind,
                        requires_user_presence: true,
                        expected_did: im_core::ids::Did::parse("did:wba:example.test:existing")
                            .unwrap(),
                        full_handle: im_core::ids::Handle::parse("alice.example.test", "").unwrap(),
                    },
                ),
                default_identity_change: None,
                retry_after_seconds: None,
                retry_at: None,
                warnings: Vec::new(),
            });

        assert_eq!(mapped.state, "join_required");
        assert!(mapped.identity.is_none());
        assert!(mapped.account_id.is_none());
        let join_required = mapped.join_required.expect("typed Join result");
        assert_eq!(join_required.preparation_id, "regjoin_opaque");
        assert_eq!(
            join_required.mode,
            DartHandleRegistrationJoinMode::HandleRecoveryRebind
        );
        assert!(join_required.requires_user_presence);
        assert_eq!(join_required.expected_did, "did:wba:example.test:existing");
        assert_eq!(join_required.full_handle, "alice.example.test");
    }

    #[test]
    fn active_sync_binding_mapping_preserves_unbounded_decimal_generations() {
        let mapped =
            DartActiveSyncAccountBinding::from(im_core::identity::ActiveSyncAccountBinding {
                owner_identity_id: "owner-alice".to_owned(),
                account_id: "account-alice".to_owned(),
                current_did: "did:wba:awiki.info:user:alice".to_owned(),
                protocol_device_id: "device-desktop".to_owned(),
                identity_generation: "184467440737095516160000000000000000001".to_owned(),
                device_auth_generation: "184467440737095516160000000000000000002".to_owned(),
            });

        assert_eq!(
            mapped.identity_generation,
            "184467440737095516160000000000000000001"
        );
        assert_eq!(
            mapped.device_auth_generation,
            "184467440737095516160000000000000000002"
        );
    }

    #[test]
    fn relationship_status_mapping_preserves_directional_truth() {
        let mapped: DartRelationStatus = RelationshipStatus {
            peer: PeerRef::parse("bob.awiki", "").unwrap(),
            did: Did::parse("did:example:bob").unwrap(),
            is_following: false,
            is_follower: true,
            is_friend: false,
            is_blocked: false,
            is_blocked_by: false,
            is_contact: true,
            messaged: true,
            relationship: Some("none".to_owned()),
            warnings: vec!["status-warning".to_owned()],
        }
        .into();

        assert_eq!(mapped.peer, "bob.awiki");
        assert_eq!(mapped.did, "did:example:bob");
        assert!(!mapped.is_following);
        assert!(mapped.is_follower);
        assert!(!mapped.is_friend);
        assert!(mapped.is_contact);
        assert!(mapped.messaged);
        assert_eq!(mapped.relationship.as_deref(), Some("none"));
        assert_eq!(mapped.warnings, vec!["status-warning"]);
    }

    #[test]
    fn realtime_event_mapping_preserves_connection_and_message_events() {
        let event =
            realtime_event_to_dart(ImEvent::ConnectionStateChanged(ConnectionStateChanged {
                state: RealtimeConnectionState::Connected,
                reason: Some("ready".to_string()),
            }));
        assert_eq!(event.kind, "connection_state_changed");
        assert_eq!(event.state.as_deref(), Some("connected"));
        assert_eq!(event.reason.as_deref(), Some("ready"));

        let event = realtime_event_to_dart(ImEvent::MessageReceived(MessageReceivedEvent {
            message: Message {
                id: MessageId::parse("msg-dart-map-1").unwrap(),
                thread: ThreadRef::Direct(PeerRef::parse("did:example:alice", "").unwrap()),
                direction: MessageDirection::Incoming,
                sender: PeerRef::parse("did:example:bob", "").unwrap(),
                receiver: Some(PeerRef::parse("did:example:alice", "").unwrap()),
                group: None,
                body: MessageBodyView::Text {
                    text: "hello".to_string(),
                    kind: MessageKind::Text,
                },
                sent_at: None,
                received_at: None,
                metadata: MessageMetadata::default(),
            },
            attachment_summary: None,
            download_action: None,
            sync: None,
            warnings: Vec::new(),
        }));
        assert_eq!(event.kind, "message_received");
        let message = event.message.expect("message payload");
        assert_eq!(message.id, "msg-dart-map-1");
        assert_eq!(message.thread_kind, "direct");
        assert_eq!(message.body.text.as_deref(), Some("hello"));
    }

    #[test]
    fn realtime_event_mapping_preserves_group_host_local_and_unknown_events() {
        let group = realtime_event_to_dart(ImEvent::GroupUpdated(GroupUpdatedEvent {
            group: GroupRef::parse("did:example:group").unwrap(),
            update_kind: GroupUpdateKind::MessageAdded,
            event_type: None,
            group_event_seq: None,
            group_state_version: None,
            actor_did: None,
            subject_did: None,
            subject_handle: None,
            previous_subject_did: None,
            handle_binding_generation: None,
            membership_status: None,
            changed_at: None,
            sync: None,
        }));
        assert_eq!(group.kind, "group_updated");
        assert_eq!(group.group.as_deref(), Some("did:example:group"));
        assert_eq!(group.update_kind.as_deref(), Some("message_added"));

        let message_update = realtime_event_to_dart(ImEvent::MessageUpdated(MessageUpdatedEvent {
            message_id: MessageId::parse("msg-dart-map-2").unwrap(),
            thread: ThreadRef::Thread(ThreadId::parse("thread-1").unwrap()),
            update_kind: MessageUpdateKind::DeliveryStateChanged,
            sync: None,
        }));
        assert_eq!(message_update.kind, "message_updated");
        assert_eq!(message_update.message_id.as_deref(), Some("msg-dart-map-2"));
        assert_eq!(message_update.thread_kind.as_deref(), Some("thread"));
        assert_eq!(message_update.thread_id.as_deref(), Some("thread-1"));
        assert_eq!(
            message_update.update_kind.as_deref(),
            Some("delivery_state_changed")
        );

        let local = realtime_event_to_dart(ImEvent::LocalNotification(LocalNotificationEvent {
            notification_id: Some("local-1".to_string()),
            title: Some("Title".to_string()),
            body: Some("Body".to_string()),
            source: Some("sdk".to_string()),
        }));
        assert_eq!(local.kind, "local_notification");
        assert_eq!(local.notification_id.as_deref(), Some("local-1"));
        assert_eq!(local.source.as_deref(), Some("sdk"));

        let host = realtime_event_to_dart(ImEvent::HostNotification(HostNotificationEvent {
            event_type: HostNotificationKind::GroupState,
            title: Some("Host".to_string()),
            body: None,
            thread: Some(ThreadRef::Group(
                GroupRef::parse("did:example:group").unwrap(),
            )),
        }));
        assert_eq!(host.kind, "host_notification");
        assert_eq!(host.host_kind.as_deref(), Some("group_state"));
        assert_eq!(host.thread_kind.as_deref(), Some("group"));

        let unknown =
            realtime_event_to_dart(ImEvent::UnknownNotification(UnknownNotificationEvent {
                content_type: Some("application/json".to_string()),
                notification_type: Some("custom.event".to_string()),
                reason: "unsupported notification".to_string(),
                sync: None,
            }));
        assert_eq!(unknown.kind, "unknown_notification");
        assert_eq!(unknown.content_type.as_deref(), Some("application/json"));
        assert_eq!(unknown.notification_type.as_deref(), Some("custom.event"));
        assert_eq!(unknown.reason.as_deref(), Some("unsupported notification"));
    }

    #[test]
    fn realtime_event_mapping_exposes_only_trusted_system_notification_signal() {
        let event = realtime_event_to_dart(ImEvent::SystemNotificationChanged(
            SystemNotificationChangedEvent {
                notification: SystemNotificationSnapshot {
                    event_id: "event-join-1".to_owned(),
                    did: "did:wba:example.test:alice".to_owned(),
                    join_session_id: "join-session-secret-adjacent".to_owned(),
                    kind: SystemNotificationKind::JoinRequested,
                    state: SystemNotificationState::Pending,
                    session_revision: 1,
                    issued_at: "2026-07-23T10:00:00Z".to_owned(),
                    expires_at: "2026-07-23T10:10:00Z".to_owned(),
                    first_seen_at: "2026-07-23T10:00:01Z".to_owned(),
                    terminal: false,
                },
                sync: Some(RealtimeSyncHint {
                    event_id: Some("sync-event-join-1".to_owned()),
                    event_seq: Some("42".to_owned()),
                    event_type: Some("system.notification".to_owned()),
                    domains: std::collections::BTreeSet::from([
                        im_core::realtime::SyncDomain::DeviceRegistry,
                    ]),
                    reason: Some("system.notification".to_owned()),
                    dirty_lanes: Default::default(),
                    sync_dirty: false,
                    gap_detected: false,
                    has_unknown_domain: false,
                }),
            },
        ));

        assert_eq!(event.kind, "system_notification_changed");
        assert_eq!(event.notification_id.as_deref(), Some("event-join-1"));
        assert_eq!(
            event.notification_type.as_deref(),
            Some("awiki.device.join-requested.v1")
        );
        assert!(event.message.is_none());
        assert!(event.body.is_none());
        assert!(event.content_type.is_none());
        assert!(event.reason.is_none());
        let sync = event.sync.expect("reliable sync hint");
        assert_eq!(sync.domains, vec![DartSyncDomain::DeviceRegistry]);
        assert_eq!(sync.reason.as_deref(), Some("system.notification"));
    }

    #[test]
    fn realtime_event_mapping_preserves_sync_hint_without_checkpoint_control() {
        let sync = RealtimeSyncHint {
            event_id: Some("sev-1".to_string()),
            event_seq: Some("42".to_string()),
            event_type: Some("message.created".to_string()),
            domains: std::collections::BTreeSet::from([
                im_core::realtime::SyncDomain::Message,
                im_core::realtime::SyncDomain::Profile,
            ]),
            reason: Some("message_available".to_string()),
            dirty_lanes: Default::default(),
            sync_dirty: true,
            gap_detected: true,
            has_unknown_domain: true,
        };

        let event =
            realtime_event_to_dart(ImEvent::UnknownNotification(UnknownNotificationEvent {
                content_type: Some("application/json".to_string()),
                notification_type: Some("direct.incoming".to_string()),
                reason: "unsupported notification".to_string(),
                sync: Some(sync),
            }));

        let hint = event.sync.expect("sync hint is preserved");
        assert_eq!(
            hint.domains,
            vec![DartSyncDomain::Message, DartSyncDomain::Profile]
        );
        assert_eq!(hint.reason.as_deref(), Some("message_available"));
        assert!(hint.sync_dirty);
        assert!(hint.gap_detected);
        assert!(hint.has_unknown_domain);
    }

    #[test]
    fn group_summary_mapping_exposes_core_canonical_conversation_id() {
        let mapped = DartGroupSummary::from(im_core::groups::GroupSummary {
            id: None,
            did: im_core::ids::GroupRef::parse("did:example:group").unwrap(),
            name: Some("Group".to_owned()),
            display_name: None,
            avatar_uri: None,
            my_role: Some("member".to_owned()),
            membership_status: Some("active".to_owned()),
            member_count: Some(1),
            last_message_at: None,
        });

        assert_eq!(mapped.conversation_id, "group:did:example:group");
    }
}
