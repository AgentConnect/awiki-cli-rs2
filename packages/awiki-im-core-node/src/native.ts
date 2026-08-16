import type {
  DownloadAttachmentInput,
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
  SyncOptions,
  SyncResult,
} from './types.js'

export interface NativeImCoreNodeClient {
  getDefaultIdentity(): Promise<NodeIdentity | null>
  requestRegistrationOtp(input: RegistrationInput): Promise<OtpChallenge>
  completeRegistration(input: RegistrationWithOtp): Promise<NodeIdentity>
  updateDisplayName(displayName: string): Promise<NodeIdentity>
  resolvePeer(peer: string): Promise<NodePeer>
  syncNow(input?: SyncOptions): Promise<SyncResult>
  listConversations(input?: PageInput): Promise<Page<NodeConversation>>
  getHistory(input: HistoryInput): Promise<Page<NodeMessage>>
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
