import { loadNativeBinding } from './loader.js'
import { createIdentityProviderDispatch } from './provider-bridge.js'
import type {
  NativeExternalHttpAuthAttempt,
  NativeHandleRecoveryProgress,
  NativeImCoreNodeClient,
  NativeRealtimeSession,
} from './native.js'
import {
  ImCoreNodeError,
  type AdminDeviceJoinProgress,
  type AddGroupMemberInput,
  type CreateGroupInput,
  type CurrentDeviceSummary,
  type DisplayProfileBatchInput,
  type DeviceJoinApprovalPrompt,
  type DeviceJoinRequestNotice,
  type DeviceRegistrySnapshot,
  type DeviceRevokeResult,
  type DownloadAttachmentInput,
  type ExternalHttpAuthAttempt,
  type ExternalHttpHeader,
  type ExternalHttpRequest,
  type ExternalHttpResponse,
  type GroupInput,
  type GroupMemberPage,
  type GroupMembersInput,
  type HandleRecoveryOperationInput,
  type HandleRecoveryOperationSummary,
  type HandleRecoveryOtpInput,
  type HandleRecoveryOtpResult,
  type HandleRecoveryPrepareInput,
  type HandleRecoveryProgress,
  type HistoryInput,
  type ImCoreNodeClient,
  type ImCoreNodeOpenOptions,
  type MailAccount,
  type MailInboxInput,
  type MailInboxPage,
  type MailMessage,
  type MarkMailReadInput,
  type MarkMailReadResult,
  type MarkReadResult,
  type NodeConversation,
  type NodeDownload,
  type NodeDisplayProfile,
  type NodeGroup,
  type NodeGroupMember,
  type NodeIdentity,
  type NodeMessage,
  type NodeProfile,
  type NodePeer,
  type OtpChallenge,
  type Page,
  type PageInput,
  type LocalDeviceJoinSession,
  type PreparedRegistrationJoinInput,
  type PreparedRegistrationJoinProgress,
  type PreparedRegistrationJoinResumeInput,
  type RegistrationInput,
  type RegistrationOutcome,
  type RegistrationWithOtp,
  type RemoveGroupMemberInput,
  type SendAttachmentInput,
  type SendMailInput,
  type SendMailResult,
  type SendTextInput,
  type SendPayloadInput,
  type SyncOptions,
  type SyncResult,
  type StartDeviceJoinVerificationInput,
  type RealtimeEvent,
  type RealtimeOptions,
  type RealtimeSession,
  type RealtimeStatus,
  type RootKeyTransferPreparation,
  type RootKeyTransferSendResult,
  type UpdateProfileInput,
} from './types.js'

export * from './types.js'

interface NativeSafeError {
  readonly code: string
  readonly safeMessage: string
  readonly retryable: boolean
}

function nativeSafeError(error: unknown): NativeSafeError | undefined {
  if (!(error instanceof Error)) return undefined
  try {
    const value: unknown = JSON.parse(error.message)
    if (
      typeof value === 'object' && value !== null
      && 'code' in value && typeof value.code === 'string'
      && 'safeMessage' in value && typeof value.safeMessage === 'string'
      && 'retryable' in value && typeof value.retryable === 'boolean'
    ) return value as NativeSafeError
  }
  catch {}
  return undefined
}

async function call<T>(operation: () => Promise<T>): Promise<T> {
  try {
    return await operation()
  }
  catch (error) {
    const safe = nativeSafeError(error)
    if (safe) throw new ImCoreNodeError(safe.code, safe.safeMessage, safe.retryable)
    if (error instanceof ImCoreNodeError) throw error
    throw new ImCoreNodeError('internal', 'The IM operation failed internally.', false)
  }
}

class RustImCoreNodeClient implements ImCoreNodeClient {
  public constructor(private readonly native: NativeImCoreNodeClient) {}

  public async prepareExternalHttpRequest(input: ExternalHttpRequest): Promise<ExternalHttpAuthAttempt> {
    const body = input.body === undefined
      ? undefined
      : Buffer.from(input.body.buffer, input.body.byteOffset, input.body.byteLength)
    const native = await call(() => this.native.prepareExternalHttpRequest({
      url: input.url,
      method: input.method,
      headers: copyHeaders(input.headers),
      ...(body === undefined ? {} : { body }),
    }))
    return new RustExternalHttpAuthAttempt(native)
  }

  public getDefaultIdentity(): Promise<NodeIdentity | null> {
    return call(() => this.native.getDefaultIdentity())
  }

  public requestRegistrationOtp(input: RegistrationInput): Promise<OtpChallenge> {
    return call(() => this.native.requestRegistrationOtp(input))
  }

  public completeRegistration(input: RegistrationWithOtp): Promise<NodeIdentity> {
    return call(() => this.native.completeRegistration(input))
  }

  public completeRegistrationWithOutcome(input: RegistrationWithOtp): Promise<RegistrationOutcome> {
    return call(() => this.native.completeRegistrationWithOutcome(input))
  }

  public beginPreparedRegistrationJoin(input: PreparedRegistrationJoinInput): Promise<PreparedRegistrationJoinProgress> {
    return call(async () => copyPreparedRegistrationJoinProgress(
      await this.native.beginPreparedRegistrationJoin(input),
    ))
  }

  public resumePreparedRegistrationJoin(input: PreparedRegistrationJoinResumeInput): Promise<PreparedRegistrationJoinProgress> {
    return call(async () => copyPreparedRegistrationJoinProgress(
      await this.native.resumePreparedRegistrationJoin(input),
    ))
  }

  public listLocalDeviceJoinSessions(): Promise<readonly LocalDeviceJoinSession[]> {
    return call(async () => (await this.native.listLocalDeviceJoinSessions()).map(value => ({ ...value })))
  }

  public cancelPreparedRegistrationJoin(input: PreparedRegistrationJoinResumeInput): Promise<LocalDeviceJoinSession> {
    return call(async () => ({ ...await this.native.cancelPreparedRegistrationJoin(input) }))
  }

  public getCurrentDeviceSummary(): Promise<CurrentDeviceSummary> {
    return call(async () => ({ ...await this.native.getCurrentDeviceSummary() }))
  }

  public getDeviceRegistry(): Promise<DeviceRegistrySnapshot> {
    return call(async () => {
      const value = await this.native.getDeviceRegistry()
      return { ...value, devices: value.devices.map(device => ({ ...device })) }
    })
  }

  public listLocalDeviceJoinRequests(): Promise<readonly DeviceJoinRequestNotice[]> {
    return call(async () => (await this.native.listLocalDeviceJoinRequests()).map(value => ({ ...value })))
  }

  public startDeviceJoinVerification(input: StartDeviceJoinVerificationInput): Promise<AdminDeviceJoinProgress> {
    return call(async () => ({ ...await this.native.startDeviceJoinVerification(input) }))
  }

  public getLocalDeviceJoinVerificationProgress(input: PreparedRegistrationJoinResumeInput): Promise<AdminDeviceJoinProgress> {
    return call(async () => ({ ...await this.native.getLocalDeviceJoinVerificationProgress(input) }))
  }

  public prepareDeviceJoinApproval(input: { readonly joinSessionId: string; readonly sasConfirmed: boolean }): Promise<DeviceJoinApprovalPrompt> {
    return call(async () => ({ ...await this.native.prepareDeviceJoinApproval(input) }))
  }

  public confirmDeviceJoinApproval(input: { readonly approvalHandle: string; readonly userPresenceConfirmed: boolean }): Promise<AdminDeviceJoinProgress> {
    return call(async () => ({ ...await this.native.confirmDeviceJoinApproval(input) }))
  }

  public rejectDeviceJoin(input: { readonly joinSessionId: string; readonly reason: 'user_rejected' | 'sas_mismatch' }): Promise<AdminDeviceJoinProgress> {
    return call(async () => ({ ...await this.native.rejectDeviceJoin(input) }))
  }

  public revokeDevice(input: { readonly targetDeviceId: string; readonly userPresenceConfirmed: boolean }): Promise<DeviceRevokeResult> {
    return call(async () => ({ ...await this.native.revokeDevice(input) }))
  }

  public prepareRootKeyTransfer(input: { readonly recipientDeviceId: string }): Promise<RootKeyTransferPreparation> {
    return call(async () => {
      const value = await this.native.prepareRootKeyTransfer(input)
      return { ...value, recipient: { ...value.recipient } }
    })
  }

  public confirmAndSendRootKeyTransfer(input: {
    readonly authorizationHandle: string
    readonly userPresenceConfirmed: boolean
  }): Promise<RootKeyTransferSendResult> {
    return call(async () => ({ ...await this.native.confirmAndSendRootKeyTransfer(input) }))
  }

  public confirmUserPresence(input: { readonly reason: string }): Promise<boolean> {
    return call(() => this.native.confirmUserPresence(input))
  }

  public updateDisplayName(displayName: string): Promise<NodeIdentity> {
    return call(() => this.native.updateDisplayName(displayName))
  }

  public getProfile(): Promise<NodeProfile> {
    return call(() => this.native.getProfile())
  }

  public updateProfile(input: UpdateProfileInput): Promise<NodeProfile> {
    return call(() => this.native.updateProfile({
      ...input,
      ...(input.tags === undefined ? {} : { tags: [...input.tags] }),
    }))
  }

  public resolvePeer(peer: string): Promise<NodePeer> {
    return call(() => this.native.resolvePeer(peer))
  }

  public hydrateDisplayProfiles(input: DisplayProfileBatchInput): Promise<readonly NodeDisplayProfile[]> {
    return call(() => this.native.hydrateDisplayProfiles({ peers: [...input.peers] }))
  }

  public createGroup(input: CreateGroupInput): Promise<NodeGroup> {
    return call(() => this.native.createGroup(input))
  }

  public addGroupMember(input: AddGroupMemberInput): Promise<NodeGroupMember> {
    return call(() => this.native.addGroupMember(input))
  }

  public getGroup(input: GroupInput): Promise<NodeGroup> {
    return call(() => this.native.getGroup(input))
  }

  public listGroups(input?: PageInput): Promise<Page<NodeGroup>> {
    return call(() => this.native.listGroups(input))
  }

  public joinGroup(input: GroupInput): Promise<NodeGroup> {
    return call(() => this.native.joinGroup(input))
  }

  public leaveGroup(input: GroupInput): Promise<void> {
    return call(() => this.native.leaveGroup(input))
  }

  public listGroupMembers(input: GroupMembersInput): Promise<GroupMemberPage> {
    return call(() => this.native.listGroupMembers(input))
  }

  public removeGroupMember(input: RemoveGroupMemberInput): Promise<NodeGroupMember> {
    return call(() => this.native.removeGroupMember(input))
  }

  public syncNow(input?: SyncOptions): Promise<SyncResult> {
    return call(() => this.native.syncNow(input))
  }

  public async startRealtime(input?: RealtimeOptions): Promise<RealtimeSession> {
    const native = await call(() => this.native.startRealtime(input))
    return new RustRealtimeSession(native)
  }

  public listConversations(input?: PageInput): Promise<Page<NodeConversation>> {
    return call(() => this.native.listConversations(input))
  }

  public getHistory(input: HistoryInput): Promise<Page<NodeMessage>> {
    return call(() => this.native.getHistory(input))
  }

  public getLocalConversationTimeline(input: HistoryInput): Promise<Page<NodeMessage>> {
    return call(() => this.native.getLocalConversationTimeline(input))
  }

  public markConversationRead(conversationId: string): Promise<MarkReadResult> {
    return call(() => this.native.markConversationRead(conversationId))
  }

  public sendText(input: SendTextInput): Promise<NodeMessage> {
    return call(() => this.native.sendText(input))
  }

  public sendPayload(input: SendPayloadInput): Promise<NodeMessage> {
    return call(() => this.native.sendPayload(input))
  }

  public sendAttachment(input: SendAttachmentInput): Promise<NodeMessage> {
    const bytes = Buffer.from(input.bytes.buffer, input.bytes.byteOffset, input.bytes.byteLength)
    return call(() => this.native.sendAttachment({ ...input, bytes }))
  }

  public async downloadAttachment(input: DownloadAttachmentInput): Promise<NodeDownload> {
    const value = await call(() => this.native.downloadAttachment(input))
    return { attachment: value.attachment, bytes: value.bytes }
  }

  public getMailAccount(): Promise<MailAccount> {
    return call(() => this.native.getMailAccount())
  }

  public listMailInbox(input?: MailInboxInput): Promise<MailInboxPage> {
    return call(() => this.native.listMailInbox(input))
  }

  public readMail(messageId: string): Promise<MailMessage> {
    return call(() => this.native.readMail(messageId))
  }

  public markMailRead(input: MarkMailReadInput): Promise<MarkMailReadResult> {
    return call(() => this.native.markMailRead(input))
  }

  public sendMail(input: SendMailInput): Promise<SendMailResult> {
    return call(() => this.native.sendMail(input))
  }

  public requestHandleRecoveryOtp(input: HandleRecoveryOtpInput): Promise<HandleRecoveryOtpResult> {
    return call(() => this.native.requestHandleRecoveryOtp(input))
  }

  public prepareHandleRecovery(input: HandleRecoveryPrepareInput): Promise<HandleRecoveryProgress> {
    return call(async () => copyHandleRecoveryProgress(await this.native.prepareHandleRecovery(input)))
  }

  public activateHandleRecovery(input: HandleRecoveryOperationInput): Promise<HandleRecoveryProgress> {
    return call(async () => copyHandleRecoveryProgress(await this.native.activateHandleRecovery(input)))
  }

  public getHandleRecoveryStatus(input: HandleRecoveryOperationInput): Promise<HandleRecoveryProgress> {
    return call(async () => copyHandleRecoveryProgress(await this.native.getHandleRecoveryStatus(input)))
  }

  public resumeHandleRecovery(input: HandleRecoveryOperationInput): Promise<HandleRecoveryProgress> {
    return call(async () => copyHandleRecoveryProgress(await this.native.resumeHandleRecovery(input)))
  }

  public discardHandleRecovery(input: HandleRecoveryOperationInput): Promise<HandleRecoveryOperationSummary> {
    return call(() => this.native.discardHandleRecovery(input))
  }

  public retireDefaultIdentityForRejoin(): Promise<void> {
    return call(() => this.native.retireDefaultIdentityForRejoin())
  }

  public clearLocalData(): Promise<{ readonly cleared: boolean }> {
    return call(() => this.native.clearLocalData())
  }

  public close(): Promise<void> {
    return call(() => this.native.close())
  }
}

class RustExternalHttpAuthAttempt implements ExternalHttpAuthAttempt {
  public readonly targetUrl: string
  public readonly method: string
  public readonly headerPatch: readonly ExternalHttpHeader[]
  public readonly retryCount: number

  public constructor(private readonly native: NativeExternalHttpAuthAttempt) {
    this.targetUrl = native.getTargetUrl()
    this.method = native.getMethod()
    this.headerPatch = copyHeaders(native.getHeaderPatch())
    this.retryCount = native.getRetryCount()
  }

  public async handleResponse(response: ExternalHttpResponse): Promise<ExternalHttpAuthAttempt | null> {
    const retry = await call(() => this.native.handleResponse({
      statusCode: response.statusCode,
      headers: copyHeaders(response.headers),
    }))
    return retry === null ? null : new RustExternalHttpAuthAttempt(retry)
  }
}

function copyHeaders(headers: readonly ExternalHttpHeader[]): ExternalHttpHeader[] {
  return headers.map(header => ({ name: header.name, value: header.value }))
}

function copyPreparedRegistrationJoinProgress(
  value: PreparedRegistrationJoinProgress,
): PreparedRegistrationJoinProgress {
  return {
    joinSessionId: value.joinSessionId,
    did: value.did,
    localPhase: value.localPhase,
    remoteState: value.remoteState,
    expiresAt: value.expiresAt,
    ...(value.sas === undefined ? {} : { sas: value.sas }),
    completed: value.completed,
    ...(value.identity === undefined ? {} : { identity: { ...value.identity } }),
  }
}

/** Copy the raw binding object so the public SDK never leaks N-API naming details. */
function copyHandleRecoveryProgress(value: NativeHandleRecoveryProgress): HandleRecoveryProgress {
  return {
    operationId: value.operationId,
    ownerIdentityId: value.ownerIdentityId,
    fullHandle: value.fullHandle,
    ...(value.previousDid === undefined ? {} : { previousDid: value.previousDid }),
    currentDid: value.currentDid,
    phase: value.phase,
    ...(value.failureCode === undefined ? {} : { failureCode: value.failureCode }),
    retryable: value.retryable,
    impact: {
      localOrdinaryDataWillMigrate: value.impact.localOrdinaryDataWillMigrate,
      otherDevicesMustRejoin: value.impact.otherDevicesMustRejoin,
    },
  }
}

class RustRealtimeSession implements RealtimeSession {
  public constructor(private readonly native: NativeRealtimeSession) {}

  public nextEvent(): Promise<RealtimeEvent | null> {
    return call(() => this.native.nextEvent())
  }

  public getStatus(): Promise<RealtimeStatus> {
    return call(() => this.native.getStatus())
  }

  public stop(): Promise<void> {
    return call(() => this.native.stop())
  }
}

/**
 * Open one Rust IM Core for the supplied absolute state root.
 *
 * All I/O methods are Promise-based. Call `close()` during host teardown; GC is
 * only a last-resort cancellation path. A second process or instance opening
 * the same state root fails with `state_in_use`.
 */
export async function openImCoreNodeClient(options: ImCoreNodeOpenOptions): Promise<ImCoreNodeClient> {
  const binding = loadNativeBinding()
  const { identityProvider, ...nativeOptions } = options
  const dispatch = identityProvider === undefined
    ? undefined
    : createIdentityProviderDispatch(identityProvider)
  const native = await call(() => binding.openNativeClient(nativeOptions, dispatch))
  return new RustImCoreNodeClient(native)
}
