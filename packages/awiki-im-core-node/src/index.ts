import { loadNativeBinding } from './loader.js'
import type {
  NativeExternalHttpAuthAttempt,
  NativeImCoreNodeClient,
  NativeRealtimeSession,
} from './native.js'
import {
  ImCoreNodeError,
  type AddGroupMemberInput,
  type CreateGroupInput,
  type DisplayProfileBatchInput,
  type DownloadAttachmentInput,
  type ExternalHttpAuthAttempt,
  type ExternalHttpHeader,
  type ExternalHttpRequest,
  type ExternalHttpResponse,
  type HistoryInput,
  type ImCoreNodeClient,
  type ImCoreNodeOpenOptions,
  type MarkReadResult,
  type NodeConversation,
  type NodeDownload,
  type NodeDisplayProfile,
  type NodeGroup,
  type NodeGroupMember,
  type NodeIdentity,
  type NodeMessage,
  type NodePeer,
  type OtpChallenge,
  type Page,
  type PageInput,
  type RegistrationInput,
  type RegistrationWithOtp,
  type SendAttachmentInput,
  type SendTextInput,
  type SyncOptions,
  type SyncResult,
  type RealtimeEvent,
  type RealtimeOptions,
  type RealtimeSession,
  type RealtimeStatus,
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

  public updateDisplayName(displayName: string): Promise<NodeIdentity> {
    return call(() => this.native.updateDisplayName(displayName))
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

  public sendAttachment(input: SendAttachmentInput): Promise<NodeMessage> {
    const bytes = Buffer.from(input.bytes.buffer, input.bytes.byteOffset, input.bytes.byteLength)
    return call(() => this.native.sendAttachment({ ...input, bytes }))
  }

  public async downloadAttachment(input: DownloadAttachmentInput): Promise<NodeDownload> {
    const value = await call(() => this.native.downloadAttachment(input))
    return { attachment: value.attachment, bytes: value.bytes }
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
  const native = await call(() => binding.openNativeClient(options))
  return new RustImCoreNodeClient(native)
}
