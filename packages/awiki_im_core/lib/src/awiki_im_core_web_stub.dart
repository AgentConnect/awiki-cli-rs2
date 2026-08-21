import 'models/attachment.dart';
import 'models/config.dart';
import 'models/identity.dart';
import 'models/local_state_upgrade.dart';
import 'models/message.dart';
import 'models/secure.dart';

UnsupportedError _unsupported() => UnsupportedError(
  'awiki_im_core native Rust backend is not supported on Flutter Web.',
);

class AwikiImCore {
  static Future<LocalStateUpgradeInspection> inspectLocalStateUpgrade({
    required AwikiImCorePaths paths,
  }) async {
    throw _unsupported();
  }

  static Future<LocalStateUpgradeResult> upgradeLocalState({
    required AwikiImCorePaths paths,
  }) async {
    throw _unsupported();
  }

  static Future<LocalStateRestoreResult> restoreLocalStateBackup({
    required AwikiImCorePaths paths,
  }) async {
    throw _unsupported();
  }

  static Future<AwikiImCore> open({
    required AwikiImCoreConfig config,
    required AwikiImCorePaths paths,
    AwikiImCoreOpenOptions? openOptions,
  }) async {
    throw _unsupported();
  }

  Future<List<IdentitySummary>> listIdentities() async {
    throw _unsupported();
  }

  Future<IdentitySummary?> defaultIdentity() async {
    throw _unsupported();
  }

  Future<IdentitySummary> resolveIdentity(IdentitySelector selector) async {
    throw _unsupported();
  }

  Future<IdentitySummary> updateDisplayNameProjection({
    required String identityId,
    String? displayName,
  }) async {
    throw _unsupported();
  }

  Future<IdentityDeviceSummary> identityDeviceSummary(
    IdentitySelector selector,
  ) async {
    throw _unsupported();
  }

  Future<LegacyRegistryEpochAdoptionAuthority?>
  legacyRegistryEpochAdoptionAuthority(IdentitySelector selector) async {
    throw _unsupported();
  }

  Future<HandleRecoveryOtpResult> requestHandleRecoveryOtp({
    IdentitySelector? selector,
    required String fullHandle,
    required String phone,
  }) async {
    throw _unsupported();
  }

  Future<HandleRecoveryProgress> prepareHandleRecovery({
    required String operationId,
    required String phone,
    required String code,
  }) async {
    throw _unsupported();
  }

  Future<HandleRecoveryProgress> activateHandleRecovery({
    required String operationId,
    required bool userPresenceConfirmed,
  }) async {
    throw _unsupported();
  }

  Future<HandleRecoveryProgress> resumeHandleRecovery(
    String operationId,
  ) async {
    throw _unsupported();
  }

  Future<HandleRecoveryProgress> handleRecoveryStatus(
    String operationId,
  ) async {
    throw _unsupported();
  }

  Future<List<HandleRecoveryOperationSummary>> listHandleRecoveryOperations(
    IdentitySelector selector,
  ) async {
    throw _unsupported();
  }

  Future<HandleRecoveryOperationSummary> discardHandleRecoveryPreAttempt(
    String operationId,
  ) async {
    throw _unsupported();
  }

  Future<HandleRecoveryOperationSummary>
  quarantineHandleRecoveryKeyUnavailable({
    required String operationId,
    required bool userPresenceConfirmed,
  }) async {
    throw _unsupported();
  }

  Future<HandleRecoveryAccountEpochReceipt?> authorizedHandleRecoveryReceipt(
    IdentitySelector selector,
  ) async {
    throw _unsupported();
  }

  Future<AuthorizedJoinActivationProgress> activateAuthorizedJoin({
    required IdentitySelector selector,
    required String phone,
    required String code,
    required String handle,
    required String did,
    required String operationId,
    int? ttlSeconds,
    required bool userPresenceConfirmed,
  }) async {
    throw _unsupported();
  }

  Future<AuthorizedJoinActivationProgress> resumeAuthorizedJoinActivation(
    String joinSessionId,
  ) async {
    throw _unsupported();
  }

  Future<DeviceRevokeResult> revokeDevice({
    required IdentitySelector selector,
    required String targetDeviceId,
    required bool userPresenceConfirmed,
  }) async {
    throw _unsupported();
  }

  Future<List<DeviceJoinSessionSummary>> localDeviceJoinSessions() async {
    throw _unsupported();
  }

  Future<DeviceJoinProgress> beginDeviceJoin({
    required String did,
    required String operationId,
    int ttlSeconds = 600,
    required DeviceJoinAccountVerificationGrant accountVerificationGrant,
  }) async {
    throw _unsupported();
  }

  Future<AuthorizedJoinActivationProgress> beginPreparedRegistrationDeviceJoin({
    required String preparationId,
    required String operationId,
    int ttlSeconds = 600,
    required bool userPresenceConfirmed,
  }) async {
    throw _unsupported();
  }

  Future<DeviceJoinProgress> pollNewDeviceJoin(String joinSessionId) async {
    throw _unsupported();
  }

  Future<DeviceJoinSessionSummary> cancelNewDeviceJoin(
    String joinSessionId,
  ) async {
    throw _unsupported();
  }

  Future<DeviceJoinRegistrySnapshot> identityDeviceRegistry(
    IdentitySelector selector,
  ) async {
    throw _unsupported();
  }

  Future<List<DeviceJoinRequestNotice>> localDeviceJoinRequests(
    IdentitySelector selector,
  ) async {
    throw _unsupported();
  }

  Future<DeviceJoinProgress> localDeviceJoinVerificationProgress({
    required IdentitySelector selector,
    required String joinSessionId,
  }) async {
    throw _unsupported();
  }

  Future<DeviceJoinProgress> startDeviceJoinVerification({
    required IdentitySelector selector,
    required String joinSessionId,
    required String operationId,
    int challengeTtlSeconds = 300,
  }) async {
    throw _unsupported();
  }

  Future<DeviceJoinProgress> rejectDeviceJoin({
    required IdentitySelector selector,
    required String joinSessionId,
    required DeviceJoinRejectReason reason,
  }) async {
    throw _unsupported();
  }

  Future<DeviceJoinApprovalPrompt> prepareDeviceJoinApproval({
    required IdentitySelector selector,
    required String joinSessionId,
    required bool sasConfirmed,
  }) async {
    throw _unsupported();
  }

  Future<DeviceJoinProgress> confirmDeviceJoinApproval({
    required String approvalHandle,
    required bool userPresenceConfirmed,
  }) async {
    throw _unsupported();
  }

  @Deprecated('Use identityCustodyStatus for identity custody state.')
  Future<IdentityVaultStatus> identityVaultStatus(
    IdentitySelector selector,
  ) async {
    throw _unsupported();
  }

  Future<IdentityCustodyStatus> identityCustodyStatus(
    IdentitySelector selector,
  ) async {
    throw _unsupported();
  }

  Future<LegacyUpgradeStatus> legacyUpgradeStatus(
    IdentitySelector selector,
  ) async {
    throw _unsupported();
  }

  Future<LegacyUpgradeStatus> upgradeLegacyIdentity(
    IdentitySelector selector,
  ) async {
    throw _unsupported();
  }

  @Deprecated(
    'Use identity custody migration APIs; this name migrates to ANP Identity.',
  )
  Future<IdentityVaultMigrationReport> migrateIdentityVault(
    IdentitySelector selector,
  ) async {
    throw _unsupported();
  }

  @Deprecated('Use identityCustodyStatus for identity custody state.')
  Future<IdentityVaultVerificationReport> verifyIdentityVault(
    IdentitySelector selector,
  ) async {
    throw _unsupported();
  }

  Future<DeleteLocalIdentityResult> deleteLocalIdentity(
    IdentitySelector selector,
  ) async {
    throw _unsupported();
  }

  Future<DeleteLocalIdentityResult> deleteLocalIdentityData(
    IdentitySelector selector,
  ) async {
    throw _unsupported();
  }

  Future<DaemonSubkeyPublicPackage> authorizeDaemonSubkey({
    required IdentitySelector selector,
    required DaemonSubkeyPublicProposal proposal,
  }) async {
    throw _unsupported();
  }

  Future<AwikiImClient> client(IdentitySelector selector) async {
    throw _unsupported();
  }

  Future<void> dispose() async {}
}

class AwikiImClient {
  MessageApi get messages => MessageApi._();

  AttachmentApi get attachments => AttachmentApi._();

  SecureApi get secure => SecureApi._();

  RootKeyTransferApi get rootKeyTransfer => RootKeyTransferApi._();

  Future<ActiveSyncAccountBinding> activeSyncAccountBinding() async {
    throw _unsupported();
  }

  Future<void> dispose() async {}
}

class RootKeyTransferApi {
  RootKeyTransferApi._();

  Future<RootKeyTransferPreparation> prepare({
    required String recipientDeviceId,
  }) async {
    throw const RootKeyTransferException(
      code: 'root_transfer.unsupported',
      retryable: false,
    );
  }

  Future<RootKeyTransferSendResult> confirmAndSend({
    required RootKeyTransferAuthorizationHandle authorizationHandle,
    required bool userPresenceConfirmed,
  }) async {
    throw const RootKeyTransferException(
      code: 'root_transfer.unsupported',
      retryable: false,
    );
  }
}

class MessageApi {
  MessageApi._();

  Future<SendMessageResult> sendText(SendTextRequest request) async {
    throw _unsupported();
  }

  Future<SendMessageResult> sendPayload(SendPayloadRequest request) async {
    throw _unsupported();
  }

  Future<SendMessageResult> sendConversationText(
    SendConversationTextRequest request,
  ) async {
    throw _unsupported();
  }

  Future<SendMessageResult> sendConversationPayload(
    SendConversationPayloadRequest request,
  ) async {
    throw _unsupported();
  }

  Future<MessagePage> localHistory(
    ThreadRef thread, {
    required int limit,
    String? cursor,
  }) async {
    throw _unsupported();
  }

  Future<MessagePage> localConversationTimeline(
    ConversationReadRef conversation, {
    required int limit,
    String? cursor,
  }) async {
    throw _unsupported();
  }

  Future<MarkThreadReadResult> markThreadRead(
    ThreadRef thread, {
    ReadWatermark? watermark,
    int? fallbackMaxMessageIds,
  }) async {
    throw _unsupported();
  }

  Future<MarkThreadReadResult> markConversationRead(
    ConversationReadRef conversation, {
    ReadWatermark? watermark,
    int? fallbackMaxMessageIds,
  }) async {
    throw _unsupported();
  }

  Future<SyncDeltaResult> syncDelta(SyncDeltaRequest request) async {
    throw _unsupported();
  }

  Future<MessageSyncOutcome> syncNow(MessageSyncRequest request) async {
    throw _unsupported();
  }

  Future<MessageSyncDiagnostics> syncDiagnostics() async {
    throw _unsupported();
  }

  Future<SyncThreadAfterResult> syncThreadAfter(
    SyncThreadAfterRequest request,
  ) async {
    throw _unsupported();
  }

  Future<SyncThreadAfterResult> syncConversationAfter(
    SyncConversationAfterRequest request,
  ) async {
    throw _unsupported();
  }

  Future<ConversationListSnapshot?> loadConversationSnapshot() async {
    throw _unsupported();
  }

  Future<void> clearConversationSnapshot() async {
    throw _unsupported();
  }

  Stream<ConversationStorePatch> watchConversationPatches() {
    throw _unsupported();
  }

  Future<ConversationStorePatch> repairConversationStore() async {
    throw _unsupported();
  }

  Stream<ThreadMessageStorePatch> watchThreadPatches(
    ThreadRef thread, {
    int limit = 100,
  }) {
    throw _unsupported();
  }

  Stream<ThreadMessageStorePatch> watchConversationTimelinePatches(
    ConversationReadRef conversation, {
    int limit = 100,
  }) {
    throw _unsupported();
  }

  Future<ThreadMessageStorePatch> repairThreadStore(
    ThreadRef thread, {
    int limit = 100,
  }) async {
    throw _unsupported();
  }

  Future<ThreadMessageStorePatch> repairConversationTimelineStore(
    ConversationReadRef conversation, {
    int limit = 100,
  }) async {
    throw _unsupported();
  }
}

class AttachmentApi {
  AttachmentApi._();

  Future<AttachmentSendResult> send(AttachmentSendRequest request) async {
    throw _unsupported();
  }

  Future<AttachmentSendResult> sendConversation(
    SendConversationAttachmentRequest request,
  ) async {
    throw _unsupported();
  }

  Future<DownloadedAttachment> download(
    DownloadAttachmentRequest request,
  ) async {
    throw _unsupported();
  }

  Future<bool> cancelDownload(String localPath) async {
    throw _unsupported();
  }
}

class SecureApi {
  SecureApi._();

  DirectSecureConversation direct(String peer) => DirectSecureConversation._();

  GroupSecureConversation group(String group) => GroupSecureConversation._();

  SecureOutboxApi get outbox => SecureOutboxApi._();
}

class DirectSecureConversation {
  DirectSecureConversation._();

  Future<DirectSecureStatus> status() async {
    throw _unsupported();
  }

  Future<DirectSecurePrepareResult> prepare() async {
    throw _unsupported();
  }

  Future<DirectSecureRepairResult> repair() async {
    throw _unsupported();
  }
}

class GroupSecureConversation {
  GroupSecureConversation._();

  Future<GroupSecureStatus> status() async {
    throw _unsupported();
  }

  Future<GroupSecurePrepareResult> prepare() async {
    throw _unsupported();
  }

  Future<GroupSecureRepairResult> repair() async {
    throw _unsupported();
  }
}

class SecureOutboxApi {
  SecureOutboxApi._();

  Future<List<SecureOutboxEntry>> listFailed() async {
    throw _unsupported();
  }

  Future<SecureOutboxResult> retry(String outboxId) async {
    throw _unsupported();
  }

  Future<SecureOutboxResult> drop(String outboxId) async {
    throw _unsupported();
  }
}
