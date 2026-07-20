import 'models/attachment.dart';
import 'models/config.dart';
import 'models/identity.dart';
import 'models/local_state_upgrade.dart';
import 'models/message.dart';
import 'models/secure.dart';

UnsupportedError _unsupported() => UnsupportedError(
  'awiki_im_core native Rust backend is not supported on Flutter Web.',
);

class DeviceJoinAccountVerificationGrant {
  DeviceJoinAccountVerificationGrant.fromToken(String token) {
    if (token.trim().isEmpty) {
      throw ArgumentError('account verification grant must not be empty');
    }
  }

  @override
  String toString() => 'DeviceJoinAccountVerificationGrant(<redacted>)';
}

class HandleRecoveryBeginVerificationGrant {
  HandleRecoveryBeginVerificationGrant.fromToken(String token) {
    if (token.trim().isEmpty) {
      throw ArgumentError(
        'recovery begin verification grant must not be empty',
      );
    }
  }

  @override
  String toString() => 'HandleRecoveryBeginVerificationGrant(<redacted>)';
}

class HandleRecoveryFinalizeVerificationGrant {
  HandleRecoveryFinalizeVerificationGrant.fromToken(String token) {
    if (token.trim().isEmpty) {
      throw ArgumentError(
        'recovery finalize verification grant must not be empty',
      );
    }
  }

  @override
  String toString() => 'HandleRecoveryFinalizeVerificationGrant(<redacted>)';
}

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

  Future<IdentityDeviceSummary> identityDeviceSummary(
    IdentitySelector selector,
  ) async {
    throw _unsupported();
  }

  Future<RootKeyTransferSendResult> sendRootKeyTransfer({
    required IdentitySelector selector,
    required String recipientDeviceId,
    required String messageId,
    required bool userPresenceConfirmed,
  }) async {
    throw _unsupported();
  }

  Future<List<RootKeyTransferSummary>> listRootKeyTransfers({
    required IdentitySelector selector,
    bool includeCompleted = false,
  }) async {
    throw _unsupported();
  }

  Future<RootKeyTransferSummary> retryRootKeyTransfer({
    required IdentitySelector selector,
    required String messageId,
    required bool userPresenceConfirmed,
  }) async {
    throw _unsupported();
  }

  Future<List<HandleRecoveryProgress>> localHandleRecoverySessions() async {
    throw _unsupported();
  }

  Future<HandleRecoveryProgress> beginHandleRecovery({
    required String handle,
    required HandleRecoveryBeginVerificationGrant verificationGrant,
  }) async {
    throw _unsupported();
  }

  Future<HandleRecoveryProgress> pollHandleRecovery(
    String recoverySessionId,
  ) async {
    throw _unsupported();
  }

  Future<HandleRecoveryCancelResult> cancelHandleRecovery({
    required IdentitySelector oldIdentity,
    required String recoverySessionId,
    required bool userPresenceConfirmed,
  }) async {
    throw _unsupported();
  }

  Future<HandleRecoveryFinalizeResult> finalizeHandleRecovery({
    required String recoverySessionId,
    required HandleRecoveryFinalizeVerificationGrant verificationGrant,
    required bool userPresenceConfirmed,
  }) async {
    throw _unsupported();
  }

  Future<IdentitySummary> resumeHandleRecoveryActivation(
    String recoverySessionId,
  ) async {
    throw _unsupported();
  }

  Future<void> markHandleRecoveryActivationComplete(
    String recoverySessionId,
  ) async {
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

  Future<DeviceJoinProgress> claimDeviceJoin({
    required IdentitySelector selector,
    required String joinSessionId,
    required String operationId,
    int challengeTtlSeconds = 300,
  }) async {
    throw _unsupported();
  }

  Future<DeviceJoinProgress> pollAdminDeviceJoin({
    required IdentitySelector selector,
    required String joinSessionId,
  }) async {
    throw _unsupported();
  }

  Future<DeviceJoinApprovalPrompt> prepareDeviceJoinApproval({
    required IdentitySelector selector,
    required String joinSessionId,
    DeviceJoinRole role = DeviceJoinRole.member,
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

  Future<DeviceJoinSessionSummary> cancelAdminDeviceJoin({
    required IdentitySelector selector,
    required String joinSessionId,
  }) async {
    throw _unsupported();
  }

  Future<IdentityVaultStatus> identityVaultStatus(
    IdentitySelector selector,
  ) async {
    throw _unsupported();
  }

  Future<IdentityVaultMigrationReport> migrateIdentityVault(
    IdentitySelector selector,
  ) async {
    throw _unsupported();
  }

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

  Future<DaemonSubkeyPrivatePackage> loadDaemonSubkeyPackage(
    IdentitySelector selector,
  ) async {
    throw _unsupported();
  }

  Future<DaemonSubkeyPrivatePackage> ensureDaemonSubkeyPackage(
    IdentitySelector selector,
  ) async {
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

  Future<void> dispose() async {}
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
