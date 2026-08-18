import type {
  DownloadAttachmentInput,
  ExternalHttpHeader,
  ExternalHttpRequest,
  ExternalHttpResponse,
  HistoryInput,
  ImCoreNodeOpenOptions,
  MarkReadResult,
  NodeConversation,
  NodeDownload,
  NodeIdentity,
  NodeMessage,
  NodePeer,
  OtpChallenge,
  Page,
  PageInput,
  RegistrationInput,
  RegistrationWithOtp,
  SendAttachmentInput,
  SendTextInput,
  SkillAgentProvisionInput,
  SyncOptions,
  SyncResult,
} from './types.js'

export interface NativeImCoreNodeIdentityClient {
  getIdentity(): Promise<NodeIdentity>
  updateDisplayName(displayName: string): Promise<NodeIdentity>
  resolvePeer(peer: string): Promise<NodePeer>
  syncNow(input?: SyncOptions): Promise<SyncResult>
  listConversations(input?: PageInput): Promise<Page<NodeConversation>>
  getHistory(input: HistoryInput): Promise<Page<NodeMessage>>
  getLocalConversationTimeline(input: HistoryInput): Promise<Page<NodeMessage>>
  markConversationRead(conversationId: string): Promise<MarkReadResult>
  sendText(input: SendTextInput): Promise<NodeMessage>
  sendAttachment(input: Omit<SendAttachmentInput, 'bytes'> & { readonly bytes: Buffer }): Promise<NodeMessage>
  downloadAttachment(input: DownloadAttachmentInput): Promise<Omit<NodeDownload, 'bytes'> & { readonly bytes: Buffer }>
}

export interface NativeExternalHttpAuthAttempt {
  getTargetUrl(): string
  getMethod(): string
  getHeaderPatch(): ExternalHttpHeader[]
  getRetryCount(): number
  handleResponse(response: ExternalHttpResponse): Promise<NativeExternalHttpAuthAttempt | null>
}

export interface NativeImCoreNodeClient {
  prepareExternalHttpRequest(
    input: Omit<ExternalHttpRequest, 'headers' | 'body'> & {
      readonly headers: ExternalHttpHeader[]
      readonly body?: Buffer
    },
  ): Promise<NativeExternalHttpAuthAttempt>
  getDefaultIdentity(): Promise<NodeIdentity | null>
  listIdentities(): Promise<NodeIdentity[]>
  identityClient(identityId: string): Promise<NativeImCoreNodeIdentityClient>
  provisionSkillAgentIdentity(input: SkillAgentProvisionInput): Promise<NodeIdentity>
  acknowledgeSkillAgentProvision(operationId: string): Promise<void>
  requestRegistrationOtp(input: RegistrationInput): Promise<OtpChallenge>
  completeRegistration(input: RegistrationWithOtp): Promise<NodeIdentity>
  updateDisplayName(displayName: string): Promise<NodeIdentity>
  resolvePeer(peer: string): Promise<NodePeer>
  syncNow(input?: SyncOptions): Promise<SyncResult>
  listConversations(input?: PageInput): Promise<Page<NodeConversation>>
  getHistory(input: HistoryInput): Promise<Page<NodeMessage>>
  getLocalConversationTimeline(input: HistoryInput): Promise<Page<NodeMessage>>
  markConversationRead(conversationId: string): Promise<MarkReadResult>
  sendText(input: SendTextInput): Promise<NodeMessage>
  sendAttachment(input: Omit<SendAttachmentInput, 'bytes'> & { readonly bytes: Buffer }): Promise<NodeMessage>
  downloadAttachment(input: DownloadAttachmentInput): Promise<Omit<NodeDownload, 'bytes'> & { readonly bytes: Buffer }>
  clearLocalData(): Promise<{ readonly cleared: boolean }>
  close(): Promise<void>
}

export interface NativeBinding {
  readonly nativeApiVersion: () => number
  readonly openNativeClient: (options: ImCoreNodeOpenOptions) => Promise<NativeImCoreNodeClient>
}
