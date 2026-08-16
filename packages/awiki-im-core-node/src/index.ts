import { loadNativeBinding } from './loader.js'
import type { NativeImCoreNodeClient } from './native.js'
import {
  ImCoreNodeError,
  type DownloadAttachmentInput,
  type HistoryInput,
  type ImCoreNodeClient,
  type ImCoreNodeOpenOptions,
  type MarkReadResult,
  type NodeConversation,
  type NodeDownload,
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

  public updateDisplayName(displayName: string): Promise<NodeIdentity> {
    return call(() => this.native.updateDisplayName(displayName))
  }

  public resolvePeer(peer: string): Promise<NodePeer> {
    return call(() => this.native.resolvePeer(peer))
  }

  public syncNow(input?: SyncOptions): Promise<SyncResult> {
    return call(() => this.native.syncNow(input))
  }

  public listConversations(input?: PageInput): Promise<Page<NodeConversation>> {
    return call(() => this.native.listConversations(input))
  }

  public getHistory(input: HistoryInput): Promise<Page<NodeMessage>> {
    return call(() => this.native.getHistory(input))
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
