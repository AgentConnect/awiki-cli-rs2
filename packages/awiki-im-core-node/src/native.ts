import type {
  AdminDeviceJoinProgress,
  AddGroupMemberInput,
  CreateGroupInput,
  CurrentDeviceSummary,
  DisplayProfileBatchInput,
  DeviceJoinApprovalPrompt,
  DeviceJoinRequestNotice,
  DeviceRegistrySnapshot,
  DeviceRevokeResult,
  DownloadAttachmentInput,
  ExternalHttpHeader,
  ExternalHttpRequest,
  ExternalHttpResponse,
  GroupInput,
  GroupMemberPage,
  GroupMembersInput,
  GroupRebindRecoverySummary,
  HandleRecoveryOperationInput,
  HandleRecoveryOperationSummary,
  HandleRecoveryAttestationResult,
  HandleRecoveryOtpInput,
  HandleRecoveryOtpResult,
  HandleRecoveryPrepareInput,
  HandleRecoveryPhase,
  HistoryInput,
  ImCoreNodeOpenOptions,
  MailAccount,
  MailInboxInput,
  MailInboxPage,
  MailMessage,
  MarkMailReadInput,
  MarkMailReadResult,
  MarkReadResult,
  NodeConversation,
  NodeDownload,
  NodeDisplayProfile,
  NodeGroup,
  NodeGroupMember,
  NodeProfile,
  NodeIdentity,
  NodeMessage,
  NodePeer,
  OtpChallenge,
  Page,
  PageInput,
  PreparedRegistrationJoinInput,
  PreparedRegistrationJoinProgress,
  PreparedRegistrationJoinResumeInput,
  LocalDeviceJoinSession,
  RegistrationInput,
  RegistrationOutcome,
  RegistrationWithOtp,
  RemoveGroupMemberInput,
  SendAttachmentInput,
  SendMailInput,
  SendMailResult,
  SendTextInput,
  SendPayloadInput,
  SyncOptions,
  SyncResult,
  StartDeviceJoinVerificationInput,
  UpdateProfileInput,
  RealtimeEvent,
  RealtimeOptions,
  RealtimeStatus,
} from './types.js'

/** Raw N-API recovery shape. Keep it separate from the stable public facade. */
export interface NativeHandleRecoveryProgress {
  readonly operationId: string
  readonly ownerIdentityId: string
  readonly fullHandle: string
  readonly previousDid?: string
  readonly currentDid: string
  readonly phase: HandleRecoveryPhase
  readonly failureCode?: string
  readonly retryable: boolean
  readonly impact: {
    readonly localOrdinaryDataWillMigrate: boolean
    readonly otherDevicesMustRejoin: boolean
    readonly unsupportedE2eeGroupCount?: number
    /** Compatibility with native packages built before the explicit N-API field name. */
    readonly unsupportedE2EeGroupCount?: number
    readonly unsupportedDidOnlyGroupCount: number
  }
}

export interface NativeExternalHttpAuthAttempt {
  getTargetUrl(): string
  getMethod(): string
  getHeaderPatch(): ExternalHttpHeader[]
  getRetryCount(): number
  handleResponse(response: ExternalHttpResponse): Promise<NativeExternalHttpAuthAttempt | null>
}

export interface NativeRealtimeSession {
  nextEvent(): Promise<RealtimeEvent | null>
  getStatus(): Promise<RealtimeStatus>
  stop(): Promise<void>
}

export interface NativeImCoreNodeClient {
  prepareExternalHttpRequest(
    input: Omit<ExternalHttpRequest, 'headers' | 'body'> & {
      readonly headers: ExternalHttpHeader[]
      readonly body?: Buffer
    },
  ): Promise<NativeExternalHttpAuthAttempt>
  getDefaultIdentity(): Promise<NodeIdentity | null>
  requestRegistrationOtp(input: RegistrationInput): Promise<OtpChallenge>
  completeRegistration(input: RegistrationWithOtp): Promise<NodeIdentity>
  completeRegistrationWithOutcome(input: RegistrationWithOtp): Promise<RegistrationOutcome>
  beginPreparedRegistrationJoin(input: PreparedRegistrationJoinInput): Promise<PreparedRegistrationJoinProgress>
  resumePreparedRegistrationJoin(input: PreparedRegistrationJoinResumeInput): Promise<PreparedRegistrationJoinProgress>
  listLocalDeviceJoinSessions(): Promise<LocalDeviceJoinSession[]>
  cancelPreparedRegistrationJoin(input: PreparedRegistrationJoinResumeInput): Promise<LocalDeviceJoinSession>
  getCurrentDeviceSummary(): Promise<CurrentDeviceSummary>
  getDeviceRegistry(): Promise<DeviceRegistrySnapshot>
  listLocalDeviceJoinRequests(): Promise<DeviceJoinRequestNotice[]>
  startDeviceJoinVerification(input: StartDeviceJoinVerificationInput): Promise<AdminDeviceJoinProgress>
  getLocalDeviceJoinVerificationProgress(input: PreparedRegistrationJoinResumeInput): Promise<AdminDeviceJoinProgress>
  prepareDeviceJoinApproval(input: { readonly joinSessionId: string; readonly sasConfirmed: boolean }): Promise<DeviceJoinApprovalPrompt>
  confirmDeviceJoinApproval(input: { readonly approvalHandle: string; readonly userPresenceConfirmed: boolean }): Promise<AdminDeviceJoinProgress>
  rejectDeviceJoin(input: { readonly joinSessionId: string; readonly reason: 'user_rejected' | 'sas_mismatch' }): Promise<AdminDeviceJoinProgress>
  revokeDevice(input: { readonly targetDeviceId: string; readonly userPresenceConfirmed: boolean }): Promise<DeviceRevokeResult>
  updateDisplayName(displayName: string): Promise<NodeIdentity>
  getProfile(): Promise<NodeProfile>
  updateProfile(input: UpdateProfileInput): Promise<NodeProfile>
  resolvePeer(peer: string): Promise<NodePeer>
  hydrateDisplayProfiles(input: DisplayProfileBatchInput): Promise<NodeDisplayProfile[]>
  createGroup(input: CreateGroupInput): Promise<NodeGroup>
  addGroupMember(input: AddGroupMemberInput): Promise<NodeGroupMember>
  getGroup(input: GroupInput): Promise<NodeGroup>
  listGroups(input?: PageInput): Promise<Page<NodeGroup>>
  joinGroup(input: GroupInput): Promise<NodeGroup>
  leaveGroup(input: GroupInput): Promise<void>
  listGroupMembers(input: GroupMembersInput): Promise<GroupMemberPage>
  removeGroupMember(input: RemoveGroupMemberInput): Promise<NodeGroupMember>
  resumeGroupRebindRecovery(limit?: number): Promise<GroupRebindRecoverySummary>
  syncNow(input?: SyncOptions): Promise<SyncResult>
  startRealtime(input?: RealtimeOptions): Promise<NativeRealtimeSession>
  listConversations(input?: PageInput): Promise<Page<NodeConversation>>
  getHistory(input: HistoryInput): Promise<Page<NodeMessage>>
  getLocalConversationTimeline(input: HistoryInput): Promise<Page<NodeMessage>>
  markConversationRead(conversationId: string): Promise<MarkReadResult>
  sendText(input: SendTextInput): Promise<NodeMessage>
  sendPayload(input: SendPayloadInput): Promise<NodeMessage>
  sendAttachment(input: Omit<SendAttachmentInput, 'bytes'> & { readonly bytes: Buffer }): Promise<NodeMessage>
  downloadAttachment(input: DownloadAttachmentInput): Promise<Omit<NodeDownload, 'bytes'> & { readonly bytes: Buffer }>
  getMailAccount(): Promise<MailAccount>
  listMailInbox(input?: MailInboxInput): Promise<MailInboxPage>
  readMail(messageId: string): Promise<MailMessage>
  markMailRead(input: MarkMailReadInput): Promise<MarkMailReadResult>
  sendMail(input: SendMailInput): Promise<SendMailResult>
  requestHandleRecoveryOtp(input: HandleRecoveryOtpInput): Promise<HandleRecoveryOtpResult>
  prepareHandleRecovery(input: HandleRecoveryPrepareInput): Promise<NativeHandleRecoveryProgress>
  activateHandleRecovery(input: HandleRecoveryOperationInput): Promise<NativeHandleRecoveryProgress>
  getHandleRecoveryStatus(input: HandleRecoveryOperationInput): Promise<NativeHandleRecoveryProgress>
  resumeHandleRecovery(input: HandleRecoveryOperationInput): Promise<NativeHandleRecoveryProgress>
  issueHandleRecoveryAttestation(input: HandleRecoveryOperationInput): Promise<HandleRecoveryAttestationResult>
  discardHandleRecovery(input: HandleRecoveryOperationInput): Promise<HandleRecoveryOperationSummary>
  clearLocalData(): Promise<{ readonly cleared: boolean }>
  close(): Promise<void>
}

export interface NativeBinding {
  readonly nativeApiVersion: () => number
  readonly openNativeClient: (options: ImCoreNodeOpenOptions) => Promise<NativeImCoreNodeClient>
}
