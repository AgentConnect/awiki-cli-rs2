import { loadNativeBinding } from './loader.js'
import { createIdentityProviderDispatch } from './provider-bridge.js'
import type {
  NativeHandleRecoveryProgress,
  NativeImCoreNodeClient,
  NativeRealtimeSession,
} from './native.js'
import {
  ImCoreNodeError,
  type AddGroupMemberInput,
  type CreateGroupInput,
  type DisplayProfileBatchInput,
  type DownloadAttachmentInput,
  type GroupInput,
  type GroupMemberPage,
  type GroupMembersInput,
  type GroupRebindRecoverySummary,
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
  type RealtimeEvent,
  type RealtimeOptions,
  type RealtimeSession,
  type RealtimeStatus,
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

  public resumeGroupRebindRecovery(limit?: number): Promise<GroupRebindRecoverySummary> {
    return call(() => this.native.resumeGroupRebindRecovery(limit))
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

  public clearLocalData(): Promise<{ readonly cleared: boolean }> {
    return call(() => this.native.clearLocalData())
  }

  public close(): Promise<void> {
    return call(() => this.native.close())
  }
}

function nativeUint32(value: unknown): number {
  if (!Number.isSafeInteger(value) || (value as number) < 0 || (value as number) > 0xffff_ffff) {
    throw new Error('invalid native unsigned integer')
  }
  return value as number
}

function copyPreparedRegistrationJoinProgress(
  value: PreparedRegistrationJoinProgress,
): PreparedRegistrationJoinProgress {
  return {
    joinSessionId: value.joinSessionId,
    did: value.did,
    localPhase: value.localPhase,
    remoteState: value.remoteState,
    completed: value.completed,
    ...(value.identity === undefined ? {} : { identity: { ...value.identity } }),
  }
}

/** Copy the raw binding object so the public SDK never leaks N-API naming details. */
function copyHandleRecoveryProgress(value: NativeHandleRecoveryProgress): HandleRecoveryProgress {
  const unsupportedE2eeGroupCount = value.impact.unsupportedE2eeGroupCount
    ?? value.impact.unsupportedE2EeGroupCount
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
      unsupportedE2eeGroupCount: nativeUint32(unsupportedE2eeGroupCount),
      unsupportedDidOnlyGroupCount: nativeUint32(value.impact.unsupportedDidOnlyGroupCount),
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
