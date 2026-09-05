/** State and service configuration owned by the Node host. */
export interface ImCoreNodeOpenOptions {
  /** Absolute, process-exclusive root for identities, SQLite, cache, and metadata. */
  readonly stateRoot: string
  readonly serviceBaseUrl: string
  readonly didDomain: string
  readonly userServiceEndpoint?: string
  readonly messageServiceEndpoint?: string
  readonly mailServiceEndpoint?: string
  readonly anpServiceEndpoint?: string
  readonly anpServiceDid?: string
  /** Candidate build identity sent by Core on local-product User/Message requests. */
  readonly clientVersionInfo?: {
    readonly product: 'awiki-me' | 'awiki-cli' | 'awiki-daemon'
    readonly release: string
    readonly version: string
    readonly build?: number
  }
  /** Default timeout for one operation, from 1 to 600000 milliseconds. */
  readonly operationTimeoutMs?: number
  /** Timeout for bounded synchronization before list reads. */
  readonly syncTimeoutMs?: number
  /** Test-only exception for loopback HTTP external-auth targets. */
  readonly externalHttpAllowInsecureLoopbackForTesting?: boolean
  /** Enables permanent revocation of another device. Defaults to false. */
  readonly multiDeviceDeviceRevokeEnabled?: boolean
  /** Enables Core-owned durable phone recovery for an existing Handle. */
  readonly multiDeviceHandleRecoveryEnabled?: boolean
  /** Exact User Service audience required when Handle recovery is enabled. */
  readonly multiDeviceAudience?: string
  /** Trusted Host-only ANP Identity Provider lease used by DSH External mode. */
  readonly identityProvider?: ImCoreIdentityProvider
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
  readonly targetUrl: string
  readonly method: string
  readonly headerPatch: readonly ExternalHttpHeader[]
  readonly retryCount: number
  handleResponse(response: ExternalHttpResponse): Promise<ExternalHttpAuthAttempt | null>
}

export interface ImCoreIdentityReference {
  readonly storeId: string
  readonly identityId: string
  readonly did: string
}

export interface ImCoreSealedSecretDelivery {
  readonly envelope: ImCoreSealedSecretEnvelope
  readonly authorization: {
    readonly providerInstanceId: string
    readonly parentLeaseId: string
    readonly consumer: string
    readonly capability: string
    readonly storeId: string
    readonly expiresAt: number
  }
  readonly aad: string
}

export interface ImCoreSealedSecretEnvelope {
  readonly protocol: 'anp-sealed-secret/1'
  readonly suite: 'hpke-base-x25519-hkdf-sha256-chacha20poly1305-v1'
  readonly encappedKey: string
  readonly ciphertext: string
}

/** JSON value accepted by Host identity-provider document operations. */
export type ImCoreJsonValue =
  | null
  | boolean
  | number
  | string
  | ImCoreJsonValue[]
  | { [key: string]: ImCoreJsonValue }

export interface ImCoreIdentityProvider {
  readonly protocol: 'anp-identity-provider-ts/1'
  readonly capabilities: readonly string[]
  info(): Promise<unknown>
  recover(): Promise<unknown>
  list(): Promise<readonly unknown[]>
  publicIdentity(reference: ImCoreIdentityReference): Promise<unknown>
  hostStatus(reference: ImCoreIdentityReference): Promise<{
    readonly rootCapability: 'absent' | 'pending' | 'active'
    readonly rootKeyFingerprint: string
    readonly checkpoint?: {
      readonly documentVersion: number
      readonly registryVersion: number
      readonly documentDigest: string
    }
  }>
  create(request: unknown): Promise<unknown>
  delete(reference: ImCoreIdentityReference): Promise<void>
  recoverIdentity(reference: ImCoreIdentityReference): Promise<void>
  ecdhSealed(request: {
    readonly identity: ImCoreIdentityReference
    readonly kid: string
    readonly peerPublic: Buffer
    readonly recipientPublicKey: Buffer
    readonly requestId: string
  }): Promise<ImCoreSealedSecretDelivery>
  exportRootKeySealed?(request: {
    readonly identity: ImCoreIdentityReference
    readonly kid: string
    readonly recipientPublicKey: Buffer
    readonly requestId: string
    readonly userPresenceConfirmed: boolean
  }): Promise<ImCoreSealedSecretDelivery>
  prepareLegacyRootImport?(request: {
    readonly identity: ImCoreIdentityReference
    readonly evidence: unknown
    readonly encoding: 'raw32' | 'pkcs8_der'
    readonly requestId: string
  }): Promise<ImCorePreparedRootImport>
  prepareIdentityMaterialImport?(request: {
    readonly remote: unknown
    readonly didWba: boolean
    readonly keys: readonly {
      readonly kid: string
      readonly purpose:
        | 'root_control'
        | 'authentication'
        | 'device_assertion'
        | 'application_assertion'
        | 'key_agreement'
      readonly encoding: 'raw32' | 'pkcs8_der'
    }[]
    readonly requestId: string
  }): Promise<ImCorePreparedIdentityMaterialImport>
  importWrappedRoot?(
    reference: ImCoreIdentityReference,
    envelope: unknown,
  ): Promise<'pending' | 'active'>
  sign(
    reference: ImCoreIdentityReference,
    request:
      | { readonly purpose: 'authentication'; readonly kid?: string; readonly payload: Buffer }
      | { readonly purpose: 'device_assertion'; readonly kid?: string; readonly payload: Buffer }
      | {
          readonly purpose: 'application_assertion'
          readonly domain: string
          readonly kid?: string
          readonly payload: Buffer
        },
  ): Promise<{ readonly kid: string; readonly algorithm: 'ed25519'; readonly bytes: Buffer }>
  signOriginProof(
    reference: ImCoreIdentityReference,
    request: {
      readonly method: string
      readonly meta: unknown
      readonly body: unknown
      readonly kid?: string
      readonly options?: {
        readonly created?: number
        readonly expires?: number
        readonly nonce?: string
      }
    },
  ): Promise<unknown>
  signDocumentProof(
    reference: ImCoreIdentityReference,
    request: {
      readonly kid?: string
      readonly document: ImCoreJsonValue
      readonly options?: {
        readonly proofPurpose?: string
        readonly proofType?: string
        readonly cryptosuite?: string
        readonly created?: string
        readonly domain?: string
        readonly challenge?: string
      }
    },
  ): Promise<unknown>
  prepareHttpSignature(request: {
    readonly identity: ImCoreIdentityReference
    readonly kid?: string
    readonly url: string
    readonly method: string
    readonly headers: readonly { readonly name: string; readonly value: string }[]
    readonly body?: Buffer
    readonly nonce?: string
    readonly created?: number
    readonly expires?: number
    readonly coveredComponents?: readonly string[]
  }): Promise<unknown>
  prepareDocumentChange(
    reference: ImCoreIdentityReference,
    request: unknown,
  ): Promise<ImCoreProviderDocumentChangeSession>
  resumeDocumentChange(
    reference: ImCoreIdentityReference,
  ): Promise<ImCoreProviderDocumentChangeSession | undefined>
  prepareIdentityTransition(request: {
    readonly expectedCurrentDid: string
    readonly operationId: string
    readonly successor: ImCoreIdentityReference
    readonly transitionDocument?: unknown
  }): Promise<ImCoreProviderIdentityTransitionSession>
  resumeIdentityTransition(
    expectedCurrentDid: string,
  ): Promise<ImCoreProviderIdentityTransitionSession | undefined>
  adoptVerifiedDocument(
    reference: ImCoreIdentityReference,
    remote: unknown,
  ): Promise<string>
  beginDeviceEnrollment(request: unknown): Promise<ImCoreProviderEnrollmentSession>
  beginRequestSigningEnrollment(request: unknown): Promise<ImCoreProviderEnrollmentSession>
  resumeEnrollment(
    reference: ImCoreIdentityReference,
  ): Promise<ImCoreProviderEnrollmentSession | undefined>
  confirmRootPromotion?(
    reference: ImCoreIdentityReference,
    request: { readonly remote: unknown },
  ): Promise<void>
  signPendingRootObjectProof?(
    reference: ImCoreIdentityReference,
    request: {
      readonly kid?: string
      readonly document: unknown
      readonly issuerDid: string
      readonly created?: string
    },
  ): Promise<unknown>
}

/** Host-only sealed import workflow retained inside the provider bridge. */
export interface ImCorePreparedRootImport {
  offer(): {
    readonly recipientPublicKey: Buffer
    readonly requestId: string
    readonly token: string
    readonly authorization: ImCoreSealedSecretDelivery['authorization']
    readonly aad: string
  }
  complete(
    token: string,
    envelope: ImCoreSealedSecretEnvelope,
  ): Promise<'pending' | 'active'>
}

/** Host-only sealed legacy migration workflow retained inside the provider bridge. */
export interface ImCorePreparedIdentityMaterialImport {
  offer(): {
    readonly target: ImCoreIdentityReference
    readonly recipientPublicKey: Buffer
    readonly requestId: string
    readonly token: string
    readonly authorization: ImCoreSealedSecretDelivery['authorization']
    readonly itemAad: readonly string[]
  }
  complete(
    token: string,
    envelopes: readonly ImCoreSealedSecretEnvelope[],
  ): Promise<unknown>
}

/** Host-only opaque workflow retained inside the provider bridge. */
export interface ImCoreProviderDocumentChangeSession {
  candidate(): Promise<unknown>
  hostPhase(): Promise<'prepared' | 'publication_in_flight' | 'publication_uncertain' | 'published'>
  beginPublication(): Promise<unknown>
  complete(attempt: unknown, result: unknown): Promise<unknown>
  reconcile(observation: unknown): Promise<unknown>
}

/** Host-only DID transition workflow retained inside the provider bridge. */
export interface ImCoreProviderIdentityTransitionSession {
  candidate(): Promise<unknown>
  beginPublication(): Promise<unknown>
  complete(attempt: unknown, result: unknown): Promise<unknown>
  reconcile(observation: unknown): Promise<unknown>
}

/** Host-only enrollment workflow retained inside the provider bridge. */
export interface ImCoreProviderEnrollmentSession {
  proposal(): Promise<unknown>
  signDeviceAssertion(payload: Buffer): Promise<Buffer>
  deriveDeviceSharedSecretSealed(request: {
    readonly peerPublic: Buffer
    readonly recipientPublicKey: Buffer
    readonly requestId: string
  }): Promise<ImCoreSealedSecretDelivery>
  activate(remote: unknown): Promise<'activated'>
  cancel(): Promise<void>
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

/** Public editable profile projection. Proofs, metadata, and private state are excluded. */
export interface NodeProfile {
  readonly did: string
  readonly handle?: string
  readonly displayName?: string
  readonly bio?: string
  readonly tags: readonly string[]
  readonly updatedAt?: string
}

export interface UpdateProfileInput {
  readonly displayName: string
  readonly bio?: string
  readonly tags?: readonly string[]
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

export interface ExistingHandleRegistration {
  /** Opaque process-local Core preparation identifier. */
  readonly continuationId: string
  readonly fullHandle: string
  readonly expectedDid: string
  readonly mode: 'ordinary' | 'handle_recovery_rebind'
  readonly requiresUserPresence: boolean
}

export type RegistrationOutcome =
  | {
      readonly status: 'registered'
      readonly identity: NodeIdentity
      readonly warnings: readonly string[]
    }
  | {
      readonly status: 'existing_handle'
      readonly existingHandle: ExistingHandleRegistration
      readonly warnings: readonly string[]
    }

/** Host-only activation of a process-local existing-Handle preparation. */
export interface PreparedRegistrationJoinInput {
  readonly continuationId: string
  readonly operationId: string
  readonly ttlSeconds?: number
  readonly userPresenceConfirmed: boolean
}

/** Host-only continuation of an already-started prepared registration Join. */
export interface PreparedRegistrationJoinResumeInput {
  readonly joinSessionId: string
}

export type PreparedRegistrationJoinLocalPhase =
  | 'pending'
  | 'challenge_prepared'
  | 'response_prepared'
  | 'response_verified'
  | 'approval_prepared'
  | 'authorized'
  | 'cancelled'
  | 'expired'

export type PreparedRegistrationJoinRemoteState =
  | 'pending'
  | 'challenge_sent'
  | 'response_verified'
  | 'consumed'
  | 'cancelled'
  | 'rejected'
  | 'expired'

/** Secret-free progress for a prepared existing-Handle activation. */
export interface PreparedRegistrationJoinProgress {
  readonly joinSessionId: string
  readonly did: string
  readonly localPhase: PreparedRegistrationJoinLocalPhase
  readonly remoteState: PreparedRegistrationJoinRemoteState
  readonly expiresAt: string
  readonly sas?: string
  readonly completed: boolean
  readonly identity?: NodeIdentity
}

export interface LocalDeviceJoinSession {
  readonly joinSessionId: string
  readonly side: 'new_device' | 'admin'
  readonly localPhase: PreparedRegistrationJoinLocalPhase
  readonly expiresAt: string
}

export interface CurrentDeviceSummary {
  readonly identityId: string
  readonly did: string
  readonly mode: 'legacy' | 'v_next'
  readonly protocolDeviceId?: string
  readonly role?: 'member' | 'admin'
  readonly readiness: 'legacy' | 'member_ready' | 'admin_awaiting_root' | 'admin_ready' | 'blocked'
  readonly canManage: boolean
}

export interface RegistryDeviceSummary {
  readonly protocolDeviceId: string
  readonly signingKeyId: string
  readonly e2eeKeyId: string
  readonly status: 'active' | 'revoked'
  readonly role: 'member' | 'admin'
  readonly managementReady: boolean
  readonly isCurrent: boolean
  readonly authGeneration: string
}

export interface DeviceRegistrySnapshot {
  readonly did: string
  readonly registryVersion: string
  readonly devices: readonly RegistryDeviceSummary[]
}

export interface DeviceJoinRequestNotice {
  readonly eventId: string
  readonly joinSessionId: string
  readonly did: string
  readonly protocolDeviceId: string
  readonly candidateKeyFingerprint: string
  readonly issuedAt: string
  readonly expiresAt: string
  readonly state: PreparedRegistrationJoinRemoteState
  readonly claimedByCurrentDevice: boolean
  readonly canStartVerification: boolean
}

export interface StartDeviceJoinVerificationInput {
  readonly joinSessionId: string
  readonly operationId: string
  readonly challengeTtlSeconds: number
}

export interface AdminDeviceJoinProgress {
  readonly joinSessionId: string
  readonly did: string
  readonly protocolDeviceId: string
  readonly localPhase: PreparedRegistrationJoinLocalPhase
  readonly remoteState: PreparedRegistrationJoinRemoteState
  readonly expiresAt: string
  readonly sas?: string
}

export interface DeviceJoinApprovalPrompt {
  readonly approvalHandle: string
  readonly joinSessionId: string
  readonly sas: string
  readonly expiresAt: string
}

export interface DeviceRevokeResult {
  readonly did: string
  readonly targetDeviceId: string
  readonly status: 'revoked'
}

/** Host-only, short-lived Core authorization for one exact-device Root Transfer. */
export interface RootKeyTransferPreparation {
  readonly authorizationHandle: string
  readonly recipient: RootKeyTransferRecipientSummary
  readonly expiresAt: string
}

export interface RootKeyTransferRecipientSummary {
  readonly did: string
  readonly deviceId: string
  readonly signingKeyId: string
  readonly e2eeKeyId: string
  readonly registryVersion: string
}

export interface RootKeyTransferSendResult {
  readonly did: string
  readonly senderDeviceId: string
  readonly recipientDeviceId: string
  readonly messageId: string
  readonly acceptedAt: string
}

export interface UserPresenceInput {
  readonly reason: string
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

/** Minimal private transport-protected group creation input. */
export interface CreateGroupInput {
  readonly name: string
  readonly description?: string
}

/** Created group with its canonical conversation route. */
export interface NodeGroup {
  readonly did: string
  readonly conversationId: string
  readonly title: string
  readonly description?: string
  readonly memberCount?: number
  readonly myRole?: string
  readonly membershipStatus?: string
}

/** One member reference accepted as a DID, full Handle, or local Handle name. */
export interface AddGroupMemberInput {
  readonly groupDid: string
  readonly member: string
  readonly role?: 'admin' | 'member'
}

/** Authoritative member identity returned after group membership mutation. */
export interface NodeGroupMember {
  readonly did: string
  readonly handle?: string
}

export interface GroupInput {
  readonly groupDid: string
}

export interface GroupMembersInput extends GroupInput, PageInput {}

export interface RemoveGroupMemberInput extends GroupInput {
  readonly member: string
}

/** One authoritative group membership record. A stable DID can be absent on malformed legacy rows. */
export interface NodeGroupMemberRecord {
  readonly membershipId?: string
  readonly peerPersonaId?: string
  readonly did?: string
  readonly credentialDid?: string
  readonly handle?: string
  readonly role?: string
  readonly status?: string
  readonly joinedAt?: string
  readonly subjectType?: string
}

/** Authoritative member page. Cursor and group version values remain opaque to callers. */
export interface GroupMemberPage {
  readonly items: readonly NodeGroupMemberRecord[]
  readonly total?: number
  readonly nextCursor?: string
  readonly hasMore: boolean
  readonly pageGroup?: string
  readonly groupStateVersion?: string
  readonly warnings: readonly string[]
}

/** Local-only batch lookup used to hydrate message sender labels without network I/O. */
export interface DisplayProfileBatchInput {
  readonly peers: readonly string[]
}

/** One locally cached display profile. */
export interface NodeDisplayProfile {
  readonly did?: string
  readonly handle?: string
  readonly displayName?: string
  readonly cacheHit: boolean
  readonly isStale: boolean
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

/** Core-supported canonical reliable-sync reasons retained from 0.1.2. */
export type SyncReason =
  | 'session_start'
  | 'app_resume'
  | 'websocket_hint'
  | 'websocket_reconnect'
  | 'foreground_reconcile'
  | 'manual_refresh'
  | 'after_mutation'

/** Explicit reliable-sync input. Omitted `reason` defaults to `manual_refresh`. */
export interface SyncOptions {
  readonly reason?: SyncReason
  readonly limit?: number
  readonly timeoutMs?: number
}

export type SyncStatus =
  | 'idle'
  | 'changed'
  | 'recovery_required'
  | 'retryable_failure'
  | 'auth_revoked'
  | 'blocked'

/** Product-safe reliable-sync result. */
export interface SyncResult {
  readonly status: SyncStatus
  readonly eventsApplied: number
  readonly pagesFetched: number
  readonly messagesHydrated: number
  readonly duplicatesSkipped: number
  readonly olderHistoryExcluded: boolean
  readonly changedConversationIds: readonly string[]
  /** Closed Core failure code. It never contains an error message or identity data. */
  readonly errorCode?: string
  readonly warnings: readonly string[]
}

export type RealtimeConnectionState =
  | 'disconnected'
  | 'connecting'
  | 'connected'
  | 'reconnecting'
  | 'closed'

/** Bounded reconnect policy. The native facade defaults to exponential reconnect without an attempt cap. */
export interface RealtimeOptions {
  readonly eventBuffer?: number
  readonly reconnectBaseDelayMs?: number
  readonly reconnectMaxDelayMs?: number
  readonly reconnectMaxAttempts?: number
}

export interface RealtimeStatus {
  readonly connected: boolean
  readonly state: RealtimeConnectionState
}

export interface RealtimeConnectionStateChangedEvent {
  readonly kind: 'connection_state_changed'
  readonly state: RealtimeConnectionState
}

export type RealtimeSyncCause =
  | 'connection_ready'
  | 'reconnected'
  | 'message'
  | 'message_update'
  | 'group'
  | 'system_notification'
  | 'stream_recovery'

/**
 * A scheduling signal to run `syncNow()`. It is never a reliable cursor or checkpoint.
 * Message bodies, wire event sequence values, raw frames, URLs, and credentials are excluded.
 */
export interface RealtimeSyncRequiredEvent {
  readonly kind: 'sync_required'
  readonly cause: RealtimeSyncCause
  readonly dirty: boolean
  readonly gapDetected: boolean
}

export type RealtimeEvent = RealtimeConnectionStateChangedEvent | RealtimeSyncRequiredEvent

export interface RealtimeSession {
  /**
   * Returns null after the native stream has closed, including event-buffer exhaustion.
   * Treat null as stream recovery: stop this session, run canonical `syncNow()` with
   * `websocket_reconnect`, then start a replacement session. Only one consumer should call this.
   */
  nextEvent(): Promise<RealtimeEvent | null>
  getStatus(): Promise<RealtimeStatus>
  /** Idempotently stops and joins the native realtime worker. */
  stop(): Promise<void>
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

/** Generic JSON Payload send. ANP-P9 mention payloads are validated again in Rust. */
export interface SendPayloadInput {
  readonly conversationId: string
  readonly payloadJson: string
  readonly clientMessageId?: string
  readonly idempotencyKey?: string
}

export interface HandleRecoveryOtpInput {
  readonly fullHandle: string
  readonly phone: string
}

export interface HandleRecoveryOtpResult {
  readonly ownerIdentityId: string
  readonly fullHandle: string
  readonly operationId: string
  readonly accepted: boolean
  readonly retryAfterSeconds: number
  readonly retryAt: string
}

export interface HandleRecoveryPrepareInput {
  readonly operationId: string
  readonly phone: string
  readonly otp: string
}

export interface HandleRecoveryOperationInput {
  readonly operationId: string
}

export type HandleRecoveryPhase =
  | 'awaiting_factor'
  | 'ready_to_commit'
  | 'remote_outcome_unknown'
  | 'remote_committed'
  | 'identity_transition_pending'
  | 'applied'
  | 'quarantined_key_unavailable'

export interface HandleRecoveryImpact {
  readonly localOrdinaryDataWillMigrate: boolean
  readonly otherDevicesMustRejoin: boolean
}

/** Durable recovery status. Hosts resume uncertain states instead of repeating activation. */
export interface HandleRecoveryProgress {
  readonly operationId: string
  readonly ownerIdentityId: string
  readonly fullHandle: string
  readonly previousDid?: string
  readonly currentDid: string
  readonly phase: HandleRecoveryPhase
  readonly failureCode?: string
  readonly retryable: boolean
  readonly impact: HandleRecoveryImpact
}

export interface HandleRecoveryOperationSummary {
  readonly operationId: string
  readonly ownerIdentityId: string
  readonly fullHandle: string
  readonly lifecycle: string
  readonly commitAttempted: boolean
  readonly keyState: string
  readonly lastErrorCode?: string
  readonly createdAt: string
  readonly updatedAt: string
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

export interface MailInboxInput {
  readonly folder?: string
  readonly limit?: number
  readonly offset?: number
  readonly unreadOnly?: boolean
}

export interface MarkMailReadInput {
  readonly messageIds: readonly string[]
}

export interface SendMailInput {
  readonly to: readonly string[]
  readonly cc?: readonly string[]
  readonly subject: string
  readonly bodyText: string
}

export interface MailAccount {
  readonly mailboxAddress?: string
  readonly displayName?: string
  readonly status?: string
}

export interface MailMessageSummary {
  readonly id: string
  readonly folder?: string
  readonly from: readonly string[]
  readonly to: readonly string[]
  readonly cc: readonly string[]
  readonly subject: string
  readonly subjectTruncated: boolean
  readonly preview?: string
  readonly previewTruncated: boolean
  readonly receivedAt?: string
  readonly sentAt?: string
  readonly unread: boolean
  readonly hasAttachments: boolean
  readonly attachmentCount?: number
}

export interface MailAttachmentMetadata {
  readonly index: number
  readonly fileName?: string
  readonly contentType?: string
  /** Decimal byte count; kept as a string to avoid JS integer truncation. */
  readonly sizeBytes?: string
}

export interface MailMessage {
  readonly summary: MailMessageSummary
  readonly bodyText?: string
  readonly bodyTruncated: boolean
  readonly hasHtmlBody: boolean
  readonly attachments: readonly MailAttachmentMetadata[]
}

export interface MailInboxPage {
  readonly items: readonly MailMessageSummary[]
  readonly nextOffset?: number
  readonly hasMore: boolean
}

export interface MarkMailReadResult {
  readonly updated: number
}

export interface SendMailResult {
  readonly accepted: boolean
  readonly messageId?: string
  readonly warnings: readonly string[]
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
  /** Complete registration without collapsing an existing Handle into an error. */
  completeRegistrationWithOutcome(input: RegistrationWithOtp): Promise<RegistrationOutcome>
  /** Consume the verified registration continuation after explicit Host confirmation. */
  beginPreparedRegistrationJoin(input: PreparedRegistrationJoinInput): Promise<PreparedRegistrationJoinProgress>
  /** Advance an already-started prepared registration Join without repeating its mutation. */
  resumePreparedRegistrationJoin(input: PreparedRegistrationJoinResumeInput): Promise<PreparedRegistrationJoinProgress>
  listLocalDeviceJoinSessions(): Promise<readonly LocalDeviceJoinSession[]>
  cancelPreparedRegistrationJoin(input: PreparedRegistrationJoinResumeInput): Promise<LocalDeviceJoinSession>
  getCurrentDeviceSummary(): Promise<CurrentDeviceSummary>
  getDeviceRegistry(): Promise<DeviceRegistrySnapshot>
  listLocalDeviceJoinRequests(): Promise<readonly DeviceJoinRequestNotice[]>
  startDeviceJoinVerification(input: StartDeviceJoinVerificationInput): Promise<AdminDeviceJoinProgress>
  getLocalDeviceJoinVerificationProgress(input: PreparedRegistrationJoinResumeInput): Promise<AdminDeviceJoinProgress>
  prepareDeviceJoinApproval(input: { readonly joinSessionId: string; readonly sasConfirmed: boolean }): Promise<DeviceJoinApprovalPrompt>
  confirmDeviceJoinApproval(input: { readonly approvalHandle: string; readonly userPresenceConfirmed: boolean }): Promise<AdminDeviceJoinProgress>
  rejectDeviceJoin(input: { readonly joinSessionId: string; readonly reason: 'user_rejected' | 'sas_mismatch' }): Promise<AdminDeviceJoinProgress>
  revokeDevice(input: { readonly targetDeviceId: string; readonly userPresenceConfirmed: boolean }): Promise<DeviceRevokeResult>
  prepareRootKeyTransfer(input: { readonly recipientDeviceId: string }): Promise<RootKeyTransferPreparation>
  confirmAndSendRootKeyTransfer(input: { readonly authorizationHandle: string; readonly userPresenceConfirmed: boolean }): Promise<RootKeyTransferSendResult>
  confirmUserPresence(input: UserPresenceInput): Promise<boolean>
  updateDisplayName(displayName: string): Promise<NodeIdentity>
  getProfile(): Promise<NodeProfile>
  updateProfile(input: UpdateProfileInput): Promise<NodeProfile>
  resolvePeer(peer: string): Promise<NodePeer>
  hydrateDisplayProfiles(input: DisplayProfileBatchInput): Promise<readonly NodeDisplayProfile[]>
  createGroup(input: CreateGroupInput): Promise<NodeGroup>
  addGroupMember(input: AddGroupMemberInput): Promise<NodeGroupMember>
  getGroup(input: GroupInput): Promise<NodeGroup>
  listGroups(input?: PageInput): Promise<Page<NodeGroup>>
  joinGroup(input: GroupInput): Promise<NodeGroup>
  leaveGroup(input: GroupInput): Promise<void>
  listGroupMembers(input: GroupMembersInput): Promise<GroupMemberPage>
  removeGroupMember(input: RemoveGroupMemberInput): Promise<NodeGroupMember>
  syncNow(input?: SyncOptions): Promise<SyncResult>
  /** Starts the single Core-owned realtime session for this client. */
  startRealtime(input?: RealtimeOptions): Promise<RealtimeSession>
  listConversations(input?: PageInput): Promise<Page<NodeConversation>>
  getHistory(input: HistoryInput): Promise<Page<NodeMessage>>
  /** Read only the committed local timeline; never starts sync, history, or Directory RPC. */
  getLocalConversationTimeline(input: HistoryInput): Promise<Page<NodeMessage>>
  markConversationRead(conversationId: string): Promise<MarkReadResult>
  sendText(input: SendTextInput): Promise<NodeMessage>
  sendPayload(input: SendPayloadInput): Promise<NodeMessage>
  sendAttachment(input: SendAttachmentInput): Promise<NodeMessage>
  downloadAttachment(input: DownloadAttachmentInput): Promise<NodeDownload>
  getMailAccount(): Promise<MailAccount>
  listMailInbox(input?: MailInboxInput): Promise<MailInboxPage>
  readMail(messageId: string): Promise<MailMessage>
  markMailRead(input: MarkMailReadInput): Promise<MarkMailReadResult>
  sendMail(input: SendMailInput): Promise<SendMailResult>
  requestHandleRecoveryOtp(input: HandleRecoveryOtpInput): Promise<HandleRecoveryOtpResult>
  prepareHandleRecovery(input: HandleRecoveryPrepareInput): Promise<HandleRecoveryProgress>
  activateHandleRecovery(input: HandleRecoveryOperationInput): Promise<HandleRecoveryProgress>
  getHandleRecoveryStatus(input: HandleRecoveryOperationInput): Promise<HandleRecoveryProgress>
  resumeHandleRecovery(input: HandleRecoveryOperationInput): Promise<HandleRecoveryProgress>
  discardHandleRecovery(input: HandleRecoveryOperationInput): Promise<HandleRecoveryOperationSummary>
  /** Retires only this device's default identity credential so it can rejoin without deleting ordinary local data. */
  retireDefaultIdentityForRejoin(): Promise<void>
  /** Permanently removes this state root's SDK-owned local data and keeps the client open. */
  clearLocalData(): Promise<{ readonly cleared: boolean }>
  /** Rejects new work, cancels cancel-safe I/O, drains in-flight work, and releases the state lock. */
  close(): Promise<void>
}
