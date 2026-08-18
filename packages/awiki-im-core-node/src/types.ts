/** State and service configuration owned by the Node host. */
export interface ImCoreNodeOpenOptions {
  /** Absolute, process-exclusive root for identities, SQLite, cache, and metadata. */
  readonly stateRoot: string
  readonly serviceBaseUrl: string
  readonly didDomain: string
  readonly userServiceEndpoint?: string
  readonly messageServiceEndpoint?: string
  readonly anpServiceEndpoint?: string
  readonly anpServiceDid?: string
  /** Default timeout for one operation, from 1 to 600000 milliseconds. */
  readonly operationTimeoutMs?: number
  /** Timeout for bounded synchronization before list reads. */
  readonly syncTimeoutMs?: number
  /** Test-only exception for literal loopback HTTP external-auth targets. */
  readonly externalHttpAllowInsecureLoopbackForTesting?: boolean
}

/** One exact HTTP field. Authentication field values are sensitive. */
export interface ExternalHttpHeader {
  readonly name: string
  readonly value: string
}

/** Exact request bytes authenticated by Rust and sent by a trusted host. */
export interface ExternalHttpRequest {
  readonly url: string
  readonly method: string
  readonly headers: readonly ExternalHttpHeader[]
  /** `undefined` means no body; an empty Uint8Array is an explicit empty body. */
  readonly body?: Uint8Array
}

/** Response metadata observed without transferring or consuming its body. */
export interface ExternalHttpResponse {
  readonly statusCode: number
  readonly headers: readonly ExternalHttpHeader[]
}

/** Opaque, single-use external HTTP authentication attempt. */
export interface ExternalHttpAuthAttempt {
  /** Canonical target URI covered by the signature. */
  readonly targetUrl: string
  readonly method: string
  /** Apply this patch only to the exact request represented by this attempt. */
  readonly headerPatch: readonly ExternalHttpHeader[]
  readonly retryCount: number
  /** Returns the only allowed retry, or `null` when authentication is complete. */
  handleResponse(response: ExternalHttpResponse): Promise<ExternalHttpAuthAttempt | null>
}

/** Public identity projection. No token, private key, or local path is exposed. */
export interface NodeIdentity {
  readonly identityId: string
  readonly did: string
  readonly handle?: string
  readonly displayName?: string
  /** Stable Unix milliseconds represented as a decimal string. */
  readonly registeredAtMs: string
}

/** First stage of phone registration. */
export interface RegistrationInput {
  readonly handle: string
  readonly phone: string
}

/** Second stage of phone registration. */
export interface RegistrationWithOtp extends RegistrationInput {
  readonly otp: string
}

/** Server-issued retry boundary for a registration OTP. */
export interface OtpChallenge {
  readonly retryAfterSeconds: number
  /** RFC 3339 UTC timestamp. */
  readonly retryAt: string
}

/** Verified directory result with the canonical Direct conversation ID. */
export interface NodePeer {
  readonly did: string
  readonly handle?: string
  readonly displayName?: string
  readonly conversationId: string
}

/** Opaque cursor page input. */
export interface PageInput {
  readonly cursor?: string
  readonly limit?: number
}

/** Opaque cursor page. */
export interface Page<T> {
  readonly items: readonly T[]
  readonly nextCursor?: string
  readonly hasMore: boolean
}

/** Explicit reliable-sync input. */
export interface SyncOptions {
  readonly reason?: string
  readonly limit?: number
  readonly timeoutMs?: number
}

export type SyncStatus = 'idle' | 'changed' | 'recovery_required' | 'retryable_failure' | 'auth_revoked'

/** Product-safe reliable-sync result. */
export interface SyncResult {
  readonly status: SyncStatus
  readonly eventsApplied: number
  readonly pagesFetched: number
  readonly messagesHydrated: number
  readonly duplicatesSkipped: number
  readonly changedConversationIds: readonly string[]
  readonly warnings: readonly string[]
}

/** One canonical Direct or Group conversation. */
export interface NodeConversation {
  readonly id: string
  readonly kind: 'direct' | 'group'
  readonly peerDid?: string
  readonly peerHandle?: string
  readonly groupDid?: string
  readonly title?: string
  readonly participants: readonly string[]
  readonly unreadCount: number
  readonly messageCount: number
  /** RFC 3339 timestamp. */
  readonly lastMessageAt?: string
  readonly lastMessage?: NodeMessage
}

/** Single attachment metadata. Byte counts are decimal strings. */
export interface NodeAttachment {
  readonly id: string
  readonly fileName: string
  readonly mimeType: string
  readonly sizeBytes: string
  readonly digestB64u: string
  readonly sha256Hex?: string
}

export interface NodeMessageContent {
  readonly kind: 'text' | 'attachment' | 'payload' | 'unsupported'
  readonly text?: string
  readonly attachment?: NodeAttachment
  readonly caption?: string
  readonly payloadJson?: string
  readonly unsupportedContentType?: string
}

/** Canonically routed message projection. */
export interface NodeMessage {
  readonly id: string
  readonly conversationId: string
  readonly conversationKind: 'direct' | 'group'
  readonly senderDid: string
  readonly senderHandle?: string
  readonly senderDisplayName?: string
  /** RFC 3339 timestamp. */
  readonly sentAt?: string
  readonly outgoing: boolean
  readonly content: NodeMessageContent
}

export interface HistoryInput extends PageInput {
  readonly conversationId: string
}

export interface MarkReadResult {
  readonly updatedCount: number
  readonly remoteAcknowledged: boolean
  readonly partial: boolean
  readonly fallbackUsed: boolean
  readonly pendingRemoteAck: boolean
  readonly warnings: readonly string[]
}

export interface SendTextInput {
  readonly conversationId: string
  readonly text: string
  readonly markdown?: boolean
  readonly clientMessageId?: string
  readonly idempotencyKey?: string
}

/** Single attachment send. `bytes` crosses N-API as Uint8Array, never JSON/base64. */
export interface SendAttachmentInput {
  readonly conversationId: string
  readonly fileName: string
  readonly mimeType: string
  readonly bytes: Uint8Array
  readonly caption?: string
  readonly clientMessageId?: string
  readonly idempotencyKey?: string
}

export interface DownloadAttachmentInput {
  readonly conversationId: string
  readonly messageId: string
  readonly attachmentId?: string
  readonly timeoutMs?: number
}

/** Verified in-memory download. */
export interface NodeDownload {
  readonly attachment: NodeAttachment
  readonly bytes: Uint8Array
}

/** Stable, redacted error taxonomy from the Rust bridge. */
export class ImCoreNodeError extends Error {
  public readonly name = 'ImCoreNodeError'

  public constructor(
    public readonly code: string,
    public readonly safeMessage: string,
    public readonly retryable: boolean,
  ) {
    super(safeMessage)
  }
}

/** Environment-scoped Promise API backed by one Rust ImCore/ImClient pair. */
export interface ImCoreNodeClient {
  prepareExternalHttpRequest(input: ExternalHttpRequest): Promise<ExternalHttpAuthAttempt>
  getDefaultIdentity(): Promise<NodeIdentity | null>
  requestRegistrationOtp(input: RegistrationInput): Promise<OtpChallenge>
  completeRegistration(input: RegistrationWithOtp): Promise<NodeIdentity>
  updateDisplayName(displayName: string): Promise<NodeIdentity>
  resolvePeer(peer: string): Promise<NodePeer>
  syncNow(input?: SyncOptions): Promise<SyncResult>
  listConversations(input?: PageInput): Promise<Page<NodeConversation>>
  getHistory(input: HistoryInput): Promise<Page<NodeMessage>>
  /** Read only the committed local timeline; never starts sync, history, or Directory RPC. */
  getLocalConversationTimeline(input: HistoryInput): Promise<Page<NodeMessage>>
  markConversationRead(conversationId: string): Promise<MarkReadResult>
  sendText(input: SendTextInput): Promise<NodeMessage>
  sendAttachment(input: SendAttachmentInput): Promise<NodeMessage>
  downloadAttachment(input: DownloadAttachmentInput): Promise<NodeDownload>
  /** Permanently removes this state root's SDK-owned local data and keeps the client open. */
  clearLocalData(): Promise<{ readonly cleared: boolean }>
  /** Rejects new work, cancels cancel-safe I/O, drains in-flight work, and releases the state lock. */
  close(): Promise<void>
}
