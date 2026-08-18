import type {
  AddGroupMemberInput,
  CreateGroupInput,
  DisplayProfileBatchInput,
  DownloadAttachmentInput,
  ExternalHttpHeader,
  ExternalHttpRequest,
  ExternalHttpResponse,
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
  NodeIdentity,
  NodeMessage,
  NodePeer,
  OtpChallenge,
  Page,
  PageInput,
  RegistrationInput,
  RegistrationWithOtp,
  SendAttachmentInput,
  SendMailInput,
  SendMailResult,
  SendTextInput,
  SyncOptions,
  SyncResult,
  RealtimeEvent,
  RealtimeOptions,
  RealtimeStatus,
} from './types.js'

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
  updateDisplayName(displayName: string): Promise<NodeIdentity>
  resolvePeer(peer: string): Promise<NodePeer>
  hydrateDisplayProfiles(input: DisplayProfileBatchInput): Promise<NodeDisplayProfile[]>
  createGroup(input: CreateGroupInput): Promise<NodeGroup>
  addGroupMember(input: AddGroupMemberInput): Promise<NodeGroupMember>
  syncNow(input?: SyncOptions): Promise<SyncResult>
  startRealtime(input?: RealtimeOptions): Promise<NativeRealtimeSession>
  listConversations(input?: PageInput): Promise<Page<NodeConversation>>
  getHistory(input: HistoryInput): Promise<Page<NodeMessage>>
  getLocalConversationTimeline(input: HistoryInput): Promise<Page<NodeMessage>>
  markConversationRead(conversationId: string): Promise<MarkReadResult>
  sendText(input: SendTextInput): Promise<NodeMessage>
  sendAttachment(input: Omit<SendAttachmentInput, 'bytes'> & { readonly bytes: Buffer }): Promise<NodeMessage>
  downloadAttachment(input: DownloadAttachmentInput): Promise<Omit<NodeDownload, 'bytes'> & { readonly bytes: Buffer }>
  getMailAccount(): Promise<MailAccount>
  listMailInbox(input?: MailInboxInput): Promise<MailInboxPage>
  readMail(messageId: string): Promise<MailMessage>
  markMailRead(input: MarkMailReadInput): Promise<MarkMailReadResult>
  sendMail(input: SendMailInput): Promise<SendMailResult>
  clearLocalData(): Promise<{ readonly cleared: boolean }>
  close(): Promise<void>
}

export interface NativeBinding {
  readonly nativeApiVersion: () => number
  readonly openNativeClient: (options: ImCoreNodeOpenOptions) => Promise<NativeImCoreNodeClient>
}
