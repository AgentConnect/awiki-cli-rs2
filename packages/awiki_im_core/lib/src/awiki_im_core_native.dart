import 'dart:async';
import 'dart:typed_data';

import 'generated/api/auth.dart' as gen_auth;
import 'generated/api/attachments.dart' as gen_attachments;
import 'generated/api/client.dart' as gen_client;
import 'generated/api/core.dart' as gen_core;
import 'generated/api/directory.dart' as gen_directory;
import 'generated/api/email.dart' as gen_email;
import 'generated/api/groups.dart' as gen_groups;
import 'generated/api/identity.dart' as gen_identity_api;
import 'generated/api/local_state_upgrade.dart' as gen_local_state_upgrade;
import 'generated/api/messages.dart' as gen_messages;
import 'generated/api/profile.dart' as gen_profile;
import 'generated/api/realtime.dart' as gen_realtime;
import 'generated/api/secure.dart' as gen_secure;
import 'generated/dto/auth.dart' as gen_auth_dto;
import 'generated/dto/attachment.dart' as gen_attachment;
import 'generated/dto/config.dart' as gen_config;
import 'generated/dto/directory.dart' as gen_directory_dto;
import 'generated/dto/email.dart' as gen_email_dto;
import 'generated/dto/error.dart' as gen_error;
import 'generated/dto/group.dart' as gen_group_dto;
import 'generated/dto/identity.dart' as gen_identity;
import 'generated/dto/local_state_upgrade.dart' as gen_local_state_upgrade_dto;
import 'generated/dto/message.dart' as gen_message;
import 'generated/dto/profile.dart' as gen_profile_dto;
import 'generated/dto/realtime.dart' as gen_realtime_dto;
import 'generated/dto/secure.dart' as gen_secure_dto;
import 'generated/frb_generated.dart' as gen;
import 'models/auth.dart';
import 'models/attachment.dart';
import 'models/config.dart';
import 'models/directory.dart';
import 'models/email.dart';
import 'models/error.dart';
import 'models/group.dart';
import 'models/identity.dart';
import 'models/local_state_upgrade.dart';
import 'models/message.dart';
import 'models/message_payload.dart';
import 'models/profile.dart';
import 'models/realtime.dart';
import 'models/secure.dart';
import 'native_library_loader.dart';

bool _rustLibInitialized = false;

Future<void> _ensureRustLibInitialized() async {
  if (_rustLibInitialized) return;
  await gen.RustLib.init(externalLibrary: loadAwikiImCoreExternalLibrary());
  _rustLibInitialized = true;
}

Future<T> _mapNativeErrors<T>(Future<T> Function() action) async {
  try {
    return await action();
  } on gen_error.DartImError catch (error) {
    throw AwikiImCoreException(
      code: error.code,
      message: error.message,
      field: error.field,
      statusCode: error.statusCode,
      capability: error.capability,
      serviceCode: error.serviceCode,
      serviceDataJson: error.serviceDataJson,
      deviceRevokeOutcomeCategory: error.deviceRevokeOutcomeCategory
          ?._toModel(),
      handleRecoveryFailureCode: _handleRecoveryFailureCode(error.serviceCode),
    );
  }
}

HandleRecoveryFailureCode? _handleRecoveryFailureCode(String? code) =>
    switch (code) {
      'factor_retry_required' => HandleRecoveryFailureCode.factorRetryRequired,
      'result_absent' => HandleRecoveryFailureCode.resultAbsent,
      'outcome_unknown' => HandleRecoveryFailureCode.outcomeUnknown,
      'local_key_unavailable' => HandleRecoveryFailureCode.localKeyUnavailable,
      'local_transition_pending' =>
        HandleRecoveryFailureCode.localTransitionPending,
      'local_migration_unsupported' =>
        HandleRecoveryFailureCode.localMigrationUnsupported,
      'unknown_epoch' => HandleRecoveryFailureCode.unknownEpoch,
      _ => null,
    };

extension on gen_error.DartDeviceRevokeOutcomeCategory {
  DeviceRevokeOutcomeCategory _toModel() => switch (this) {
    gen_error.DartDeviceRevokeOutcomeCategory.cancelledBeforeSubmit =>
      DeviceRevokeOutcomeCategory.cancelledBeforeSubmit,
    gen_error.DartDeviceRevokeOutcomeCategory.rejectedBeforeCommit =>
      DeviceRevokeOutcomeCategory.rejectedBeforeCommit,
    gen_error.DartDeviceRevokeOutcomeCategory.outcomeUnknown =>
      DeviceRevokeOutcomeCategory.outcomeUnknown,
  };
}

class AwikiImCore {
  AwikiImCore._(this._inner);

  final gen_client.ArcDartImCore _inner;
  bool _disposed = false;

  static Future<AwikiImCore> open({
    required AwikiImCoreConfig config,
    required AwikiImCorePaths paths,
    AwikiImCoreOpenOptions? openOptions,
  }) async {
    await _ensureRustLibInitialized();
    final inner = await _mapNativeErrors(
      () => gen_core.openCoreWithOptionalOptions(
        config: config._toGen(),
        paths: paths._toGen(),
        options: openOptions?._toGen(),
      ),
    );
    return AwikiImCore._(inner);
  }

  static Future<LocalStateUpgradeInspection> inspectLocalStateUpgrade({
    required AwikiImCorePaths paths,
  }) async {
    await _ensureRustLibInitialized();
    final result = await _mapNativeErrors(
      () => gen_local_state_upgrade.inspectLocalStateUpgrade(
        paths: paths._toGen(),
      ),
    );
    return result._toModel();
  }

  static Future<LocalStateUpgradeResult> upgradeLocalState({
    required AwikiImCorePaths paths,
  }) async {
    await _ensureRustLibInitialized();
    final result = await _mapNativeErrors(
      () => gen_local_state_upgrade.upgradeLocalState(paths: paths._toGen()),
    );
    return result._toModel();
  }

  static Future<LocalStateRestoreResult> restoreLocalStateBackup({
    required AwikiImCorePaths paths,
  }) async {
    await _ensureRustLibInitialized();
    final result = await _mapNativeErrors(
      () => gen_local_state_upgrade.restoreLocalStateBackup(
        paths: paths._toGen(),
      ),
    );
    return result._toModel();
  }

  Future<AwikiImClient> client(IdentitySelector selector) async {
    _ensureNotDisposed();
    final inner = await _mapNativeErrors(
      () => gen_client.coreClient(core: _inner, selector: selector._toGen()),
    );
    return AwikiImClient._(inner);
  }

  Future<List<String>> validatePaths() async {
    _ensureNotDisposed();
    return _mapNativeErrors(() => gen_core.validatePaths(core: _inner));
  }

  Future<List<IdentitySummary>> listIdentities() async {
    _ensureNotDisposed();
    final identities = await _mapNativeErrors(
      () => gen_identity_api.listIdentities(core: _inner),
    );
    return identities.map((identity) => identity._toModel()).toList();
  }

  Future<IdentitySummary?> defaultIdentity() async {
    _ensureNotDisposed();
    final identity = await _mapNativeErrors(
      () => gen_identity_api.defaultIdentity(core: _inner),
    );
    return identity?._toModel();
  }

  Future<IdentitySummary> resolveIdentity(IdentitySelector selector) async {
    _ensureNotDisposed();
    final identity = await _mapNativeErrors(
      () => gen_identity_api.resolveIdentity(
        core: _inner,
        selector: selector._toGen(),
      ),
    );
    return identity._toModel();
  }

  Future<IdentitySummary> updateDisplayNameProjection({
    required String identityId,
    String? displayName,
  }) async {
    _ensureNotDisposed();
    final identity = await _mapNativeErrors(
      () => gen_identity_api.updateDisplayNameProjection(
        core: _inner,
        identityId: identityId,
        displayName: displayName,
      ),
    );
    return identity._toModel();
  }

  Future<IdentityDeviceSummary> identityDeviceSummary(
    IdentitySelector selector,
  ) async {
    _ensureNotDisposed();
    final summary = await _mapNativeErrors(
      () => gen_identity_api.identityDeviceSummary(
        core: _inner,
        selector: selector._toGen(),
      ),
    );
    return summary._toModel();
  }

  Future<LegacyRegistryEpochAdoptionAuthority?>
  legacyRegistryEpochAdoptionAuthority(IdentitySelector selector) async {
    _ensureNotDisposed();
    final authority = await _mapNativeErrors(
      () => gen_identity_api.legacyRegistryEpochAdoptionAuthority(
        core: _inner,
        selector: selector._toGen(),
      ),
    );
    return authority?._toModel();
  }

  Future<HandleRecoveryOtpResult> requestHandleRecoveryOtp({
    IdentitySelector? selector,
    required String fullHandle,
    required String phone,
  }) async {
    _ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_identity_api.requestHandleRecoveryOtp(
        core: _inner,
        selector: selector?._toGen(),
        fullHandle: fullHandle,
        phone: phone,
      ),
    );
    return result._toModel();
  }

  Future<HandleRecoveryProgress> prepareHandleRecovery({
    required String operationId,
    required String phone,
    required String code,
  }) async {
    _ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_identity_api.prepareHandleRecovery(
        core: _inner,
        operationId: operationId,
        phone: phone,
        code: code,
      ),
    );
    return result._toModel();
  }

  Future<HandleRecoveryProgress> activateHandleRecovery({
    required String operationId,
    required bool userPresenceConfirmed,
  }) async {
    _ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_identity_api.activateHandleRecovery(
        core: _inner,
        operationId: operationId,
        userPresenceConfirmed: userPresenceConfirmed,
      ),
    );
    return result._toModel();
  }

  Future<HandleRecoveryProgress> resumeHandleRecovery(
    String operationId,
  ) async {
    _ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_identity_api.resumeHandleRecovery(
        core: _inner,
        operationId: operationId,
      ),
    );
    return result._toModel();
  }

  Future<HandleRecoveryProgress> handleRecoveryStatus(
    String operationId,
  ) async {
    _ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_identity_api.handleRecoveryStatus(
        core: _inner,
        operationId: operationId,
      ),
    );
    return result._toModel();
  }

  Future<List<HandleRecoveryOperationSummary>> listHandleRecoveryOperations(
    IdentitySelector selector,
  ) async {
    _ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_identity_api.listHandleRecoveryOperations(
        core: _inner,
        selector: selector._toGen(),
      ),
    );
    return result.map((operation) => operation._toModel()).toList();
  }

  Future<HandleRecoveryOperationSummary> discardHandleRecoveryPreAttempt(
    String operationId,
  ) async {
    _ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_identity_api.discardHandleRecoveryPreAttempt(
        core: _inner,
        operationId: operationId,
      ),
    );
    return result._toModel();
  }

  Future<HandleRecoveryOperationSummary>
  quarantineHandleRecoveryKeyUnavailable({
    required String operationId,
    required bool userPresenceConfirmed,
  }) async {
    _ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_identity_api.quarantineHandleRecoveryKeyUnavailable(
        core: _inner,
        operationId: operationId,
        userPresenceConfirmed: userPresenceConfirmed,
      ),
    );
    return result._toModel();
  }

  Future<HandleRecoveryAccountEpochReceipt?> authorizedHandleRecoveryReceipt(
    IdentitySelector selector,
  ) async {
    _ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_identity_api.authorizedHandleRecoveryReceipt(
        core: _inner,
        selector: selector._toGen(),
      ),
    );
    return result?._toModel();
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
    _ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_identity_api.activateAuthorizedJoin(
        core: _inner,
        selector: selector._toGen(),
        phone: phone,
        code: code,
        handle: handle,
        did: did,
        operationId: operationId,
        ttlSeconds: ttlSeconds == null ? null : BigInt.from(ttlSeconds),
        userPresenceConfirmed: userPresenceConfirmed,
      ),
    );
    return result._toModel();
  }

  Future<AuthorizedJoinActivationProgress> resumeAuthorizedJoinActivation(
    String joinSessionId,
  ) async {
    _ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_identity_api.resumeAuthorizedJoinActivation(
        core: _inner,
        joinSessionId: joinSessionId,
      ),
    );
    return result._toModel();
  }

  Future<DeviceRevokeResult> revokeDevice({
    required IdentitySelector selector,
    required String targetDeviceId,
    required bool userPresenceConfirmed,
  }) async {
    _ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_identity_api.revokeDevice(
        core: _inner,
        selector: selector._toGen(),
        targetDeviceId: targetDeviceId,
        userPresenceConfirmed: userPresenceConfirmed,
      ),
    );
    return result._toModel();
  }

  Future<List<DeviceJoinSessionSummary>> localDeviceJoinSessions() async {
    _ensureNotDisposed();
    final sessions = await _mapNativeErrors(
      () => gen_identity_api.localDeviceJoinSessions(core: _inner),
    );
    return sessions.map((session) => session._toModel()).toList();
  }

  Future<DeviceJoinProgress> beginDeviceJoin({
    required String did,
    required String operationId,
    int ttlSeconds = 600,
    required DeviceJoinAccountVerificationGrant accountVerificationGrant,
  }) async {
    _ensureNotDisposed();
    final grantBytes = accountVerificationGrant.takeBytes();
    try {
      final progress = await _mapNativeErrors(
        () => gen_identity_api.beginDeviceJoin(
          core: _inner,
          did: did,
          operationId: operationId,
          ttlSeconds: BigInt.from(ttlSeconds),
          accountVerificationGrant: grantBytes,
        ),
      );
      return progress._toModel();
    } finally {
      grantBytes.fillRange(0, grantBytes.length, 0);
    }
  }

  Future<AuthorizedJoinActivationProgress> beginPreparedRegistrationDeviceJoin({
    required String preparationId,
    required String operationId,
    int ttlSeconds = 600,
    required bool userPresenceConfirmed,
  }) async {
    _ensureNotDisposed();
    final progress = await _mapNativeErrors(
      () => gen_identity_api.beginPreparedRegistrationDeviceJoin(
        core: _inner,
        preparationId: preparationId,
        operationId: operationId,
        ttlSeconds: BigInt.from(ttlSeconds),
        userPresenceConfirmed: userPresenceConfirmed,
      ),
    );
    return progress._toModel();
  }

  Future<DeviceJoinProgress> pollNewDeviceJoin(String joinSessionId) async {
    _ensureNotDisposed();
    final progress = await _mapNativeErrors(
      () => gen_identity_api.pollNewDeviceJoin(
        core: _inner,
        joinSessionId: joinSessionId,
      ),
    );
    return progress._toModel();
  }

  Future<DeviceJoinSessionSummary> cancelNewDeviceJoin(
    String joinSessionId,
  ) async {
    _ensureNotDisposed();
    final session = await _mapNativeErrors(
      () => gen_identity_api.cancelNewDeviceJoin(
        core: _inner,
        joinSessionId: joinSessionId,
      ),
    );
    return session._toModel();
  }

  Future<DeviceJoinRegistrySnapshot> identityDeviceRegistry(
    IdentitySelector selector,
  ) async {
    _ensureNotDisposed();
    final snapshot = await _mapNativeErrors(
      () => gen_identity_api.identityDeviceRegistry(
        core: _inner,
        selector: selector._toGen(),
      ),
    );
    return snapshot._toModel();
  }

  Future<List<DeviceJoinRequestNotice>> localDeviceJoinRequests(
    IdentitySelector selector,
  ) async {
    _ensureNotDisposed();
    final requests = await _mapNativeErrors(
      () => gen_identity_api.localDeviceJoinRequests(
        core: _inner,
        selector: selector._toGen(),
      ),
    );
    return requests.map((request) => request._toModel()).toList();
  }

  Future<DeviceJoinProgress> localDeviceJoinVerificationProgress({
    required IdentitySelector selector,
    required String joinSessionId,
  }) async {
    _ensureNotDisposed();
    final progress = await _mapNativeErrors(
      () => gen_identity_api.localDeviceJoinVerificationProgress(
        core: _inner,
        selector: selector._toGen(),
        joinSessionId: joinSessionId,
      ),
    );
    return progress._toModel();
  }

  Future<DeviceJoinProgress> startDeviceJoinVerification({
    required IdentitySelector selector,
    required String joinSessionId,
    required String operationId,
    int challengeTtlSeconds = 300,
  }) async {
    _ensureNotDisposed();
    final progress = await _mapNativeErrors(
      () => gen_identity_api.startDeviceJoinVerification(
        core: _inner,
        selector: selector._toGen(),
        joinSessionId: joinSessionId,
        operationId: operationId,
        challengeTtlSeconds: BigInt.from(challengeTtlSeconds),
      ),
    );
    return progress._toModel();
  }

  Future<DeviceJoinProgress> rejectDeviceJoin({
    required IdentitySelector selector,
    required String joinSessionId,
    required DeviceJoinRejectReason reason,
  }) async {
    _ensureNotDisposed();
    final progress = await _mapNativeErrors(
      () => gen_identity_api.rejectDeviceJoin(
        core: _inner,
        selector: selector._toGen(),
        joinSessionId: joinSessionId,
        reason: reason._toGen(),
      ),
    );
    return progress._toModel();
  }

  Future<DeviceJoinApprovalPrompt> prepareDeviceJoinApproval({
    required IdentitySelector selector,
    required String joinSessionId,
    required bool sasConfirmed,
  }) async {
    _ensureNotDisposed();
    final prompt = await _mapNativeErrors(
      () => gen_identity_api.prepareDeviceJoinApproval(
        core: _inner,
        selector: selector._toGen(),
        joinSessionId: joinSessionId,
        sasConfirmed: sasConfirmed,
      ),
    );
    return prompt._toModel();
  }

  Future<DeviceJoinProgress> confirmDeviceJoinApproval({
    required String approvalHandle,
    required bool userPresenceConfirmed,
  }) async {
    _ensureNotDisposed();
    final progress = await _mapNativeErrors(
      () => gen_identity_api.confirmDeviceJoinApproval(
        core: _inner,
        approvalHandle: approvalHandle,
        userPresenceConfirmed: userPresenceConfirmed,
      ),
    );
    return progress._toModel();
  }

  Future<IdentityVaultStatus> identityVaultStatus(
    IdentitySelector selector,
  ) async {
    _ensureNotDisposed();
    final status = await _mapNativeErrors(
      () => gen_identity_api.identityVaultStatus(
        core: _inner,
        selector: selector._toGen(),
      ),
    );
    return status._toModel();
  }

  Future<LegacyUpgradeStatus> legacyUpgradeStatus(
    IdentitySelector selector,
  ) async {
    _ensureNotDisposed();
    final status = await _mapNativeErrors(
      () => gen_identity_api.legacyUpgradeStatus(
        core: _inner,
        selector: selector._toGen(),
      ),
    );
    return status._toModel();
  }

  Future<LegacyUpgradeStatus> upgradeLegacyIdentity(
    IdentitySelector selector,
  ) async {
    _ensureNotDisposed();
    final status = await _mapNativeErrors(
      () => gen_identity_api.upgradeLegacyIdentity(
        core: _inner,
        selector: selector._toGen(),
      ),
    );
    return status._toModel();
  }

  Future<IdentityVaultMigrationReport> migrateIdentityVault(
    IdentitySelector selector,
  ) async {
    _ensureNotDisposed();
    final report = await _mapNativeErrors(
      () => gen_identity_api.migrateIdentityVault(
        core: _inner,
        selector: selector._toGen(),
      ),
    );
    return report._toModel();
  }

  Future<IdentityVaultVerificationReport> verifyIdentityVault(
    IdentitySelector selector,
  ) async {
    _ensureNotDisposed();
    final report = await _mapNativeErrors(
      () => gen_identity_api.verifyIdentityVault(
        core: _inner,
        selector: selector._toGen(),
      ),
    );
    return report._toModel();
  }

  Future<DeleteLocalIdentityResult> deleteLocalIdentity(
    IdentitySelector selector,
  ) async {
    _ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_identity_api.deleteLocalIdentity(
        core: _inner,
        selector: selector._toGen(),
      ),
    );
    return result._toModel();
  }

  Future<DaemonSubkeyPrivatePackage> loadDaemonSubkeyPackage(
    IdentitySelector selector,
  ) async {
    _ensureNotDisposed();
    final package = await _mapNativeErrors(
      () => gen_identity_api.loadDaemonSubkeyPackage(
        core: _inner,
        selector: selector._toGen(),
      ),
    );
    return package._toModel();
  }

  Future<DaemonSubkeyPrivatePackage> ensureDaemonSubkeyPackage(
    IdentitySelector selector,
  ) async {
    _ensureNotDisposed();
    final package = await _mapNativeErrors(
      () => gen_identity_api.ensureDaemonSubkeyPackage(
        core: _inner,
        selector: selector._toGen(),
      ),
    );
    return package._toModel();
  }

  Future<DaemonSubkeyAuthorizationRevokeResult> revokeDaemonSubkeyAuthorization(
    IdentitySelector selector,
  ) async {
    _ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_identity_api.revokeDaemonSubkeyAuthorization(
        core: _inner,
        selector: selector._toGen(),
      ),
    );
    return result._toModel();
  }

  Future<HandleRegistrationResult> registerHandleWithPhone({
    String? localAlias,
    required String requestedHandle,
    required String phone,
    String? otp,
    String? inviteCode,
    InitialProfile profile = const InitialProfile(),
    bool makeDefault = true,
  }) async {
    _ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_identity_api.registerHandleWithPhone(
        core: _inner,
        localAlias: localAlias,
        requestedHandle: requestedHandle,
        phone: phone,
        otp: otp,
        inviteCode: inviteCode,
        profile: profile._toGen(),
        makeDefault: makeDefault,
      ),
    );
    return result._toModel();
  }

  Future<HandleRegistrationResult> registerHandleWithEmail({
    String? localAlias,
    required String requestedHandle,
    required String email,
    bool waitForVerification = true,
    String? inviteCode,
    InitialProfile profile = const InitialProfile(),
    bool makeDefault = true,
  }) async {
    _ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_identity_api.registerHandleWithEmail(
        core: _inner,
        localAlias: localAlias,
        requestedHandle: requestedHandle,
        email: email,
        waitForVerification: waitForVerification,
        inviteCode: inviteCode,
        profile: profile._toGen(),
        makeDefault: makeDefault,
      ),
    );
    return result._toModel();
  }

  Future<HandleRegistrationResult> registerHandleWithoutContactVerification({
    String? localAlias,
    required String requestedHandle,
    String? inviteCode,
    InitialProfile profile = const InitialProfile(),
    bool makeDefault = true,
  }) async {
    _ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_identity_api.registerHandleWithoutContactVerification(
        core: _inner,
        localAlias: localAlias,
        requestedHandle: requestedHandle,
        inviteCode: inviteCode,
        profile: profile._toGen(),
        makeDefault: makeDefault,
      ),
    );
    return result._toModel();
  }

  Future<void> dispose() async {
    if (_disposed) return;
    await _mapNativeErrors(() => gen_core.closeCore(core: _inner));
    _disposed = true;
  }

  void _ensureNotDisposed() {
    if (_disposed) {
      throw const AwikiImCoreException(
        code: 'object_closed',
        message: 'core disposed',
      );
    }
  }
}

class AwikiImClient {
  AwikiImClient._(this._inner);

  final gen_attachments.ArcDartImClient _inner;
  final StreamController<RealtimeEvent> _eventsController =
      StreamController<RealtimeEvent>.broadcast();
  final StreamController<RealtimeConnectionState> _connectionStatesController =
      StreamController<RealtimeConnectionState>.broadcast();
  gen_realtime.ArcDartRealtimeSession? _realtimeSession;
  StreamSubscription<RealtimeEvent>? _realtimeEventSubscription;
  RealtimeOptions? _realtimeOptions;
  int? _realtimeLogicalSessionId;
  int _nextRealtimeLogicalSessionId = 0;
  int _nativeRealtimeGeneration = 0;
  Future<void> _lifecycleTail = Future<void>.value();
  bool _disposing = false;
  bool _disposed = false;

  AuthApi get auth => AuthApi._(this);
  IdentityApi get identity => IdentityApi._(this);
  DirectoryApi get directory => DirectoryApi._(this);
  ProfileApi get profile => ProfileApi._(this);
  MessageApi get messages => MessageApi._(this);
  AttachmentApi get attachments => AttachmentApi._(this);
  EmailApi get email => EmailApi._(this);
  GroupApi get groups => GroupApi._(this);
  RealtimeApi get realtime => RealtimeApi._(this);
  SecureApi get secure => SecureApi._(this);
  RootKeyTransferApi get rootKeyTransfer => RootKeyTransferApi._(this);

  Stream<RealtimeEvent> get events => _eventsController.stream;

  Stream<RealtimeConnectionState> get connectionStates =>
      _connectionStatesController.stream;

  Future<ActiveSyncAccountBinding> activeSyncAccountBinding() async {
    _ensureNotDisposed();
    final binding = await _mapNativeErrors(
      () => gen_identity_api.activeSyncAccountBinding(client: _inner),
    );
    return binding._toModel();
  }

  Future<void> dispose() async {
    if (_disposed || _disposing) return;
    _disposing = true;
    try {
      await _runClientLifecycle(() async {
        try {
          await _stopNativeRealtimeUnlocked(clearLogicalSession: true);
        } finally {
          await _mapNativeErrors(() => gen_client.closeClient(client: _inner));
        }
      }, allowDisposing: true);
    } finally {
      _disposed = true;
      await _eventsController.close();
      await _connectionStatesController.close();
    }
  }

  void _ensureNotDisposed() {
    if (_disposed || _disposing) {
      throw const AwikiImCoreException(
        code: 'object_closed',
        message: 'client disposed',
      );
    }
  }
}

class _NativeRootKeyTransferAuthorizationHandle
    implements RootKeyTransferAuthorizationHandle {
  const _NativeRootKeyTransferAuthorizationHandle(this.value);

  final String value;

  @override
  String toString() => 'RootKeyTransferAuthorizationHandle(<redacted>)';
}

class RootKeyTransferApi {
  RootKeyTransferApi._(this._client);

  final AwikiImClient _client;

  Future<RootKeyTransferPreparation> prepare({
    required String recipientDeviceId,
  }) async {
    _ensureAvailable();
    final preparation = await _mapRootKeyTransferErrors(
      () => gen_identity_api.prepareRootKeyTransfer(
        client: _client._inner,
        recipientDeviceId: recipientDeviceId,
      ),
    );
    return preparation._toModel();
  }

  Future<RootKeyTransferSendResult> confirmAndSend({
    required RootKeyTransferAuthorizationHandle authorizationHandle,
    required bool userPresenceConfirmed,
  }) async {
    _ensureAvailable();
    if (authorizationHandle case _NativeRootKeyTransferAuthorizationHandle(
      :final value,
    )) {
      final result = await _mapRootKeyTransferErrors(
        () => gen_identity_api.confirmAndSendRootKeyTransfer(
          client: _client._inner,
          authorizationHandle: value,
          userPresenceConfirmed: userPresenceConfirmed,
        ),
      );
      return result._toModel();
    }
    throw const RootKeyTransferException(
      code: 'root_transfer.authorization_invalid',
      retryable: false,
    );
  }

  void _ensureAvailable() {
    if (_client._disposed) {
      throw const RootKeyTransferException(
        code: 'root_transfer.temporarily_unavailable',
        retryable: true,
      );
    }
  }
}

class AuthApi {
  AuthApi._(this._client);

  final AwikiImClient _client;

  Future<AuthStatus> status() async {
    _client._ensureNotDisposed();
    final status = await _mapNativeErrors(
      () => gen_auth.authStatus(client: _client._inner),
    );
    return status._toModel();
  }

  Future<SessionBundle> login() async {
    _client._ensureNotDisposed();
    final bundle = await _mapNativeErrors(
      () => gen_auth.authLogin(client: _client._inner),
    );
    return bundle._toModel();
  }

  Future<SessionBundle> ensureSession(AuthScope scope) async {
    _client._ensureNotDisposed();
    final bundle = await _mapNativeErrors(
      () => gen_auth.authEnsureSession(
        client: _client._inner,
        scope: scope._toGen(),
      ),
    );
    return bundle._toModel();
  }

  Future<SessionUpdate> refreshSession() async {
    _client._ensureNotDisposed();
    final update = await _mapNativeErrors(
      () => gen_auth.authRefreshSession(client: _client._inner),
    );
    return update._toModel();
  }
}

class IdentityApi {
  IdentityApi._(this._client);

  final AwikiImClient _client;

  Future<IdentitySummary> current() async {
    _client._ensureNotDisposed();
    final identity = await _mapNativeErrors(
      () => gen_client.currentIdentity(client: _client._inner),
    );
    return identity._toModel();
  }
}

class DirectoryApi {
  DirectoryApi._(this._client);

  final AwikiImClient _client;

  Future<DirectoryResolution> resolvePeer(String peer) async {
    _client._ensureNotDisposed();
    final resolution = await _mapNativeErrors(
      () => gen_directory.resolvePeer(client: _client._inner, peer: peer),
    );
    return resolution._toModel();
  }

  Future<DirectoryResolution> lookupHandle(String handle) async {
    _client._ensureNotDisposed();
    final resolution = await _mapNativeErrors(
      () => gen_directory.lookupHandle(client: _client._inner, handle: handle),
    );
    return resolution._toModel();
  }

  Future<List<DisplayProfile>> hydrateDisplayProfiles(
    List<String> peers,
  ) async {
    _client._ensureNotDisposed();
    final profiles = await _mapNativeErrors(
      () => gen_directory.hydrateDisplayProfiles(
        client: _client._inner,
        peers: peers,
      ),
    );
    return profiles.map((profile) => profile._toModel()).toList();
  }

  Future<RelationStatus> relationStatus(String peer) async {
    _client._ensureNotDisposed();
    final status = await _mapNativeErrors(
      () => gen_directory.relationStatus(client: _client._inner, peer: peer),
    );
    return status._toModel();
  }

  Future<void> follow(String peer) async {
    _client._ensureNotDisposed();
    await _mapNativeErrors(
      () => gen_directory.follow(client: _client._inner, peer: peer),
    );
  }

  Future<void> unfollow(String peer) async {
    _client._ensureNotDisposed();
    await _mapNativeErrors(
      () => gen_directory.unfollow(client: _client._inner, peer: peer),
    );
  }

  Future<RelationshipPage> listFollowers({
    int limit = 100,
    int offset = 0,
    bool hydrateProfiles = false,
  }) async {
    _client._ensureNotDisposed();
    final page = await _mapNativeErrors(
      () => gen_directory.listFollowers(
        client: _client._inner,
        limit: limit,
        offset: offset,
        hydrateProfiles: hydrateProfiles,
      ),
    );
    return page._toModel();
  }

  Future<RelationshipPage> listFollowing({
    int limit = 100,
    int offset = 0,
    bool hydrateProfiles = false,
  }) async {
    _client._ensureNotDisposed();
    final page = await _mapNativeErrors(
      () => gen_directory.listFollowing(
        client: _client._inner,
        limit: limit,
        offset: offset,
        hydrateProfiles: hydrateProfiles,
      ),
    );
    return page._toModel();
  }
}

class ProfileApi {
  ProfileApi._(this._client);

  final AwikiImClient _client;

  Future<UserProfile> loadMyProfile() async {
    _client._ensureNotDisposed();
    final profile = await _mapNativeErrors(
      () => gen_profile.loadMyProfile(client: _client._inner),
    );
    return profile._toModel();
  }

  Future<UserProfile> updateProfile(ProfilePatch patch) async {
    _client._ensureNotDisposed();
    final profile = await _mapNativeErrors(
      () => gen_profile.updateProfile(
        client: _client._inner,
        patch: patch._toGen(),
      ),
    );
    return profile._toModel();
  }

  Future<UserProfile> loadPublicProfile(IdentitySubject subject) async {
    _client._ensureNotDisposed();
    final profile = await _mapNativeErrors(
      () => gen_profile.loadPublicProfile(
        client: _client._inner,
        subject: subject._toGen(),
      ),
    );
    return profile._toModel();
  }
}

class MessageApi {
  MessageApi._(this._client);

  final AwikiImClient _client;

  Future<SendMessageResult> sendText(SendTextRequest request) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_messages.sendText(
        client: _client._inner,
        request: request._toGen(),
      ),
    );
    return result._toModel();
  }

  Future<SendMessageResult> sendPayload(SendPayloadRequest request) async {
    _client._ensureNotDisposed();
    validateMessagePayloadJson(request.payloadJson);
    final result = await _mapNativeErrors(
      () => gen_messages.sendPayload(
        client: _client._inner,
        request: request._toGen(),
      ),
    );
    return result._toModel();
  }

  Future<SendMessageResult> sendConversationText(
    SendConversationTextRequest request,
  ) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_messages.sendConversationText(
        client: _client._inner,
        request: request._toGen(),
      ),
    );
    return result._toModel();
  }

  Future<SendMessageResult> sendConversationPayload(
    SendConversationPayloadRequest request,
  ) async {
    _client._ensureNotDisposed();
    validateMessagePayloadJson(request.payloadJson);
    final result = await _mapNativeErrors(
      () => gen_messages.sendConversationPayload(
        client: _client._inner,
        request: request._toGen(),
      ),
    );
    return result._toModel();
  }

  Future<MessagePage> inbox({
    required int limit,
    String? cursor,
    bool unreadOnly = false,
    InboxHistoryOptions? inboxHistoryOptions,
  }) async {
    _client._ensureNotDisposed();
    final page = await _mapNativeErrors(
      () => gen_messages.inbox(
        client: _client._inner,
        limit: limit,
        cursor: cursor,
        unreadOnly: unreadOnly,
        inboxHistoryOptions: inboxHistoryOptions?._toGen(),
      ),
    );
    return page._toModel();
  }

  Future<MessagePage> history(
    ThreadRef thread, {
    required int limit,
    String? cursor,
    InboxHistoryOptions? inboxHistoryOptions,
  }) async {
    _client._ensureNotDisposed();
    final page = await _mapNativeErrors(
      () => gen_messages.history(
        client: _client._inner,
        thread: thread._toGen(),
        limit: limit,
        cursor: cursor,
        inboxHistoryOptions: inboxHistoryOptions?._toGen(),
      ),
    );
    return page._toModel();
  }

  Future<MessagePage> localHistory(
    ThreadRef thread, {
    required int limit,
    String? cursor,
  }) async {
    _client._ensureNotDisposed();
    final page = await _mapNativeErrors(
      () => gen_messages.localHistory(
        client: _client._inner,
        thread: thread._toGen(),
        limit: limit,
        cursor: cursor,
      ),
    );
    return page._toModel();
  }

  Future<MessagePage> localConversationTimeline(
    ConversationReadRef conversation, {
    required int limit,
    String? cursor,
  }) async {
    _client._ensureNotDisposed();
    final page = await _mapNativeErrors(
      () => gen_messages.localConversationTimeline(
        client: _client._inner,
        conversation: conversation._toGen(),
        limit: limit,
        cursor: cursor,
      ),
    );
    return page._toModel();
  }

  Future<MarkReadResult> markRead(List<String> messageIds) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () =>
          gen_messages.markRead(client: _client._inner, messageIds: messageIds),
    );
    return result._toModel();
  }

  Future<MarkThreadReadResult> markThreadRead(
    ThreadRef thread, {
    ReadWatermark? watermark,
    int? fallbackMaxMessageIds,
  }) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_messages.markThreadRead(
        client: _client._inner,
        thread: thread._toGen(),
        watermark: watermark?._toGen(),
        fallbackMaxMessageIds: fallbackMaxMessageIds,
      ),
    );
    return result._toModel();
  }

  Future<MarkThreadReadResult> markConversationRead(
    ConversationReadRef conversation, {
    ReadWatermark? watermark,
    int? fallbackMaxMessageIds,
  }) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_messages.markConversationRead(
        client: _client._inner,
        request: gen_message.DartMarkConversationReadRequest(
          conversation: conversation._toGen(),
          watermark: watermark?._toGen(),
          fallbackMaxMessageIds: fallbackMaxMessageIds,
        ),
      ),
    );
    return result._toModel();
  }

  Future<SyncDeltaResult> syncDelta(SyncDeltaRequest request) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_messages.syncDelta(
        client: _client._inner,
        request: request._toGen(),
      ),
    );
    return result._toModel();
  }

  Future<MessageSyncOutcome> syncNow(MessageSyncRequest request) async {
    _client._ensureNotDisposed();
    return _client._runClientLifecycle(() async {
      final securePreparation = await _mapNativeErrors(
        () => gen_messages.prepareSecureInboxForSync(
          client: _client._inner,
          limit: request.limit ?? 100,
        ),
      );
      if (securePreparation.authorizationContextChanged) {
        await _client._restartNativeRealtimeUnlocked();
      }
      final result = await _mapNativeErrors(
        () => gen_messages.syncNow(
          client: _client._inner,
          request: gen_message.DartMessageSyncRequest(
            reason: request.reason,
            limit: request.limit,
          ),
        ),
      );
      final outcome = result._toModel();
      if (securePreparation.warnings.isEmpty) {
        return outcome;
      }
      return MessageSyncOutcome(
        status: outcome.status,
        eventsApplied: outcome.eventsApplied,
        pagesFetched: outcome.pagesFetched,
        messagesHydrated: outcome.messagesHydrated,
        duplicatesSkipped: outcome.duplicatesSkipped,
        changedConversationIds: outcome.changedConversationIds,
        committedIncomingMessages: outcome.committedIncomingMessages,
        errorCode: outcome.errorCode,
        warnings: List<String>.unmodifiable([
          ...securePreparation.warnings,
          ...outcome.warnings,
        ]),
      );
    });
  }

  Future<MessageSyncDiagnostics> syncDiagnostics() async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_messages.syncDiagnostics(client: _client._inner),
    );
    return result._toModel();
  }

  Future<SyncThreadAfterResult> syncThreadAfter(
    SyncThreadAfterRequest request,
  ) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_messages.syncThreadAfter(
        client: _client._inner,
        request: request._toGen(),
      ),
    );
    return result._toModel();
  }

  Future<SyncThreadAfterResult> syncConversationAfter(
    SyncConversationAfterRequest request,
  ) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_messages.syncConversationAfter(
        client: _client._inner,
        request: request._toGen(),
      ),
    );
    return result._toModel();
  }

  Future<ConversationPage> conversations({
    required int limit,
    String? cursor,
    bool includeGroups = true,
    bool includeDirect = true,
    bool unreadOnly = false,
  }) async {
    _client._ensureNotDisposed();
    final page = await _mapNativeErrors(
      () => gen_messages.conversations(
        client: _client._inner,
        limit: limit,
        cursor: cursor,
        includeGroups: includeGroups,
        includeDirect: includeDirect,
        unreadOnly: unreadOnly,
      ),
    );
    return page._toModel();
  }

  Future<void> ensureConversation(String conversationId) async {
    _client._ensureNotDisposed();
    await _mapNativeErrors(
      () => gen_messages.ensureConversation(
        client: _client._inner,
        conversationId: conversationId,
      ),
    );
  }

  Future<ConversationListSnapshot?> loadConversationSnapshot() async {
    _client._ensureNotDisposed();
    final snapshot = await _mapNativeErrors(
      () => gen_messages.loadConversationSnapshot(client: _client._inner),
    );
    return snapshot?._toModel();
  }

  Future<void> clearConversationSnapshot() async {
    _client._ensureNotDisposed();
    await _mapNativeErrors(
      () => gen_messages.clearConversationSnapshot(client: _client._inner),
    );
  }

  Stream<ConversationStorePatch> watchConversationPatches() async* {
    _client._ensureNotDisposed();
    final session = await _mapNativeErrors(
      () => gen_messages.watchConversationPatches(client: _client._inner),
    );
    try {
      yield* gen_messages
          .conversationPatchStream(session: session)
          .map((patch) => patch._toModel());
    } finally {
      await _mapNativeErrors(
        () => gen_messages.stopConversationPatchSession(session: session),
      );
    }
  }

  Future<ConversationStorePatch> repairConversationStore() async {
    _client._ensureNotDisposed();
    final patch = await _mapNativeErrors(
      () => gen_messages.repairConversationStore(client: _client._inner),
    );
    return patch._toModel();
  }

  Stream<ThreadMessageStorePatch> watchThreadPatches(
    ThreadRef thread, {
    int limit = 100,
  }) async* {
    _client._ensureNotDisposed();
    final session = await _mapNativeErrors(
      () => gen_messages.watchThreadPatches(
        client: _client._inner,
        thread: thread._toGen(),
        limit: limit,
      ),
    );
    try {
      yield* gen_messages
          .threadMessagePatchStream(session: session)
          .map((patch) => patch._toModel());
    } finally {
      await _mapNativeErrors(
        () => gen_messages.stopThreadMessagePatchSession(session: session),
      );
    }
  }

  Stream<ThreadMessageStorePatch> watchConversationTimelinePatches(
    ConversationReadRef conversation, {
    int limit = 100,
  }) async* {
    _client._ensureNotDisposed();
    final session = await _mapNativeErrors(
      () => gen_messages.watchConversationTimelinePatches(
        client: _client._inner,
        conversation: conversation._toGen(),
        limit: limit,
      ),
    );
    try {
      yield* gen_messages
          .threadMessagePatchStream(session: session)
          .map((patch) => patch._toModel());
    } finally {
      await _mapNativeErrors(
        () => gen_messages.stopThreadMessagePatchSession(session: session),
      );
    }
  }

  Future<ThreadMessageStorePatch> repairThreadStore(
    ThreadRef thread, {
    int limit = 100,
  }) async {
    _client._ensureNotDisposed();
    final patch = await _mapNativeErrors(
      () => gen_messages.repairThreadStore(
        client: _client._inner,
        thread: thread._toGen(),
        limit: limit,
      ),
    );
    return patch._toModel();
  }

  Future<ThreadMessageStorePatch> repairConversationTimelineStore(
    ConversationReadRef conversation, {
    int limit = 100,
  }) async {
    _client._ensureNotDisposed();
    final patch = await _mapNativeErrors(
      () => gen_messages.repairConversationTimelineStore(
        client: _client._inner,
        conversation: conversation._toGen(),
        limit: limit,
      ),
    );
    return patch._toModel();
  }

  Future<SendMessageResult> retryMessage(String messageId) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_messages.retryMessage(
        client: _client._inner,
        messageId: messageId,
      ),
    );
    return result._toModel();
  }
}

class AttachmentApi {
  AttachmentApi._(this._client);

  final AwikiImClient _client;

  Future<AttachmentSendResult> send(AttachmentSendRequest request) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_attachments.sendAttachment(
        client: _client._inner,
        request: request._toGen(),
      ),
    );
    return result._toModel();
  }

  Future<AttachmentSendResult> sendConversation(
    SendConversationAttachmentRequest request,
  ) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_attachments.sendConversationAttachment(
        client: _client._inner,
        request: request._toGen(),
      ),
    );
    return result._toModel();
  }

  Future<DownloadedAttachment> download(
    DownloadAttachmentRequest request,
  ) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_attachments.downloadAttachment(
        client: _client._inner,
        request: request._toGen(),
      ),
    );
    return result._toModel();
  }

  Future<bool> cancelDownload(String localPath) async {
    _client._ensureNotDisposed();
    return gen_attachments.cancelAttachmentDownload(destination: localPath);
  }
}

class EmailApi {
  EmailApi._(this._client);

  final AwikiImClient _client;

  Future<EmailAccount> account() async {
    _client._ensureNotDisposed();
    final account = await _mapNativeErrors(
      () => gen_email.account(client: _client._inner),
    );
    return account._toModel();
  }

  Future<EmailMessageSummaryPage> inbox({
    String folder = 'inbox',
    int limit = 20,
    int offset = 0,
    bool unreadOnly = false,
  }) async {
    _client._ensureNotDisposed();
    final page = await _mapNativeErrors(
      () => gen_email.inbox(
        client: _client._inner,
        folder: folder,
        limit: limit,
        offset: offset,
        unreadOnly: unreadOnly,
      ),
    );
    return page._toModel();
  }

  Future<EmailMessageSummaryPage> inboxWithQuery(EmailInboxQuery query) =>
      inbox(
        folder: query.folder,
        limit: query.limit,
        offset: query.offset,
        unreadOnly: query.unreadOnly,
      );

  Future<EmailMessage> read(String messageId) async {
    _client._ensureNotDisposed();
    final message = await _mapNativeErrors(
      () => gen_email.read(client: _client._inner, messageId: messageId),
    );
    return message._toModel();
  }

  Future<EmailMarkReadResult> markRead(
    List<String> messageIds, {
    bool isRead = true,
  }) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_email.markRead(
        client: _client._inner,
        messageIds: messageIds,
        isRead: isRead,
      ),
    );
    return result._toModel();
  }

  Future<SendEmailResult> send(SendEmailRequest request) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_email.send(client: _client._inner, request: request._toGen()),
    );
    return result._toModel();
  }

  Future<EmailAttachmentContent> downloadAttachment({
    required String messageId,
    required int attachmentIndex,
  }) async {
    _client._ensureNotDisposed();
    final content = await _mapNativeErrors(
      () => gen_email.downloadAttachment(
        client: _client._inner,
        messageId: messageId,
        attachmentIndex: attachmentIndex,
      ),
    );
    return content._toModel();
  }

  Future<EmailNotificationPage> notifications({int limit = 20}) async {
    _client._ensureNotDisposed();
    final page = await _mapNativeErrors(
      () => gen_email.notifications(client: _client._inner, limit: limit),
    );
    return page._toModel();
  }
}

class GroupApi {
  GroupApi._(this._client);

  final AwikiImClient _client;

  Future<GroupReadResult> createGroup(CreateGroupRequest request) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_groups.createGroup(
        client: _client._inner,
        request: request._toGen(),
      ),
    );
    return result._toModel();
  }

  Future<GroupReadResult> joinGroup(String groupDid) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_groups.joinGroup(client: _client._inner, groupDid: groupDid),
    );
    return result._toModel();
  }

  Future<GroupReadResult> joinGroupWithIdentity(
    JoinGroupRequest request,
  ) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_groups.joinGroupWithIdentity(
        client: _client._inner,
        request: request._toGen(),
      ),
    );
    return result._toModel();
  }

  Future<GroupRebindRecoverySummary> resumeRebindRecovery({
    int limit = 100,
  }) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_groups.resumeGroupRebindRecovery(
        client: _client._inner,
        limit: limit,
      ),
    );
    return result._toModel();
  }

  Future<GroupReadResult> getGroup(String groupDid) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_groups.getGroup(client: _client._inner, groupDid: groupDid),
    );
    return result._toModel();
  }

  Future<GroupReadResult> listGroups({
    required int limit,
    String? cursor,
  }) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_groups.listGroups(
        client: _client._inner,
        limit: limit,
        cursor: cursor,
      ),
    );
    return result._toModel();
  }

  Future<GroupReadResult> listMembers(
    String groupDid, {
    required int limit,
    String? cursor,
  }) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_groups.listGroupMembers(
        client: _client._inner,
        groupDid: groupDid,
        limit: limit,
        cursor: cursor,
      ),
    );
    return result._toModel();
  }

  Future<GroupReadResult> addMember(
    String groupDid, {
    required String memberRef,
    String role = 'member',
  }) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_groups.addGroupMember(
        client: _client._inner,
        groupDid: groupDid,
        memberRef: memberRef,
        role: role,
      ),
    );
    return result._toModel();
  }

  Future<GroupReadResult> removeMember(
    String groupDid, {
    required String memberRef,
  }) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_groups.removeGroupMember(
        client: _client._inner,
        groupDid: groupDid,
        memberRef: memberRef,
      ),
    );
    return result._toModel();
  }

  Future<GroupReadResult> listMessages(
    String groupDid, {
    required int limit,
    String? cursor,
  }) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_groups.listGroupMessages(
        client: _client._inner,
        groupDid: groupDid,
        limit: limit,
        cursor: cursor,
      ),
    );
    return result._toModel();
  }

  Future<GroupReadResult> leaveGroup(String groupDid) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_groups.leaveGroup(client: _client._inner, groupDid: groupDid),
    );
    return result._toModel();
  }

  Future<String?> getJoinCode(String groupDid) async {
    _client._ensureNotDisposed();
    return _mapNativeErrors(
      () => gen_groups.getGroupJoinCode(
        client: _client._inner,
        groupDid: groupDid,
      ),
    );
  }

  Future<String?> refreshJoinCode(String groupDid) async {
    _client._ensureNotDisposed();
    return _mapNativeErrors(
      () => gen_groups.refreshGroupJoinCode(
        client: _client._inner,
        groupDid: groupDid,
      ),
    );
  }
}

class RealtimeApi {
  RealtimeApi._(this._client);

  final AwikiImClient _client;

  Future<RealtimeCapability> capability() async {
    _client._ensureNotDisposed();
    final capability = await _mapNativeErrors(
      () => gen_realtime.realtimeCapability(client: _client._inner),
    );
    return capability._toModel();
  }

  Future<RealtimeStatus> status() async {
    _client._ensureNotDisposed();
    final status = await _mapNativeErrors(
      () => gen_realtime.realtimeStatus(client: _client._inner),
    );
    return status._toModel();
  }

  Future<void> connect() async {
    await start();
  }

  Future<RealtimeSession> start({
    RealtimeOptions options = const RealtimeOptions(),
  }) async {
    _client._ensureNotDisposed();
    return _client._runClientLifecycle(() async {
      await _client._stopNativeRealtimeUnlocked(clearLogicalSession: true);
      final logicalSessionId = ++_client._nextRealtimeLogicalSessionId;
      await _client._startNativeRealtimeUnlocked(
        options: options,
        logicalSessionId: logicalSessionId,
      );
      return _NativeRealtimeSession._(_client, logicalSessionId);
    });
  }

  Future<void> stop() async {
    _client._ensureNotDisposed();
    await _client._runClientLifecycle(
      () => _client._stopNativeRealtimeUnlocked(clearLogicalSession: true),
    );
  }
}

class _NativeRealtimeSession implements RealtimeSession {
  _NativeRealtimeSession._(this._client, this._logicalSessionId);

  final AwikiImClient _client;
  final int _logicalSessionId;
  bool _disposed = false;

  @override
  Future<void> stop() async {
    if (_disposed) return;
    if (!_client._disposed && !_client._disposing) {
      await _client._runClientLifecycle(() async {
        if (_client._realtimeLogicalSessionId == _logicalSessionId) {
          await _client._stopNativeRealtimeUnlocked(clearLogicalSession: true);
        }
      });
    }
    _disposed = true;
  }

  @override
  Future<void> dispose() async {
    await stop();
  }
}

extension on AwikiImClient {
  Future<T> _runClientLifecycle<T>(
    Future<T> Function() operation, {
    bool allowDisposing = false,
  }) {
    if (_disposed || (_disposing && !allowDisposing)) {
      return Future<T>.error(
        const AwikiImCoreException(
          code: 'object_closed',
          message: 'client disposed',
        ),
      );
    }
    final completer = Completer<T>();
    _lifecycleTail = _lifecycleTail.then((_) async {
      try {
        completer.complete(await operation());
      } on Object catch (error, stackTrace) {
        completer.completeError(error, stackTrace);
      }
    });
    return completer.future;
  }

  Future<void> _startNativeRealtimeUnlocked({
    required RealtimeOptions options,
    required int logicalSessionId,
  }) async {
    final session = await _mapNativeErrors(
      () =>
          gen_realtime.realtimeStart(client: _inner, options: options._toGen()),
    );
    final nativeGeneration = ++_nativeRealtimeGeneration;
    _realtimeSession = session;
    _realtimeOptions = options;
    _realtimeLogicalSessionId = logicalSessionId;
    _realtimeEventSubscription = gen_realtime
        .realtimeEventStream(session: session)
        .map((event) => event._toModel())
        .listen(
          (event) {
            if (_isCurrentNativeRealtime(
              session: session,
              logicalSessionId: logicalSessionId,
              nativeGeneration: nativeGeneration,
            )) {
              _emitRealtimeEvent(event);
            }
          },
          onError: (Object error, StackTrace stackTrace) {
            if (_isCurrentNativeRealtime(
                  session: session,
                  logicalSessionId: logicalSessionId,
                  nativeGeneration: nativeGeneration,
                ) &&
                !_eventsController.isClosed) {
              _eventsController.addError(error, stackTrace);
            }
          },
        );
  }

  bool _isCurrentNativeRealtime({
    required gen_realtime.ArcDartRealtimeSession session,
    required int logicalSessionId,
    required int nativeGeneration,
  }) {
    return !_disposed &&
        identical(_realtimeSession, session) &&
        _realtimeLogicalSessionId == logicalSessionId &&
        _nativeRealtimeGeneration == nativeGeneration;
  }

  Future<void> _restartNativeRealtimeUnlocked() async {
    final session = _realtimeSession;
    final options = _realtimeOptions;
    final logicalSessionId = _realtimeLogicalSessionId;
    if (session == null || options == null || logicalSessionId == null) {
      return;
    }
    await _stopNativeRealtimeUnlocked(clearLogicalSession: false);
    await _startNativeRealtimeUnlocked(
      options: options,
      logicalSessionId: logicalSessionId,
    );
  }

  Future<void> _stopNativeRealtimeUnlocked({
    required bool clearLogicalSession,
  }) async {
    final session = _realtimeSession;
    final subscription = _realtimeEventSubscription;
    _realtimeSession = null;
    _realtimeEventSubscription = null;
    _nativeRealtimeGeneration += 1;
    if (clearLogicalSession) {
      _realtimeOptions = null;
      _realtimeLogicalSessionId = null;
    }

    Object? firstError;
    StackTrace? firstStackTrace;
    if (session != null) {
      try {
        await _mapNativeErrors(
          () => gen_realtime.realtimeStop(session: session),
        );
      } on Object catch (error, stackTrace) {
        firstError ??= error;
        firstStackTrace ??= stackTrace;
      }
    }
    // The native event stream ends when realtimeStop closes the session.
    // Cancelling the Dart subscription first can therefore wait forever for
    // the very stop operation that this lifecycle step has not run yet.
    try {
      await subscription?.cancel();
    } on Object catch (error, stackTrace) {
      firstError ??= error;
      firstStackTrace ??= stackTrace;
    }
    if (firstError != null) {
      Error.throwWithStackTrace(firstError, firstStackTrace!);
    }
  }

  void _emitRealtimeEvent(RealtimeEvent event) {
    if (!_eventsController.isClosed) {
      _eventsController.add(event);
    }
    if (event.isConnectionState && !_connectionStatesController.isClosed) {
      _connectionStatesController.add(
        RealtimeConnectionState(
          state: event.state ?? 'unknown',
          reason: event.reason,
        ),
      );
    }
  }
}

extension on RealtimeOptions {
  gen_realtime_dto.DartRealtimeOptions _toGen() =>
      gen_realtime_dto.DartRealtimeOptions(
        reconnect: switch (reconnect) {
          RealtimeReconnectMode.disabled => 'disabled',
          RealtimeReconnectMode.fixed => 'fixed',
          RealtimeReconnectMode.exponential => 'exponential',
        },
        eventBuffer: eventBuffer,
        reconnectDelayMs: reconnectDelayMs == null
            ? null
            : BigInt.from(reconnectDelayMs!),
        reconnectBaseDelayMs: reconnectBaseDelayMs == null
            ? null
            : BigInt.from(reconnectBaseDelayMs!),
        reconnectMaxDelayMs: reconnectMaxDelayMs == null
            ? null
            : BigInt.from(reconnectMaxDelayMs!),
        reconnectMaxAttempts: reconnectMaxAttempts,
        subscriptions: subscriptions,
      );
}

extension on gen_realtime_dto.DartRealtimeEvent {
  RealtimeEvent _toModel() => RealtimeEvent(
    kind: kind,
    state: state,
    reason: reason,
    message: message?._toModel(),
    messageId: messageId,
    threadKind: threadKind,
    threadId: threadId,
    updateKind: updateKind,
    group: group,
    notificationId: notificationId,
    title: title,
    body: body,
    source: source,
    hostKind: hostKind,
    contentType: contentType,
    notificationType: notificationType,
    sync: sync_?._toModel(),
  );
}

extension on gen_realtime_dto.DartRealtimeSyncHint {
  RealtimeSyncHint _toModel() => RealtimeSyncHint(
    domains: domains.map((domain) => domain._toModel()).toSet(),
    reason: reason,
    syncDirty: syncDirty,
    gapDetected: gapDetected,
    hasUnknownDomain: hasUnknownDomain,
  );
}

extension on gen_realtime_dto.DartSyncDomain {
  SyncDomain _toModel() => switch (this) {
    gen_realtime_dto.DartSyncDomain.message => SyncDomain.message,
    gen_realtime_dto.DartSyncDomain.profile => SyncDomain.profile,
    gen_realtime_dto.DartSyncDomain.agentInventory => SyncDomain.agentInventory,
    gen_realtime_dto.DartSyncDomain.agentStatus => SyncDomain.agentStatus,
    gen_realtime_dto.DartSyncDomain.deviceRegistry => SyncDomain.deviceRegistry,
  };
}

class SecureApi {
  SecureApi._(this._client);

  final AwikiImClient _client;

  DirectSecureConversation direct(String peer) =>
      DirectSecureConversation._(_client, peer);

  GroupSecureConversation group(String group) =>
      GroupSecureConversation._(_client, group);

  SecureOutboxApi get outbox => SecureOutboxApi._(_client);
}

class DirectSecureConversation {
  DirectSecureConversation._(this._client, this.peer);

  final AwikiImClient _client;
  final String peer;

  Future<DirectSecureStatus> status() async {
    _client._ensureNotDisposed();
    final status = await _mapNativeErrors(
      () => gen_secure.secureDirectStatus(client: _client._inner, peer: peer),
    );
    return status._toModel();
  }

  Future<DirectSecurePrepareResult> prepare() async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_secure.secureDirectPrepare(client: _client._inner, peer: peer),
    );
    return result._toModel();
  }

  Future<DirectSecureRepairResult> repair() async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_secure.secureDirectRepair(client: _client._inner, peer: peer),
    );
    return result._toModel();
  }
}

class GroupSecureConversation {
  GroupSecureConversation._(this._client, this.group);

  final AwikiImClient _client;
  final String group;

  Future<GroupSecureStatus> status() async {
    _client._ensureNotDisposed();
    final status = await _mapNativeErrors(
      () => gen_secure.secureGroupStatus(client: _client._inner, group: group),
    );
    return status._toModel();
  }

  Future<GroupSecurePrepareResult> prepare() async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_secure.secureGroupPrepare(client: _client._inner, group: group),
    );
    return result._toModel();
  }

  Future<GroupSecureRepairResult> repair() async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_secure.secureGroupRepair(client: _client._inner, group: group),
    );
    return result._toModel();
  }
}

class SecureOutboxApi {
  SecureOutboxApi._(this._client);

  final AwikiImClient _client;

  Future<List<SecureOutboxEntry>> listFailed() async {
    _client._ensureNotDisposed();
    final entries = await _mapNativeErrors(
      () => gen_secure.secureOutboxListFailed(client: _client._inner),
    );
    return entries.map((entry) => entry._toModel()).toList();
  }

  Future<SecureOutboxResult> retry(String outboxId) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_secure.secureOutboxRetry(
        client: _client._inner,
        outboxId: outboxId,
      ),
    );
    return result._toModel();
  }

  Future<SecureOutboxResult> drop(String outboxId) async {
    _client._ensureNotDisposed();
    final result = await _mapNativeErrors(
      () => gen_secure.secureOutboxDrop(
        client: _client._inner,
        outboxId: outboxId,
      ),
    );
    return result._toModel();
  }
}

extension on AwikiImCoreConfig {
  gen_config.DartImCoreConfig _toGen() => gen_config.DartImCoreConfig(
    serviceBaseUrl: serviceBaseUrl,
    didDomain: didDomain,
    clientVersionInfo: clientVersionInfo?._toGen(),
    userServiceEndpoint: userServiceEndpoint,
    messageServiceEndpoint: messageServiceEndpoint,
    mailServiceEndpoint: mailServiceEndpoint,
    anpServiceEndpoint: anpServiceEndpoint,
    anpServiceDid: anpServiceDid,
    transportPolicy: transportPolicy._toGen(),
  );
}

extension on AwikiClientVersionInfo {
  gen_config.DartClientVersionInfo _toGen() => gen_config.DartClientVersionInfo(
    product: product,
    release: release,
    version: version,
    build: build == null ? null : BigInt.from(build!),
  );
}

extension on AwikiImCorePaths {
  gen_config.DartImCorePaths _toGen() => gen_config.DartImCorePaths(
    identityRootDir: identityRootDir,
    registryPath: registryPath,
    defaultIdentityPath: defaultIdentityPath,
    sqlitePath: sqlitePath,
    cacheDir: cacheDir,
    tempDir: tempDir,
  );
}

extension on AwikiImCoreOpenOptions {
  gen_config.DartImCoreOpenOptions _toGen() => gen_config.DartImCoreOpenOptions(
    identitySecretStoragePolicy: identitySecretStoragePolicy._toGen(),
    identitySecretVault: identitySecretVault?._toGen(),
    multiDeviceDeviceRevokeEnabled: multiDeviceDeviceRevokeEnabled,
    multiDeviceDirectE2EeEnabled: multiDeviceDirectE2eeEnabled,
    multiDeviceGroupE2EeEnabled: multiDeviceGroupE2eeEnabled,
    multiDeviceHandleRecoveryEnabled: multiDeviceHandleRecoveryEnabled,
    multiDeviceAudience: multiDeviceAudience,
  );
}

extension on ImCoreSecretVaultOptions {
  gen_config.DartImCoreSecretVaultOptions _toGen() =>
      gen_config.DartImCoreSecretVaultOptions(
        rootKey: rootKey._toGen(),
        vaultDir: vaultDir,
        workspaceId: workspaceId,
        deviceId: deviceId,
      );
}

extension on DeviceVaultRootKey {
  gen_config.DartDeviceVaultRootKey _toGen() =>
      gen_config.DartDeviceVaultRootKey(bytes: bytes);
}

extension on gen_local_state_upgrade_dto.DartLocalStateUpgradeInspection {
  LocalStateUpgradeInspection _toModel() => LocalStateUpgradeInspection(
    eligibility: switch (eligibility) {
      gen_local_state_upgrade_dto
          .DartLocalStateUpgradeEligibility
          .notRequired =>
        LocalStateUpgradeEligibility.notRequired,
      gen_local_state_upgrade_dto.DartLocalStateUpgradeEligibility.required_ =>
        LocalStateUpgradeEligibility.required,
    },
    sourceSchemaVersion: sourceSchemaVersion,
    targetSchemaVersion: targetSchemaVersion,
  );
}

extension on gen_local_state_upgrade_dto.DartLocalStateUpgradeResult {
  LocalStateUpgradeResult _toModel() => LocalStateUpgradeResult(
    status: switch (status) {
      gen_local_state_upgrade_dto.DartLocalStateUpgradeStatus.notRequired =>
        LocalStateUpgradeStatus.notRequired,
      gen_local_state_upgrade_dto.DartLocalStateUpgradeStatus.completed =>
        LocalStateUpgradeStatus.completed,
    },
    sourceSchemaVersion: sourceSchemaVersion,
    targetSchemaVersion: targetSchemaVersion,
    migratedPersonas: migratedPersonas.toInt(),
    migratedConversations: migratedConversations.toInt(),
    unresolvedMessages: unresolvedMessages.toInt(),
    aliasCount: aliasCount.toInt(),
    backupAvailable: backupAvailable,
    aliasMappings: aliasMappings
        .map(
          (mapping) => LocalStateConversationAliasMapping(
            ownerIdentityId: mapping.ownerIdentityId,
            ownerDid: mapping.ownerDid,
            legacyConversationId: mapping.legacyConversationId,
            canonicalConversationId: mapping.canonicalConversationId,
          ),
        )
        .toList(growable: false),
  );
}

extension on gen_local_state_upgrade_dto.DartLocalStateRestoreResult {
  LocalStateRestoreResult _toModel() => LocalStateRestoreResult(
    restoredSchemaVersion: restoredSchemaVersion,
    targetSafetyCopyAvailable: targetSafetyCopyAvailable,
  );
}

extension on MessageTransportPolicy {
  gen_config.DartMessageTransportPolicy _toGen() => switch (this) {
    MessageTransportPolicy.auto => gen_config.DartMessageTransportPolicy.auto,
    MessageTransportPolicy.httpOnly =>
      gen_config.DartMessageTransportPolicy.httpOnly,
    MessageTransportPolicy.realtimePreferred =>
      gen_config.DartMessageTransportPolicy.realtimePreferred,
  };
}

extension on IdentitySecretStoragePolicy {
  gen_config.DartIdentitySecretStoragePolicy _toGen() => switch (this) {
    IdentitySecretStoragePolicy.fileCompat =>
      gen_config.DartIdentitySecretStoragePolicy.fileCompat,
    IdentitySecretStoragePolicy.vaultPreferred =>
      gen_config.DartIdentitySecretStoragePolicy.vaultPreferred,
    IdentitySecretStoragePolicy.vaultRequired =>
      gen_config.DartIdentitySecretStoragePolicy.vaultRequired,
  };
}

extension on gen_config.DartIdentitySecretStoragePolicy {
  IdentitySecretStoragePolicy _toModel() => switch (this) {
    gen_config.DartIdentitySecretStoragePolicy.fileCompat =>
      IdentitySecretStoragePolicy.fileCompat,
    gen_config.DartIdentitySecretStoragePolicy.vaultPreferred =>
      IdentitySecretStoragePolicy.vaultPreferred,
    gen_config.DartIdentitySecretStoragePolicy.vaultRequired =>
      IdentitySecretStoragePolicy.vaultRequired,
  };
}

extension on IdentitySelector {
  gen_identity.DartIdentitySelector _toGen() => switch (this) {
    DefaultIdentitySelector() =>
      const gen_identity.DartIdentitySelector.default_(),
    IdIdentitySelector(:final id) => gen_identity.DartIdentitySelector.id(
      id: id,
    ),
    DidIdentitySelector(:final did) => gen_identity.DartIdentitySelector.did(
      did: did,
    ),
    HandleIdentitySelector(:final handle) =>
      gen_identity.DartIdentitySelector.handle(handle: handle),
    LocalAliasIdentitySelector(:final alias) =>
      gen_identity.DartIdentitySelector.localAlias(alias: alias),
  };
}

extension on gen_identity.DartIdentitySummary {
  IdentitySummary _toModel() => IdentitySummary(
    id: id,
    did: did,
    handle: handle,
    displayName: displayName,
    localAlias: localAlias,
    deviceId: deviceId,
    isDefault: isDefault,
    readyForAuth: readyForAuth,
    readyForMessaging: readyForMessaging,
    missing: missing,
  );
}

extension on gen_identity.DartLegacyUpgradeStatus {
  LegacyUpgradeStatus _toModel() => switch (this) {
    gen_identity.DartLegacyUpgradeStatus_Idle() =>
      const LegacyUpgradeStatus.idle(),
    gen_identity.DartLegacyUpgradeStatus_Running() =>
      const LegacyUpgradeStatus.running(),
    gen_identity.DartLegacyUpgradeStatus_RetryRequired(
      :final identityId,
      :final code,
    ) =>
      LegacyUpgradeStatus.retryRequired(identityId: identityId, code: code),
    gen_identity.DartLegacyUpgradeStatus_Completed() =>
      const LegacyUpgradeStatus.completed(),
  };
}

extension on gen_identity.DartIdentityDeviceSummary {
  IdentityDeviceSummary _toModel() => IdentityDeviceSummary(
    identity: identity._toModel(),
    mode: switch (mode) {
      gen_identity.DartIdentityDeviceMode.legacy => IdentityDeviceMode.legacy,
      gen_identity.DartIdentityDeviceMode.vNext => IdentityDeviceMode.vNext,
    },
    protocolDeviceId: protocolDeviceId,
    role: switch (role) {
      gen_identity.DartIdentityDeviceRole.member => IdentityDeviceRole.member,
      gen_identity.DartIdentityDeviceRole.admin => IdentityDeviceRole.admin,
      null => null,
    },
    signingKeyId: signingKeyId,
    e2eeKeyId: e2EeKeyId,
    readiness: switch (readiness) {
      gen_identity.DartIdentityDeviceReadiness.legacy =>
        IdentityDeviceReadiness.legacy,
      gen_identity.DartIdentityDeviceReadiness.memberReady =>
        IdentityDeviceReadiness.memberReady,
      gen_identity.DartIdentityDeviceReadiness.adminAwaitingRoot =>
        IdentityDeviceReadiness.adminAwaitingRoot,
      gen_identity.DartIdentityDeviceReadiness.adminReady =>
        IdentityDeviceReadiness.adminReady,
      gen_identity.DartIdentityDeviceReadiness.blocked =>
        IdentityDeviceReadiness.blocked,
    },
    blockedReason: blockedReason,
  );
}

extension on gen_identity.DartDeviceJoinRole {
  DeviceJoinRole _toModel() => switch (this) {
    gen_identity.DartDeviceJoinRole.member => DeviceJoinRole.member,
    gen_identity.DartDeviceJoinRole.admin => DeviceJoinRole.admin,
  };
}

extension on gen_identity.DartDeviceJoinAuthorizationStatus {
  DeviceJoinAuthorizationStatus _toModel() => switch (this) {
    gen_identity.DartDeviceJoinAuthorizationStatus.active =>
      DeviceJoinAuthorizationStatus.active,
    gen_identity.DartDeviceJoinAuthorizationStatus.revoked =>
      DeviceJoinAuthorizationStatus.revoked,
  };
}

extension on gen_identity.DartDeviceJoinSessionSummary {
  DeviceJoinSessionSummary _toModel() => DeviceJoinSessionSummary(
    joinSessionId: joinSessionId,
    did: did,
    protocolDeviceId: protocolDeviceId,
    side: switch (side) {
      gen_identity.DartDeviceJoinSide.newDevice => DeviceJoinSide.newDevice,
      gen_identity.DartDeviceJoinSide.admin => DeviceJoinSide.admin,
    },
    phase: switch (phase) {
      gen_identity.DartDeviceJoinPhase.pending => DeviceJoinPhase.pending,
      gen_identity.DartDeviceJoinPhase.challengePrepared =>
        DeviceJoinPhase.challengePrepared,
      gen_identity.DartDeviceJoinPhase.responsePrepared =>
        DeviceJoinPhase.responsePrepared,
      gen_identity.DartDeviceJoinPhase.responseVerified =>
        DeviceJoinPhase.responseVerified,
      gen_identity.DartDeviceJoinPhase.approvalPrepared =>
        DeviceJoinPhase.approvalPrepared,
      gen_identity.DartDeviceJoinPhase.authorized => DeviceJoinPhase.authorized,
      gen_identity.DartDeviceJoinPhase.cancelled => DeviceJoinPhase.cancelled,
      gen_identity.DartDeviceJoinPhase.expired => DeviceJoinPhase.expired,
    },
    expiresAt: expiresAt,
  );
}

extension on gen_identity.DartDeviceJoinAuthorizedDeviceSummary {
  DeviceJoinAuthorizedDeviceSummary _toModel() =>
      DeviceJoinAuthorizedDeviceSummary(
        protocolDeviceId: protocolDeviceId,
        signingKeyId: signingKeyId,
        e2eeKeyId: e2EeKeyId,
        status: status._toModel(),
        role: role._toModel(),
        managementReady: managementReady,
        isCurrent: isCurrent,
      );
}

extension on gen_identity.DartDeviceJoinRequestNotice {
  DeviceJoinRequestNotice _toModel() => DeviceJoinRequestNotice(
    eventId: eventId,
    joinSessionId: joinSessionId,
    did: did,
    protocolDeviceId: protocolDeviceId,
    candidateKeyFingerprint: candidateKeyFingerprint,
    issuedAt: issuedAt,
    expiresAt: expiresAt,
    state: state._toModel(),
    claimedByCurrentDevice: claimedByCurrentDevice,
    canStartVerification: canStartVerification,
  );
}

extension on gen_identity.DartDeviceJoinRegistrySnapshot {
  DeviceJoinRegistrySnapshot _toModel() => DeviceJoinRegistrySnapshot(
    did: did,
    registryVersion: registryVersion,
    devices: devices.map((device) => device._toModel()).toList(),
  );
}

extension on gen_identity.DartDeviceRegistryAuthorizedDeviceSummary {
  DeviceRegistryAuthorizedDeviceSummary _toModel() =>
      DeviceRegistryAuthorizedDeviceSummary(
        protocolDeviceId: protocolDeviceId,
        signingKeyId: signingKeyId,
        e2eeKeyId: e2EeKeyId,
        status: status._toModel(),
        role: role._toModel(),
        managementReady: managementReady,
        isCurrent: isCurrent,
        authGeneration: authGeneration,
      );
}

extension on gen_identity.DartDeviceJoinRemoteState {
  DeviceJoinRemoteState _toModel() => switch (this) {
    gen_identity.DartDeviceJoinRemoteState.pending =>
      DeviceJoinRemoteState.pending,
    gen_identity.DartDeviceJoinRemoteState.challengeSent =>
      DeviceJoinRemoteState.challengeSent,
    gen_identity.DartDeviceJoinRemoteState.responseVerified =>
      DeviceJoinRemoteState.responseVerified,
    gen_identity.DartDeviceJoinRemoteState.consumed =>
      DeviceJoinRemoteState.consumed,
    gen_identity.DartDeviceJoinRemoteState.cancelled =>
      DeviceJoinRemoteState.cancelled,
    gen_identity.DartDeviceJoinRemoteState.rejected =>
      DeviceJoinRemoteState.rejected,
    gen_identity.DartDeviceJoinRemoteState.expired =>
      DeviceJoinRemoteState.expired,
  };
}

extension on gen_identity.DartDeviceJoinProgress {
  DeviceJoinProgress _toModel() => DeviceJoinProgress(
    session: session._toModel(),
    remoteState: remoteState._toModel(),
    sas: sas,
    authorizedDevice: authorizedDevice?._toModel(),
  );
}

extension on DeviceJoinRejectReason {
  gen_identity.DartDeviceJoinRejectReason _toGen() => switch (this) {
    DeviceJoinRejectReason.userRejected =>
      gen_identity.DartDeviceJoinRejectReason.userRejected,
    DeviceJoinRejectReason.sasMismatch =>
      gen_identity.DartDeviceJoinRejectReason.sasMismatch,
  };
}

extension on gen_identity.DartDeviceJoinApprovalPrompt {
  DeviceJoinApprovalPrompt _toModel() => DeviceJoinApprovalPrompt(
    approvalHandle: approvalHandle,
    joinSessionId: joinSessionId,
    sas: sas,
    expiresAt: expiresAt,
  );
}

extension on gen_identity.DartIdentitySecretStorageBackend {
  IdentitySecretStorageBackend _toModel() => switch (this) {
    gen_identity.DartIdentitySecretStorageBackend.fileCompat =>
      IdentitySecretStorageBackend.fileCompat,
    gen_identity.DartIdentitySecretStorageBackend.vault =>
      IdentitySecretStorageBackend.vault,
  };
}

extension on gen_identity.DartIdentityVaultStatus {
  IdentityVaultStatus _toModel() => IdentityVaultStatus(
    identity: identity._toModel(),
    storagePolicy: storagePolicy._toModel(),
    selectedBackend: selectedBackend._toModel(),
    vaultAvailable: vaultAvailable,
    vaultMetadataPresent: vaultMetadataPresent,
    vaultMetadataVerified: vaultMetadataVerified,
    workspaceId: workspaceId,
    deviceId: deviceId,
    plaintextCompatRetained: plaintextCompatRetained,
    missing: missing,
    warnings: warnings,
  );
}

extension on gen_identity.DartIdentityVaultMigrationReport {
  IdentityVaultMigrationReport _toModel() => IdentityVaultMigrationReport(
    identity: identity._toModel(),
    status: status._toModel(),
    migrated: migrated,
    verified: verified,
    plaintextCompatRetained: plaintextCompatRetained,
    warnings: warnings,
  );
}

extension on gen_identity.DartIdentityVaultVerificationReport {
  IdentityVaultVerificationReport _toModel() => IdentityVaultVerificationReport(
    identity: identity._toModel(),
    status: status._toModel(),
    verified: verified,
    warnings: warnings,
  );
}

extension on InitialProfile {
  gen_identity.DartInitialProfile _toGen() => gen_identity.DartInitialProfile(
    displayName: displayName,
    avatarUrl: avatarUrl,
  );
}

extension on gen_identity.DartDefaultIdentityChange {
  DefaultIdentityChange _toModel() => DefaultIdentityChange(
    previous: previous?._toModel(),
    next: next._toModel(),
    requiresDefaultIdentityWrite: requiresDefaultIdentityWrite,
    warnings: warnings,
  );
}

extension on gen_identity.DartDeleteLocalIdentityResult {
  DeleteLocalIdentityResult _toModel() => DeleteLocalIdentityResult(
    deleted: deleted._toModel(),
    wasDefault: wasDefault,
    nextDefault: nextDefault?._toModel(),
    warnings: warnings,
  );
}

extension on gen_identity.DartDaemonSubkeyPrivatePackage {
  DaemonSubkeyPrivatePackage _toModel() => DaemonSubkeyPrivatePackage(
    schema: schema,
    userDid: userDid,
    verificationMethod: verificationMethod,
    keyType: keyType,
    keyAlgorithm: keyAlgorithm,
    publicKeyMultibase: publicKeyMultibase,
    privateKeyEncoding: privateKeyEncoding,
    privateKeyPem: privateKeyPem,
    privateKeyMultibase: privateKeyMultibase,
  );
}

extension on gen_identity.DartDaemonSubkeyAuthorizationRevokeResult {
  DaemonSubkeyAuthorizationRevokeResult _toModel() =>
      DaemonSubkeyAuthorizationRevokeResult(
        userDid: userDid,
        verificationMethod: verificationMethod,
        updated: updated,
      );
}

extension on gen_identity.DartHandleRegistrationResult {
  HandleRegistrationResult _toModel() => HandleRegistrationResult(
    identity: identity?._toModel(),
    accountId: accountId,
    handle: handle,
    method: method,
    state: state,
    joinRequired: joinRequired?._toModel(),
    defaultIdentityChange: defaultIdentityChange?._toModel(),
    warnings: warnings,
  );
}

extension on gen_identity.DartActiveSyncAccountBinding {
  ActiveSyncAccountBinding _toModel() => ActiveSyncAccountBinding(
    ownerIdentityId: ownerIdentityId,
    accountId: accountId,
    currentDid: currentDid,
    protocolDeviceId: protocolDeviceId,
    identityGeneration: identityGeneration,
    deviceAuthGeneration: deviceAuthGeneration,
  );
}

extension on gen_identity.DartLegacyRegistryEpochAdoptionAuthority {
  LegacyRegistryEpochAdoptionAuthority _toModel() =>
      LegacyRegistryEpochAdoptionAuthority(
        ownerIdentityId: ownerIdentityId,
        accountUserId: accountUserId,
        currentDid: currentDid,
        bindingGeneration: bindingGeneration,
        protocolDeviceId: protocolDeviceId,
        deviceAuthGeneration: deviceAuthGeneration,
        provenanceId: provenanceId,
      );
}

extension on gen_identity.DartHandleRecoveryOtpResult {
  HandleRecoveryOtpResult _toModel() => HandleRecoveryOtpResult(
    ownerIdentityId: ownerIdentityId,
    fullHandle: fullHandle,
    operationId: operationId,
    accepted: accepted,
    retryAfterSeconds: retryAfterSeconds,
    retryAt: DateTime.parse(retryAt),
  );
}

extension on gen_identity.DartHandleRecoveryImpact {
  HandleRecoveryImpact _toModel() => HandleRecoveryImpact(
    localOrdinaryDataWillMigrate: localOrdinaryDataWillMigrate,
    otherDevicesMustRejoin: otherDevicesMustRejoin,
    unsupportedE2eeGroupCount: unsupportedE2EeGroupCount,
    unsupportedDidOnlyGroupCount: unsupportedDidOnlyGroupCount,
  );
}

extension on gen_identity.DartHandleRecoveryResetReference {
  HandleRecoveryRegistryEpochReset _toModel() =>
      HandleRecoveryRegistryEpochReset(
        accountUserId: accountUserId,
        ownerIdentityId: ownerIdentityId,
        handle: handle,
        previousDid: previousDid,
        currentDid: currentDid,
        bindingGeneration: bindingGeneration,
        sourceKind: switch (sourceKind) {
          gen_identity.DartHandleRecoveryTransitionSourceKind.initiator =>
            HandleRecoveryTransitionSourceKind.initiator,
          gen_identity.DartHandleRecoveryTransitionSourceKind.joinedDevice =>
            HandleRecoveryTransitionSourceKind.joinedDevice,
        },
        sourceId: sourceId,
      );
}

extension on gen_identity.DartHandleRecoveryProgress {
  HandleRecoveryProgress _toModel() => HandleRecoveryProgress(
    operationId: operationId,
    ownerIdentityId: ownerIdentityId,
    accountUserId: accountUserId,
    fullHandle: fullHandle,
    localPreviousDid: localPreviousDid,
    currentDid: currentDid,
    bindingGeneration: bindingGeneration,
    stateRootFingerprint: stateRootFingerprint,
    phase: switch (phase) {
      gen_identity.DartHandleRecoveryPhase.awaitingFactor =>
        HandleRecoveryPhase.awaitingFactor,
      gen_identity.DartHandleRecoveryPhase.readyToCommit =>
        HandleRecoveryPhase.readyToCommit,
      gen_identity.DartHandleRecoveryPhase.remoteOutcomeUnknown =>
        HandleRecoveryPhase.remoteOutcomeUnknown,
      gen_identity.DartHandleRecoveryPhase.remoteCommitted =>
        HandleRecoveryPhase.remoteCommitted,
      gen_identity.DartHandleRecoveryPhase.identityTransitionPending =>
        HandleRecoveryPhase.identityTransitionPending,
      gen_identity.DartHandleRecoveryPhase.applied =>
        HandleRecoveryPhase.applied,
      gen_identity.DartHandleRecoveryPhase.quarantinedKeyUnavailable =>
        HandleRecoveryPhase.quarantinedKeyUnavailable,
    },
    impact: impact._toModel(),
    registryEpochReset: resetReference?._toModel(),
    failureCode: switch (failureCode) {
      gen_identity.DartHandleRecoveryErrorCode.factorRetryRequired =>
        HandleRecoveryFailureCode.factorRetryRequired,
      gen_identity.DartHandleRecoveryErrorCode.resultAbsent =>
        HandleRecoveryFailureCode.resultAbsent,
      gen_identity.DartHandleRecoveryErrorCode.outcomeUnknown =>
        HandleRecoveryFailureCode.outcomeUnknown,
      gen_identity.DartHandleRecoveryErrorCode.localKeyUnavailable =>
        HandleRecoveryFailureCode.localKeyUnavailable,
      gen_identity.DartHandleRecoveryErrorCode.localTransitionPending =>
        HandleRecoveryFailureCode.localTransitionPending,
      gen_identity.DartHandleRecoveryErrorCode.localMigrationUnsupported =>
        HandleRecoveryFailureCode.localMigrationUnsupported,
      gen_identity.DartHandleRecoveryErrorCode.unknownEpoch =>
        HandleRecoveryFailureCode.unknownEpoch,
      null => null,
    },
  );
}

extension on gen_identity.DartHandleRecoveryOperationSummary {
  HandleRecoveryOperationSummary _toModel() => HandleRecoveryOperationSummary(
    operationId: operationId,
    ownerIdentityId: ownerIdentityId,
    accountUserId: accountUserId,
    fullHandle: fullHandle,
    lifecycleClass: switch (lifecycleClass) {
      gen_identity.DartHandleRecoveryOperationLifecycle.preCommit =>
        HandleRecoveryOperationLifecycle.preCommit,
      gen_identity.DartHandleRecoveryOperationLifecycle.remoteUnresolved =>
        HandleRecoveryOperationLifecycle.remoteUnresolved,
      gen_identity.DartHandleRecoveryOperationLifecycle.remoteCommitted =>
        HandleRecoveryOperationLifecycle.remoteCommitted,
      gen_identity
          .DartHandleRecoveryOperationLifecycle
          .localTransitionPending =>
        HandleRecoveryOperationLifecycle.localTransitionPending,
      gen_identity.DartHandleRecoveryOperationLifecycle.applied =>
        HandleRecoveryOperationLifecycle.applied,
      gen_identity.DartHandleRecoveryOperationLifecycle.discardedPreAttempt =>
        HandleRecoveryOperationLifecycle.discardedPreAttempt,
      gen_identity
          .DartHandleRecoveryOperationLifecycle
          .quarantinedKeyUnavailable =>
        HandleRecoveryOperationLifecycle.quarantinedKeyUnavailable,
      gen_identity
          .DartHandleRecoveryOperationLifecycle
          .supersededByStateChange =>
        HandleRecoveryOperationLifecycle.supersededByStateChange,
      gen_identity.DartHandleRecoveryOperationLifecycle.failedTerminal =>
        HandleRecoveryOperationLifecycle.failedTerminal,
    },
    commitAttempted: commitAttempted,
    keyState: switch (keyState) {
      gen_identity.DartHandleRecoveryKeyState.available =>
        HandleRecoveryKeyState.available,
      gen_identity.DartHandleRecoveryKeyState.temporarilyLocked =>
        HandleRecoveryKeyState.temporarilyLocked,
      gen_identity.DartHandleRecoveryKeyState.permanentlyUnavailable =>
        HandleRecoveryKeyState.permanentlyUnavailable,
      gen_identity.DartHandleRecoveryKeyState.destroyedPreAttempt =>
        HandleRecoveryKeyState.destroyedPreAttempt,
    },
    intentHash: intentHash,
    stateRootFingerprint: stateRootFingerprint,
    supersededByOperationId: supersededByOperationId,
    lastErrorCode: lastErrorCode,
    createdAt: DateTime.parse(createdAt),
    updatedAt: DateTime.parse(updatedAt),
  );
}

extension on gen_identity.DartHandleRecoveryAccountEpochReceipt {
  HandleRecoveryAccountEpochReceipt _toModel() =>
      HandleRecoveryAccountEpochReceipt(
        receiptSchemaVersion: receiptSchemaVersion,
        sourceKind: switch (sourceKind) {
          gen_identity.DartHandleRecoveryTransitionSourceKind.initiator =>
            HandleRecoveryTransitionSourceKind.initiator,
          gen_identity.DartHandleRecoveryTransitionSourceKind.joinedDevice =>
            HandleRecoveryTransitionSourceKind.joinedDevice,
        },
        sourceId: sourceId,
        accountUserId: accountUserId,
        ownerIdentityId: ownerIdentityId,
        fullHandle: fullHandle,
        localPreviousDid: localPreviousDid,
        currentDid: currentDid,
        bindingGeneration: bindingGeneration,
        currentDeviceId: currentDeviceId,
        deviceAuthGeneration: deviceAuthGeneration.toInt(),
        registryVersion: registryVersion.toInt(),
        stateRootFingerprint: stateRootFingerprint,
        appliedAt: DateTime.parse(appliedAt),
        metadataJson: metadataJson,
      );
}

extension on gen_identity.DartAuthorizedJoinActivationProgress {
  AuthorizedJoinActivationProgress _toModel() =>
      AuthorizedJoinActivationProgress(
        join: join._toModel(),
        registryEpochReset: resetReference?._toModel(),
      );
}

extension on gen_identity.DartHandleRegistrationJoinRequiredPreparation {
  HandleRegistrationJoinRequiredPreparation _toModel() =>
      HandleRegistrationJoinRequiredPreparation(
        preparationId: preparationId,
        mode: switch (mode) {
          gen_identity.DartHandleRegistrationJoinMode.ordinary =>
            HandleRegistrationJoinMode.ordinary,
          gen_identity.DartHandleRegistrationJoinMode.handleRecoveryRebind =>
            HandleRegistrationJoinMode.handleRecoveryRebind,
        },
        requiresUserPresence: requiresUserPresence,
        expectedDid: expectedDid,
        fullHandle: fullHandle,
      );
}

extension on AuthScope {
  gen_auth_dto.DartAuthScope _toGen() => switch (this) {
    AuthScope.userProfile => gen_auth_dto.DartAuthScope.userProfile,
    AuthScope.messaging => gen_auth_dto.DartAuthScope.messaging,
    AuthScope.groupMessaging => gen_auth_dto.DartAuthScope.groupMessaging,
  };
}

extension on gen_auth_dto.DartAuthScope {
  AuthScope _toModel() => switch (this) {
    gen_auth_dto.DartAuthScope.userProfile => AuthScope.userProfile,
    gen_auth_dto.DartAuthScope.messaging => AuthScope.messaging,
    gen_auth_dto.DartAuthScope.groupMessaging => AuthScope.groupMessaging,
  };
}

extension on gen_auth_dto.DartAuthStatus {
  AuthStatus _toModel() => AuthStatus(
    subject: subject,
    hasSession: hasSession,
    expiresAt: expiresAt,
    needsRefresh: needsRefresh,
    warnings: warnings,
  );
}

extension on gen_auth_dto.DartSessionBundle {
  SessionBundle _toModel() => SessionBundle(
    subject: subject,
    scope: scope._toModel(),
    expiresAt: expiresAt,
    refreshed: refreshed,
    bearerToken: bearerToken,
  );
}

extension on gen_auth_dto.DartSessionUpdate {
  SessionUpdate _toModel() => SessionUpdate(
    subject: subject,
    previousExpiresAt: previousExpiresAt,
    newExpiresAt: newExpiresAt,
    refreshed: refreshed,
    bearerToken: bearerToken,
  );
}

extension on IdentitySubject {
  gen_directory_dto.DartIdentitySubject _toGen() => switch (this) {
    DidIdentitySubject(:final did) => gen_directory_dto.DartIdentitySubject.did(
      did: did,
    ),
    HandleIdentitySubject(:final handle) =>
      gen_directory_dto.DartIdentitySubject.handle(handle: handle),
    AnyIdentitySubject(:final value) =>
      gen_directory_dto.DartIdentitySubject.any(value: value),
  };
}

extension on gen_directory_dto.DartDirectoryResolution {
  DirectoryResolution _toModel() => DirectoryResolution(
    input: input,
    did: did,
    handle: handle,
    conversationId: conversationId,
    profile: profile?._toModel(),
    warnings: warnings,
  );
}

extension on gen_directory_dto.DartRelationStatus {
  RelationStatus _toModel() => RelationStatus(
    peer: peer,
    did: did,
    isFollowing: isFollowing,
    isFollower: isFollower,
    isFriend: isFriend,
    isBlocked: isBlocked,
    isBlockedBy: isBlockedBy,
    isContact: isContact,
    messaged: messaged,
    relationship: relationship,
    displayName: displayName,
    warnings: warnings,
  );
}

extension on gen_directory_dto.DartDisplayProfile {
  DisplayProfile _toModel() => DisplayProfile(
    did: did,
    handle: handle,
    displayName: displayName,
    avatarUri: avatarUri,
    avatarUrl: avatarUrl,
    profileUri: profileUri,
    subjectType: subjectType,
    cacheHit: cacheHit,
    isStale: isStale,
    legacyFallback: legacyFallback,
    warnings: warnings,
  );
}

extension on gen_directory_dto.DartRelationshipListItem {
  RelationshipListItem _toModel() => RelationshipListItem(
    did: did,
    handle: handle,
    displayName: displayName,
    avatarUri: avatarUri,
    avatarUrl: avatarUrl,
    profileUri: profileUri,
    subjectType: subjectType,
    relationship: relationship,
    createdAt: createdAt,
    warnings: warnings,
  );
}

extension on gen_directory_dto.DartRelationshipPage {
  RelationshipPage _toModel() => RelationshipPage(
    items: items.map((item) => item._toModel()).toList(),
    nextCursor: nextCursor,
    hasMore: hasMore,
  );
}

extension on ProfilePatch {
  gen_profile_dto.DartProfilePatch _toGen() => gen_profile_dto.DartProfilePatch(
    displayName: displayName,
    bio: bio,
    tags: tags,
    markdown: markdown,
    avatarUri: avatarUri,
    avatarUrl: avatarUrl,
  );
}

extension on gen_profile_dto.DartUserProfile {
  UserProfile _toModel() => UserProfile(
    subject: subject,
    handle: handle,
    fullHandle: fullHandle,
    displayName: displayName,
    bio: bio,
    description: description,
    tags: tags,
    markdown: markdown,
    avatarUri: avatarUri,
    avatarUrl: avatarUrl,
    profileUri: profileUri,
    subjectType: subjectType,
    agentKind: agentKind,
    agentCapabilities: agentCapabilities,
    updatedAt: updatedAt,
    profileVersion: profileVersion,
    versionId: versionId,
    ttl: ttl?.toInt(),
  );
}

extension on MessageTarget {
  gen_message.DartMessageTarget _toGen() => switch (this) {
    DirectMessageTarget(:final peer) => gen_message.DartMessageTarget.direct(
      peer: peer,
    ),
    GroupMessageTarget(:final group) => gen_message.DartMessageTarget.group(
      group: group,
    ),
  };
}

extension on ThreadRef {
  gen_message.DartThreadRef _toGen() => switch (this) {
    DirectThreadRef(:final peer) => gen_message.DartThreadRef.direct(
      peer: peer,
    ),
    GroupThreadRef(:final group) => gen_message.DartThreadRef.group(
      group: group,
    ),
    MessageThreadRef(:final threadId) => gen_message.DartThreadRef.thread(
      threadId: threadId,
    ),
  };
}

extension on MessageSecurityMode {
  gen_message.DartMessageSecurityMode _toGen() => switch (this) {
    MessageSecurityMode.defaultPlain =>
      gen_message.DartMessageSecurityMode.defaultPlain,
    MessageSecurityMode.plain => gen_message.DartMessageSecurityMode.plain,
    MessageSecurityMode.e2eeRequired =>
      gen_message.DartMessageSecurityMode.e2EeRequired,
    MessageSecurityMode.secureDirect =>
      gen_message.DartMessageSecurityMode.secureDirect,
    MessageSecurityMode.groupE2ee =>
      gen_message.DartMessageSecurityMode.groupE2Ee,
  };
}

extension on SendTextRequest {
  gen_message.DartSendTextRequest _toGen() => gen_message.DartSendTextRequest(
    target: target._toGen(),
    text: text,
    markdown: markdown,
    security: security._toGen(),
    clientMessageId: clientMessageId,
    idempotencyKey: idempotencyKey,
    waitForFinalAcceptance: waitForFinalAcceptance,
    delegatedSigning: delegatedSigning?._toGen(),
  );
}

extension on SendPayloadRequest {
  gen_message.DartSendPayloadRequest _toGen() =>
      gen_message.DartSendPayloadRequest(
        target: target._toGen(),
        payloadJson: payloadJson,
        security: security._toGen(),
        clientMessageId: clientMessageId,
        idempotencyKey: idempotencyKey,
        waitForFinalAcceptance: waitForFinalAcceptance,
        delegatedSigning: delegatedSigning?._toGen(),
      );
}

extension on SendConversationTextRequest {
  gen_message.DartSendConversationTextRequest _toGen() =>
      gen_message.DartSendConversationTextRequest(
        conversation: conversation._toGen(),
        text: text,
        markdown: markdown,
        security: security._toGen(),
        clientMessageId: clientMessageId,
        idempotencyKey: idempotencyKey,
        waitForFinalAcceptance: waitForFinalAcceptance,
        delegatedSigning: delegatedSigning?._toGen(),
      );
}

extension on SendConversationPayloadRequest {
  gen_message.DartSendConversationPayloadRequest _toGen() =>
      gen_message.DartSendConversationPayloadRequest(
        conversation: conversation._toGen(),
        payloadJson: payloadJson,
        security: security._toGen(),
        clientMessageId: clientMessageId,
        idempotencyKey: idempotencyKey,
        waitForFinalAcceptance: waitForFinalAcceptance,
        delegatedSigning: delegatedSigning?._toGen(),
      );
}

extension on DelegatedSigningOptions {
  gen_message.DartDelegatedSigningOptions _toGen() =>
      gen_message.DartDelegatedSigningOptions(
        logicalSenderDid: logicalSenderDid,
        signingVerificationMethod: signingVerificationMethod,
        signingKeyRef: signingKeyRef,
        actorAgentDid: actorAgentDid,
      );
}

extension on InboxHistoryOptions {
  gen_message.DartInboxHistoryOptions _toGen() =>
      gen_message.DartInboxHistoryOptions(
        inboxOwnerDid: inboxOwnerDid,
        inboxAuthVerificationMethod: inboxAuthVerificationMethod,
        inboxAuthKeyRef: inboxAuthKeyRef,
        inboxAuth: inboxAuth?._toGen(),
      );
}

extension on InboxAuth {
  gen_message.DartInboxAuth _toGen() => switch (this) {
    ScopedInboxTokenAuth(:final token) =>
      gen_message.DartInboxAuth.scopedInboxToken(token: token._toGen()),
  };
}

extension on ScopedInboxToken {
  gen_message.DartScopedInboxToken _toGen() =>
      gen_message.DartScopedInboxToken(token: token);
}

extension on SyncDeltaRequest {
  gen_message.DartSyncDeltaRequest _toGen() => gen_message.DartSyncDeltaRequest(
    limit: limit,
    deviceId: deviceId,
    reason: reason,
  );
}

extension on ConversationReadRef {
  gen_message.DartConversationReadRef _toGen() =>
      gen_message.DartConversationReadRef(conversationId: conversationId);
}

extension on SyncThreadAfterRequest {
  gen_message.DartSyncThreadAfterRequest _toGen() =>
      gen_message.DartSyncThreadAfterRequest(
        thread: thread._toGen(),
        afterServerSeq: afterServerSeq,
        limit: limit,
      );
}

extension on SyncConversationAfterRequest {
  gen_message.DartSyncConversationAfterRequest _toGen() =>
      gen_message.DartSyncConversationAfterRequest(
        conversation: conversation._toGen(),
        afterServerSeq: afterServerSeq,
        limit: limit,
      );
}

extension on AttachmentInput {
  gen_attachment.DartAttachmentInput _toGen() => switch (this) {
    LocalFileAttachmentInput(:final path) =>
      gen_attachment.DartAttachmentInput.localFile(path: path),
    BytesAttachmentInput(:final filename, :final mimeType, :final bytes) =>
      gen_attachment.DartAttachmentInput.bytes(
        filename: filename,
        mimeType: mimeType,
        bytes: Uint8List.fromList(bytes),
      ),
  };
}

extension on AttachmentSendRequest {
  gen_attachment.DartAttachmentSendRequest _toGen() =>
      gen_attachment.DartAttachmentSendRequest(
        target: target._toGen(),
        input: input._toGen(),
        caption: caption,
        mentionPayloadJson: mentionPayloadJson,
        mimeType: mimeType,
        filename: filename,
        security: security._toGen(),
        idempotencyKey: idempotencyKey,
        waitForFinalAcceptance: waitForFinalAcceptance,
      );
}

extension on SendConversationAttachmentRequest {
  gen_attachment.DartSendConversationAttachmentRequest _toGen() =>
      gen_attachment.DartSendConversationAttachmentRequest(
        conversation: conversation._toGen(),
        input: input._toGen(),
        caption: caption,
        mentionPayloadJson: mentionPayloadJson,
        mimeType: mimeType,
        filename: filename,
        security: security._toGen(),
        clientMessageId: clientMessageId,
        idempotencyKey: idempotencyKey,
        waitForFinalAcceptance: waitForFinalAcceptance,
      );
}

extension on AttachmentDestination {
  gen_attachment.DartAttachmentDestination _toGen() => switch (this) {
    LocalFileAttachmentDestination(:final path) =>
      gen_attachment.DartAttachmentDestination.localFile(path: path),
    MemoryAttachmentDestination() =>
      const gen_attachment.DartAttachmentDestination.memory(),
  };
}

extension on DownloadAttachmentRequest {
  gen_attachment.DartDownloadAttachmentRequest _toGen() =>
      gen_attachment.DartDownloadAttachmentRequest(
        thread: thread._toGen(),
        messageId: messageId,
        attachmentId: attachmentId,
        destination: destination._toGen(),
        overwrite: overwrite,
      );
}

extension on gen_attachment.DartUploadedAttachment {
  UploadedAttachment _toModel() => UploadedAttachment(
    attachmentId: attachmentId,
    filename: filename,
    mimeType: mimeType,
    sizeBytes: sizeBytes.toInt(),
    size: size,
    digestB64u: digestB64U,
    objectUri: objectUri,
    objectEncryptionMode: objectEncryptionMode,
    plaintextSizeBytes: plaintextSizeBytes?.toInt(),
  );
}

extension on gen_attachment.DartAttachmentSendResult {
  AttachmentSendResult _toModel() => AttachmentSendResult(
    message: message._toModel(),
    targetKind: targetKind,
    targetDid: targetDid,
    attachment: attachment._toModel(),
    manifestJson: manifestJson,
  );
}

extension on gen_attachment.DartDownloadedAttachmentDestination {
  DownloadedAttachmentDestination _toModel() => switch (this) {
    gen_attachment.DartDownloadedAttachmentDestination_LocalFile(:final path) =>
      DownloadedAttachmentLocalFile(path),
    gen_attachment.DartDownloadedAttachmentDestination_Memory(:final bytes) =>
      DownloadedAttachmentMemory(bytes),
  };
}

extension on gen_attachment.DartDownloadedAttachment {
  DownloadedAttachment _toModel() => DownloadedAttachment(
    attachmentId: attachmentId,
    filename: filename,
    mimeType: mimeType,
    sizeBytes: sizeBytes?.toInt(),
    destination: destination._toModel(),
    warnings: warnings,
  );
}

extension on gen_email_dto.DartEmailAttribute {
  EmailAttribute _toModel() => EmailAttribute(key: key, value: value);
}

extension on gen_email_dto.DartEmailAccount {
  EmailAccount _toModel() => EmailAccount(
    mailboxAddress: mailboxAddress,
    displayName: displayName,
    status: status,
    attributes: attributes.map((attribute) => attribute._toModel()).toList(),
  );
}

extension on gen_email_dto.DartEmailMessageSummary {
  EmailMessageSummary _toModel() => EmailMessageSummary(
    id: id,
    folder: folder,
    from: from,
    to: to,
    cc: cc,
    subject: subject,
    preview: preview,
    receivedAt: receivedAt,
    sentAt: sentAt,
    unread: unread,
    hasAttachments: hasAttachments,
    attachmentCount: attachmentCount,
    attributes: attributes.map((attribute) => attribute._toModel()).toList(),
  );
}

extension on gen_email_dto.DartEmailMessageSummaryPage {
  EmailMessageSummaryPage _toModel() => EmailMessageSummaryPage(
    items: items.map((message) => message._toModel()).toList(),
    nextCursor: nextCursor,
    hasMore: hasMore,
  );
}

extension on gen_email_dto.DartEmailAttachmentMetadata {
  EmailAttachmentMetadata _toModel() => EmailAttachmentMetadata(
    index: index,
    filename: filename,
    contentType: contentType,
    size: size?.toInt(),
  );
}

extension on gen_email_dto.DartEmailMessage {
  EmailMessage _toModel() => EmailMessage(
    summary: summary._toModel(),
    bodyText: bodyText,
    bodyHtml: bodyHtml,
    attachments: attachments
        .map((attachment) => attachment._toModel())
        .toList(),
  );
}

extension on gen_email_dto.DartEmailMarkReadResult {
  EmailMarkReadResult _toModel() => EmailMarkReadResult(updated: updated);
}

extension on SendEmailRequest {
  gen_email_dto.DartSendEmailRequest _toGen() =>
      gen_email_dto.DartSendEmailRequest(
        to: to,
        cc: cc,
        subject: subject,
        bodyText: bodyText,
        bodyHtml: bodyHtml,
      );
}

extension on gen_email_dto.DartSendEmailResult {
  SendEmailResult _toModel() => SendEmailResult(
    accepted: accepted,
    messageId: messageId,
    warnings: warnings,
  );
}

extension on gen_email_dto.DartEmailAttachmentContent {
  EmailAttachmentContent _toModel() => EmailAttachmentContent(
    messageId: messageId,
    attachmentIndex: attachmentIndex,
    filename: filename,
    contentType: contentType,
    size: size?.toInt(),
    bytes: bytes,
  );
}

extension on gen_email_dto.DartEmailNotification {
  EmailNotification _toModel() => EmailNotification(
    id: id,
    mailboxAddress: mailboxAddress,
    fromAddr: fromAddr,
    subject: subject,
    preview: preview,
    hasAttachments: hasAttachments,
    receivedAt: receivedAt,
    attributes: attributes.map((attribute) => attribute._toModel()).toList(),
  );
}

extension on gen_email_dto.DartEmailNotificationPage {
  EmailNotificationPage _toModel() => EmailNotificationPage(
    items: items.map((notification) => notification._toModel()).toList(),
    nextCursor: nextCursor,
    hasMore: hasMore,
  );
}

extension on gen_message.DartMessageDirection {
  MessageDirection _toModel() => switch (this) {
    gen_message.DartMessageDirection.outgoing => MessageDirection.outgoing,
    gen_message.DartMessageDirection.incoming => MessageDirection.incoming,
    gen_message.DartMessageDirection.unknown => MessageDirection.unknown,
  };
}

extension on gen_message.DartMessageBodyView {
  MessageBodyView _toModel() => MessageBodyView(
    text: text,
    kind: kind,
    payloadJson: payloadJson,
    unsupportedContentType: unsupportedContentType,
  );
}

extension on gen_message.DartMessageMetadataAttribute {
  MessageMetadataAttribute _toModel() =>
      MessageMetadataAttribute(key: key, value: value);
}

extension on gen_message.DartConversationAliasSource {
  ConversationAliasSource _toModel() => switch (this) {
    gen_message.DartConversationAliasSource.legacyDirectDid =>
      ConversationAliasSource.legacyDirectDid,
    gen_message.DartConversationAliasSource.oldFlutterSortedDirect =>
      ConversationAliasSource.oldFlutterSortedDirect,
    gen_message.DartConversationAliasSource.peerScopeStorage =>
      ConversationAliasSource.peerScopeStorage,
    gen_message.DartConversationAliasSource.groupStorage =>
      ConversationAliasSource.groupStorage,
    gen_message.DartConversationAliasSource.threadStorage =>
      ConversationAliasSource.threadStorage,
    gen_message.DartConversationAliasSource.unknown =>
      ConversationAliasSource.unknown,
  };
}

extension on gen_message.DartConversationIdentityScope {
  ConversationIdentityScope _toModel() => switch (this) {
    gen_message.DartConversationIdentityScope.direct =>
      ConversationIdentityScope.direct,
    gen_message.DartConversationIdentityScope.group =>
      ConversationIdentityScope.group,
    gen_message.DartConversationIdentityScope.thread =>
      ConversationIdentityScope.thread,
    gen_message.DartConversationIdentityScope.mail =>
      ConversationIdentityScope.mail,
    gen_message.DartConversationIdentityScope.unknown =>
      ConversationIdentityScope.unknown,
  };
}

extension on gen_message.DartConversationMigrationState {
  ConversationMigrationState _toModel() => switch (this) {
    gen_message.DartConversationMigrationState.canonical =>
      ConversationMigrationState.canonical,
    gen_message.DartConversationMigrationState.aliasResolved =>
      ConversationMigrationState.aliasResolved,
    gen_message.DartConversationMigrationState.legacyInput =>
      ConversationMigrationState.legacyInput,
    gen_message.DartConversationMigrationState.unknown =>
      ConversationMigrationState.unknown,
  };
}

extension on gen_message.DartConversationStorageThreadRef {
  ConversationStorageThreadRef _toModel() =>
      ConversationStorageThreadRef(kind: kind, id: id);
}

extension on gen_message.DartConversationAlias {
  ConversationAlias _toModel() =>
      ConversationAlias(kind: kind, id: id, source: source._toModel());
}

extension on gen_message.DartConversationIdentity {
  ConversationIdentity _toModel() => ConversationIdentity(
    conversationId: conversationId,
    canonicalThreadKind: canonicalThreadKind,
    canonicalThreadId: canonicalThreadId,
    storageThreadRef: storageThreadRef._toModel(),
    aliases: aliases.map((alias) => alias._toModel()).toList(),
    identityScope: identityScope._toModel(),
    migrationState: migrationState._toModel(),
  );
}

extension on gen_message.DartMessageMetadata {
  MessageMetadata _toModel() => MessageMetadata(
    operationId: operationId,
    deliveryState: deliveryState,
    sendState: sendState,
    retryable: retryable,
    retryAction: retryAction,
    serverSequence: serverSequence,
    contentType: contentType,
    conversationIdentity: conversationIdentity?._toModel(),
    attributes: attributes.map((attribute) => attribute._toModel()).toList(),
  );
}

extension on gen_message.DartMessage {
  Message _toModel() => Message(
    id: id,
    conversationId: conversationId,
    senderPeerPersonaId: senderPeerPersonaId,
    senderDidSnapshot: senderDidSnapshot,
    threadKind: threadKind,
    threadId: threadId,
    direction: direction._toModel(),
    sender: sender,
    receiver: receiver,
    group: group,
    body: body._toModel(),
    sentAt: sentAt,
    receivedAt: receivedAt,
    metadata: metadata._toModel(),
  );
}

extension on gen_message.DartMessagePage {
  MessagePage _toModel() => MessagePage(
    items: items.map((message) => message._toModel()).toList(),
    nextCursor: nextCursor,
    hasMore: hasMore,
  );
}

extension on gen_message.DartConversation {
  Conversation _toModel() => Conversation(
    conversationId: conversationId,
    peerPersonaId: peerPersonaId,
    canonicalGroupDid: canonicalGroupDid,
    resolutionState: resolutionState._toModel(),
    threadKind: threadKind,
    threadId: threadId,
    conversationIdentity: conversationIdentity?._toModel(),
    title: title,
    participants: participants,
    lastMessage: lastMessage?._toModel(),
    unreadCount: unreadCount,
    unreadMentionCount: unreadMentionCount,
    firstUnreadMentionMessageId: firstUnreadMentionMessageId,
    messageCount: messageCount,
    lastMessageAt: lastMessageAt,
    activityAt: activityAt,
  );
}

extension on gen_message.DartConversationResolutionState {
  ConversationResolutionState _toModel() => switch (this) {
    gen_message.DartConversationResolutionState.resolved =>
      ConversationResolutionState.resolved,
    gen_message.DartConversationResolutionState.legacyUnresolved =>
      ConversationResolutionState.legacyUnresolved,
    gen_message.DartConversationResolutionState.blockedConflict =>
      ConversationResolutionState.blockedConflict,
  };
}

extension on gen_message.DartConversationPage {
  ConversationPage _toModel() => ConversationPage(
    items: items.map((conversation) => conversation._toModel()).toList(),
    nextCursor: nextCursor,
    hasMore: hasMore,
  );
}

extension on gen_message.DartConversationListSnapshot {
  ConversationListSnapshot _toModel() => ConversationListSnapshot(
    formatVersion: formatVersion,
    imSchemaVersion: imSchemaVersion.toInt(),
    ownerIdentityId: ownerIdentityId,
    ownerDid: ownerDid,
    generatedAtMs: generatedAtMs.toInt(),
    summaryVersion: summaryVersion,
    unreadTotal: unreadTotal,
    items: items.map((item) => item._toModel()).toList(),
  );
}

extension on gen_message.DartConversationStorePatch {
  ConversationStorePatch _toModel() => map(
    reset: (value) => ConversationStorePatch(
      kind: ConversationStorePatchKind.reset,
      ownerIdentityId: value.ownerIdentityId,
      ownerDid: value.ownerDid,
      version: value.version.toInt(),
      unreadTotal: value.unreadTotal,
      items: value.items.map((item) => item._toModel()).toList(),
    ),
    upsert: (value) => ConversationStorePatch(
      kind: ConversationStorePatchKind.upsert,
      ownerIdentityId: value.ownerIdentityId,
      ownerDid: value.ownerDid,
      version: value.version.toInt(),
      unreadTotal: value.unreadTotal,
      item: value.item._toModel(),
      index: value.index,
    ),
    remove: (value) => ConversationStorePatch(
      kind: ConversationStorePatchKind.remove,
      ownerIdentityId: value.ownerIdentityId,
      ownerDid: value.ownerDid,
      version: value.version.toInt(),
      unreadTotal: value.unreadTotal,
      conversationId: value.conversationId,
    ),
    reorder: (value) => ConversationStorePatch(
      kind: ConversationStorePatchKind.reorder,
      ownerIdentityId: value.ownerIdentityId,
      ownerDid: value.ownerDid,
      version: value.version.toInt(),
      unreadTotal: value.unreadTotal,
      conversationId: value.conversationId,
      index: value.index,
    ),
    repairRequired: (value) => ConversationStorePatch(
      kind: ConversationStorePatchKind.repairRequired,
      ownerIdentityId: value.ownerIdentityId,
      ownerDid: value.ownerDid,
      version: value.version.toInt(),
      unreadTotal: value.unreadTotal,
      reason: value.reason,
    ),
  );
}

extension on gen_message.DartThreadMessageStorePatch {
  ThreadMessageStorePatch _toModel() => map(
    reset: (value) => ThreadMessageStorePatch(
      kind: ThreadMessageStorePatchKind.reset,
      ownerIdentityId: value.ownerIdentityId,
      ownerDid: value.ownerDid,
      version: value.version.toInt(),
      threadKind: value.threadKind,
      threadId: value.threadId,
      conversationIdentity: value.conversationIdentity?._toModel(),
      items: value.items.map((message) => message._toModel()).toList(),
    ),
    upsert: (value) => ThreadMessageStorePatch(
      kind: ThreadMessageStorePatchKind.upsert,
      ownerIdentityId: value.ownerIdentityId,
      ownerDid: value.ownerDid,
      version: value.version.toInt(),
      threadKind: value.threadKind,
      threadId: value.threadId,
      conversationIdentity: value.conversationIdentity?._toModel(),
      message: value.message._toModel(),
      index: value.index,
    ),
    remove: (value) => ThreadMessageStorePatch(
      kind: ThreadMessageStorePatchKind.remove,
      ownerIdentityId: value.ownerIdentityId,
      ownerDid: value.ownerDid,
      version: value.version.toInt(),
      threadKind: value.threadKind,
      threadId: value.threadId,
      conversationIdentity: value.conversationIdentity?._toModel(),
      messageId: value.messageId,
    ),
    repairRequired: (value) => ThreadMessageStorePatch(
      kind: ThreadMessageStorePatchKind.repairRequired,
      ownerIdentityId: value.ownerIdentityId,
      ownerDid: value.ownerDid,
      version: value.version.toInt(),
      threadKind: value.threadKind,
      threadId: value.threadId,
      conversationIdentity: value.conversationIdentity?._toModel(),
      reason: value.reason,
    ),
  );
}

extension on gen_message.DartConversationSnapshotItem {
  ConversationSnapshotItem _toModel() => ConversationSnapshotItem(
    conversationId: conversationId,
    peerPersonaId: peerPersonaId,
    canonicalGroupDid: canonicalGroupDid,
    resolutionState: resolutionState._toModel(),
    threadKind: threadKind,
    threadId: threadId,
    title: title,
    conversationIdentity: conversationIdentity?._toModel(),
    participants: participants,
    lastMessage: lastMessage?._toModel(),
    unreadCount: unreadCount,
    unreadMentionCount: unreadMentionCount,
    firstUnreadMentionMessageId: firstUnreadMentionMessageId,
    messageCount: messageCount,
    lastMessageAt: lastMessageAt,
    activityAt: activityAt,
  );
}

extension on gen_message.DartConversationSnapshotMessage {
  ConversationSnapshotMessage _toModel() => ConversationSnapshotMessage(
    id: id,
    threadKind: threadKind,
    threadId: threadId,
    conversationIdentity: conversationIdentity?._toModel(),
    direction: direction,
    sender: sender,
    receiver: receiver,
    group: group,
    body: body._toModel(),
    sentAt: sentAt,
    receivedAt: receivedAt,
    serverSequence: serverSequence?.toInt(),
    contentType: contentType,
    attributes: attributes.map((attribute) => attribute._toModel()).toList(),
  );
}

extension on gen_message.DartConversationSnapshotMessageBody {
  ConversationSnapshotMessageBody _toModel() => ConversationSnapshotMessageBody(
    text: text,
    kind: kind,
    payloadJson: payloadJson,
    unsupportedContentType: unsupportedContentType,
  );
}

extension on gen_message.DartSendMessageResult {
  SendMessageResult _toModel() => SendMessageResult(
    message: message._toModel(),
    deliveryState: deliveryState,
    warnings: warnings,
  );
}

extension on gen_message.DartMarkReadResult {
  MarkReadResult _toModel() => MarkReadResult(
    updatedCount: updatedCount,
    messageIds: messageIds,
    warnings: warnings,
  );
}

extension on gen_message.DartMarkThreadReadResult {
  MarkThreadReadResult _toModel() => MarkThreadReadResult(
    updatedCount: updatedCount,
    remoteAcknowledged: remoteAcknowledged,
    partial: partial,
    fallbackUsed: fallbackUsed,
    pendingRemoteAck: pendingRemoteAck,
    effectiveWatermark: effectiveWatermark?._toModel(),
    legacyMessageIds: legacyMessageIds,
    warnings: warnings,
  );
}

extension on ReadWatermark {
  gen_message.DartReadWatermark _toGen() => gen_message.DartReadWatermark(
    lastReadMessageId: lastReadMessageId,
    lastReadThreadSeq: lastReadThreadSeq,
    readAt: readAt?.toUtc().toIso8601String(),
  );
}

extension on gen_message.DartReadWatermark {
  ReadWatermark _toModel() => ReadWatermark(
    lastReadMessageId: lastReadMessageId,
    lastReadThreadSeq: lastReadThreadSeq,
    readAt: readAt == null ? null : DateTime.tryParse(readAt!),
  );
}

extension on gen_message.DartSyncDeltaResult {
  SyncDeltaResult _toModel() => SyncDeltaResult(
    eventsApplied: eventsApplied,
    pagesFetched: pagesFetched,
    lastAppliedEventSeq: lastAppliedEventSeq,
    hasMore: hasMore,
    snapshotRequired: snapshotRequired,
    retentionFloorEventSeq: retentionFloorEventSeq,
    warnings: warnings,
  );
}

extension on gen_message.DartMessageSyncStatus {
  MessageSyncStatus _toModel() => switch (this) {
    gen_message.DartMessageSyncStatus.idle => MessageSyncStatus.idle,
    gen_message.DartMessageSyncStatus.changed => MessageSyncStatus.changed,
    gen_message.DartMessageSyncStatus.recoveryRequired =>
      MessageSyncStatus.recoveryRequired,
    gen_message.DartMessageSyncStatus.retryableFailure =>
      MessageSyncStatus.retryableFailure,
    gen_message.DartMessageSyncStatus.authRevoked =>
      MessageSyncStatus.authRevoked,
  };
}

extension on gen_message.DartCommittedMessageSource {
  CommittedMessageSource _toModel() => switch (this) {
    gen_message.DartCommittedMessageSource.liveDelta =>
      CommittedMessageSource.liveDelta,
  };
}

extension on gen_message.DartCommittedIncomingMessage {
  CommittedIncomingMessage _toModel() => CommittedIncomingMessage(
    eventId: eventId,
    logicalMessageId: logicalMessageId,
    source: source._toModel(),
    direction: direction._toModel(),
    message: message._toModel(),
  );
}

extension on gen_message.DartMessageSyncOutcome {
  MessageSyncOutcome _toModel() => MessageSyncOutcome(
    status: status._toModel(),
    eventsApplied: eventsApplied,
    pagesFetched: pagesFetched,
    messagesHydrated: messagesHydrated,
    duplicatesSkipped: duplicatesSkipped,
    changedConversationIds: changedConversationIds,
    committedIncomingMessages: committedIncomingMessages
        .map((message) => message._toModel())
        .toList(),
    errorCode: errorCode,
    warnings: warnings,
  );
}

extension on gen_message.DartMessageSyncMode {
  MessageSyncMode _toModel() => switch (this) {
    gen_message.DartMessageSyncMode.uninitialized =>
      MessageSyncMode.uninitialized,
    gen_message.DartMessageSyncMode.idle => MessageSyncMode.idle,
    gen_message.DartMessageSyncMode.recovering => MessageSyncMode.recovering,
    gen_message.DartMessageSyncMode.retryable => MessageSyncMode.retryable,
    gen_message.DartMessageSyncMode.blocked => MessageSyncMode.blocked,
  };
}

extension on gen_message.DartMessageSyncDirtyDomain {
  MessageSyncDirtyDomain _toModel() => switch (this) {
    gen_message.DartMessageSyncDirtyDomain.messages =>
      MessageSyncDirtyDomain.messages,
    gen_message.DartMessageSyncDirtyDomain.readState =>
      MessageSyncDirtyDomain.readState,
  };
}

extension on gen_message.DartMessageSyncRetryState {
  MessageSyncRetryState _toModel() => switch (this) {
    gen_message.DartMessageSyncRetryState.none => MessageSyncRetryState.none,
    gen_message.DartMessageSyncRetryState.pending =>
      MessageSyncRetryState.pending,
    gen_message.DartMessageSyncRetryState.inFlight =>
      MessageSyncRetryState.inFlight,
    gen_message.DartMessageSyncRetryState.scheduled =>
      MessageSyncRetryState.scheduled,
    gen_message.DartMessageSyncRetryState.permanentFailure =>
      MessageSyncRetryState.permanentFailure,
  };
}

extension on gen_message.DartMessageSyncDiagnostics {
  MessageSyncDiagnostics _toModel() => MessageSyncDiagnostics(
    lastSuccessAt: lastSuccessAt,
    mode: mode._toModel(),
    pendingMutationCount: pendingMutationCount,
    dirtyDomains: dirtyDomains.map((domain) => domain._toModel()).toList(),
    retryState: retryState._toModel(),
    nextRetryAt: nextRetryAt,
  );
}

extension on gen_message.DartSyncThreadAfterResult {
  SyncThreadAfterResult _toModel() => SyncThreadAfterResult(
    messages: messages.map((message) => message._toModel()).toList(),
    nextAfterServerSeq: nextAfterServerSeq,
    hasMore: hasMore,
    warnings: warnings,
  );
}

extension on CreateGroupRequest {
  gen_group_dto.DartCreateGroupRequest _toGen() =>
      gen_group_dto.DartCreateGroupRequest(
        name: name,
        identityMode: identityMode._toGen(),
        identityHandle: identityHandle,
        description: description,
        avatarUri: avatarUri,
        discoverability: discoverability?.value,
        admissionMode: admissionMode?.value,
        messageSecurityProfile: messageSecurityProfile?.value,
        e2Ee: e2ee,
        slug: slug,
        goal: goal,
        rules: rules,
        messagePrompt: messagePrompt,
        docUrl: docUrl,
        attachmentsAllowed: attachmentsAllowed,
        maxMembers: maxMembers?.value.toString(),
        memberMaxMessages: memberMaxMessages,
        memberMaxTotalChars: memberMaxTotalChars,
      );
}

extension on JoinGroupRequest {
  gen_group_dto.DartJoinGroupRequest _toGen() =>
      gen_group_dto.DartJoinGroupRequest(
        groupDid: groupDid,
        identityMode: identityMode._toGen(),
        identityHandle: identityHandle,
      );
}

extension on GroupIdentityMode {
  gen_group_dto.DartGroupIdentityMode _toGen() => switch (this) {
    GroupIdentityMode.handle => gen_group_dto.DartGroupIdentityMode.handle,
    GroupIdentityMode.didOnly => gen_group_dto.DartGroupIdentityMode.didOnly,
  };
}

extension on gen_group_dto.DartGroupSummary {
  GroupSummary _toModel() => GroupSummary(
    conversationId: conversationId,
    id: id,
    did: did,
    name: name,
    displayName: displayName,
    avatarUri: avatarUri,
    myRole: myRole,
    membershipStatus: membershipStatus,
    memberCount: memberCount,
    lastMessageAt: lastMessageAt,
  );
}

extension on gen_group_dto.DartGroupSnapshot {
  GroupSnapshot _toModel() => GroupSnapshot(
    conversationId: conversationId,
    id: id,
    did: did,
    name: name,
    displayName: displayName,
    avatarUri: avatarUri,
    description: description,
    myRole: myRole,
    membershipStatus: membershipStatus,
    memberCount: memberCount,
    lastMessageAt: lastMessageAt,
  );
}

extension on gen_group_dto.DartGroupMember {
  GroupMember _toModel() => GroupMember(
    membershipId: membershipId,
    peerPersonaId: peerPersonaId,
    did: did,
    credentialDid: credentialDid,
    handle: handle,
    role: role,
    status: status,
    joinedAt: joinedAt,
    subjectType: subjectType,
  );
}

extension on gen_group_dto.DartGroupReadResult {
  GroupReadResult _toModel() => GroupReadResult(
    group: group?._toModel(),
    groups: groups.map((group) => group._toModel()).toList(),
    members: members.map((member) => member._toModel()).toList(),
    messages: messages._toModel(),
    total: total,
    nextCursor: nextCursor,
    hasMore: hasMore,
    pageGroupDid: pageGroupDid,
    groupStateVersion: groupStateVersion,
    source: source,
    warnings: warnings,
  );
}

extension on gen_group_dto.DartGroupRebindRecoveryItem {
  GroupRebindRecoveryItem _toModel() => GroupRebindRecoveryItem(
    groupDid: groupDid,
    layer: layer,
    phase: phase,
    blocked: blocked,
  );
}

extension on gen_group_dto.DartGroupRebindRecoverySummary {
  GroupRebindRecoverySummary _toModel() => GroupRebindRecoverySummary(
    processed: processed,
    completed: completed,
    pending: pending,
    blocked: blocked,
    sendPausedGroupDids: sendPausedGroupDids,
    items: items.map((item) => item._toModel()).toList(),
    warnings: warnings,
  );
}

extension on gen_realtime_dto.DartRealtimeCapability {
  RealtimeCapability _toModel() => RealtimeCapability(
    statusSupported: statusSupported,
    connectSupported: connectSupported,
    runnerExposed: runnerExposed,
    reason: reason,
  );
}

extension on gen_realtime_dto.DartRealtimeStatus {
  RealtimeStatus _toModel() => RealtimeStatus(
    connected: connected,
    state: state,
    subscriptions: subscriptions,
    lastError: lastError,
    warnings: warnings,
  );
}

extension on gen_secure_dto.DartDirectSecureState {
  DirectSecureState _toModel() => switch (this) {
    gen_secure_dto.DartDirectSecureState.ready => DirectSecureState.ready,
    gen_secure_dto.DartDirectSecureState.preparing =>
      DirectSecureState.preparing,
    gen_secure_dto.DartDirectSecureState.waitingForPeer =>
      DirectSecureState.waitingForPeer,
    gen_secure_dto.DartDirectSecureState.needsRepair =>
      DirectSecureState.needsRepair,
    gen_secure_dto.DartDirectSecureState.unavailable =>
      DirectSecureState.unavailable,
    gen_secure_dto.DartDirectSecureState.unknown => DirectSecureState.unknown,
  };
}

extension on gen_secure_dto.DartDirectSecureStatus {
  DirectSecureStatus _toModel() => DirectSecureStatus(
    peer: peer,
    resolvedPeer: resolvedPeer,
    state: state._toModel(),
    canSendSecure: canSendSecure,
    pendingOutboxCount: pendingOutboxCount,
    problem: problem?._toModel(),
    warnings: warnings,
  );
}

extension on gen_secure_dto.DartDirectSecurePrepareResult {
  DirectSecurePrepareResult _toModel() => DirectSecurePrepareResult(
    peer: peer,
    state: state._toModel(),
    canSendSecure: canSendSecure,
    warnings: warnings,
  );
}

extension on gen_secure_dto.DartDirectSecureRepairResult {
  DirectSecureRepairResult _toModel() => DirectSecureRepairResult(
    peer: peer,
    state: state._toModel(),
    repaired: repaired,
    problem: problem?._toModel(),
    warnings: warnings,
  );
}

extension on gen_secure_dto.DartGroupSecureState {
  GroupSecureState _toModel() => switch (this) {
    gen_secure_dto.DartGroupSecureState.ready => GroupSecureState.ready,
    gen_secure_dto.DartGroupSecureState.syncing => GroupSecureState.syncing,
    gen_secure_dto.DartGroupSecureState.needsRepair =>
      GroupSecureState.needsRepair,
    gen_secure_dto.DartGroupSecureState.waitingForMembershipUpdate =>
      GroupSecureState.waitingForMembershipUpdate,
    gen_secure_dto.DartGroupSecureState.missingLocalState =>
      GroupSecureState.missingLocalState,
    gen_secure_dto.DartGroupSecureState.unavailable =>
      GroupSecureState.unavailable,
    gen_secure_dto.DartGroupSecureState.unknown => GroupSecureState.unknown,
  };
}

extension on gen_secure_dto.DartGroupSecureLocalReadiness {
  GroupSecureLocalReadiness _toModel() => GroupSecureLocalReadiness(
    hasLocalState: hasLocalState,
    hasActiveMembership: hasActiveMembership,
  );
}

extension on gen_secure_dto.DartGroupSecurePendingWork {
  GroupSecurePendingWork _toModel() => GroupSecurePendingWork(
    pendingNotices: pendingNotices,
    pendingCommits: pendingCommits,
  );
}

extension on gen_secure_dto.DartGroupSecureStatus {
  GroupSecureStatus _toModel() => GroupSecureStatus(
    group: group,
    state: state._toModel(),
    canSendSecure: canSendSecure,
    localReadiness: localReadiness._toModel(),
    pendingWork: pendingWork._toModel(),
    problem: problem?._toModel(),
    warnings: warnings,
  );
}

extension on gen_secure_dto.DartGroupSecurePrepareResult {
  GroupSecurePrepareResult _toModel() => GroupSecurePrepareResult(
    group: group,
    state: state._toModel(),
    canSendSecure: canSendSecure,
    warnings: warnings,
  );
}

extension on gen_secure_dto.DartGroupSecureRepairResult {
  GroupSecureRepairResult _toModel() => GroupSecureRepairResult(
    group: group,
    state: state._toModel(),
    repaired: repaired,
    addedDevices: addedDevices,
    removedDevices: removedDevices,
    remainingDevices: remainingDevices,
    problem: problem?._toModel(),
    warnings: warnings,
  );
}

extension on gen_secure_dto.DartSecureOutboxStatus {
  SecureOutboxStatus _toModel() => switch (this) {
    gen_secure_dto.DartSecureOutboxStatus.queued => SecureOutboxStatus.queued,
    gen_secure_dto.DartSecureOutboxStatus.sending => SecureOutboxStatus.sending,
    gen_secure_dto.DartSecureOutboxStatus.failed => SecureOutboxStatus.failed,
    gen_secure_dto.DartSecureOutboxStatus.sent => SecureOutboxStatus.sent,
    gen_secure_dto.DartSecureOutboxStatus.dropped => SecureOutboxStatus.dropped,
  };
}

extension on gen_secure_dto.DartSecureOutboxEntry {
  SecureOutboxEntry _toModel() => SecureOutboxEntry(
    id: id,
    target: target._toModel(),
    messageKind: messageKind,
    status: status._toModel(),
    attemptCount: attemptCount,
    lastError: lastError?._toModel(),
    createdAt: createdAt,
    updatedAt: updatedAt,
  );
}

extension on gen_secure_dto.DartSecureOutboxResult {
  SecureOutboxResult _toModel() => SecureOutboxResult(
    id: id,
    status: status._toModel(),
    delivery: delivery?._toModel(),
    warnings: warnings,
  );
}

extension on gen_secure_dto.DartSecureDelivery {
  SecureDelivery _toModel() =>
      SecureDelivery(messageId: messageId, state: state);
}

extension on gen_secure_dto.DartSecureProblem {
  SecureProblem _toModel() => SecureProblem(
    code: code._toModel(),
    message: message,
    retryable: retryable,
  );
}

extension on gen_secure_dto.DartSecureProblemCode {
  SecureProblemCode _toModel() => switch (this) {
    gen_secure_dto.DartSecureProblemCode.identityNotReady =>
      SecureProblemCode.identityNotReady,
    gen_secure_dto.DartSecureProblemCode.peerNotFound =>
      SecureProblemCode.peerNotFound,
    gen_secure_dto.DartSecureProblemCode.peerKeysUnavailable =>
      SecureProblemCode.peerKeysUnavailable,
    gen_secure_dto.DartSecureProblemCode.sessionNeedsRepair =>
      SecureProblemCode.sessionNeedsRepair,
    gen_secure_dto.DartSecureProblemCode.groupStateUnavailable =>
      SecureProblemCode.groupStateUnavailable,
    gen_secure_dto.DartSecureProblemCode.localStateUnavailable =>
      SecureProblemCode.localStateUnavailable,
    gen_secure_dto.DartSecureProblemCode.transportUnavailable =>
      SecureProblemCode.transportUnavailable,
    gen_secure_dto.DartSecureProblemCode.unsupported =>
      SecureProblemCode.unsupported,
    gen_secure_dto.DartSecureProblemCode.unknown => SecureProblemCode.unknown,
  };
}

extension on gen_message.DartMessageTarget {
  MessageTarget _toModel() => when(
    direct: (peer) => MessageTarget.direct(peer),
    group: (group) => MessageTarget.group(group),
  );
}

extension on gen_identity.DartRootKeyTransferPreparation {
  RootKeyTransferPreparation _toModel() => RootKeyTransferPreparation(
    authorizationHandle: _NativeRootKeyTransferAuthorizationHandle(
      authorizationHandle,
    ),
    recipient: recipient._toModel(),
    expiresAt: expiresAt,
  );
}

extension on gen_identity.DartRootKeyTransferRecipientSummary {
  RootKeyTransferRecipientSummary _toModel() => RootKeyTransferRecipientSummary(
    did: did,
    deviceId: deviceId,
    signingKeyId: signingKeyId,
    e2eeKeyId: e2EeKeyId,
    registryVersion: registryVersion.toInt(),
  );
}

extension on gen_identity.DartRootKeyTransferSendResult {
  RootKeyTransferSendResult _toModel() => RootKeyTransferSendResult(
    did: did,
    senderDeviceId: senderDeviceId,
    recipientDeviceId: recipientDeviceId,
    messageId: messageId,
    acceptedAt: acceptedAt,
  );
}

extension on gen_identity.DartDeviceRevokeResult {
  DeviceRevokeResult _toModel() => DeviceRevokeResult(
    did: did,
    targetDeviceId: targetDeviceId,
    status: switch (status) {
      gen_identity.DartDeviceRevokeStatus.revoked => DeviceRevokeStatus.revoked,
    },
  );
}

Future<T> _mapRootKeyTransferErrors<T>(Future<T> Function() action) async {
  try {
    return await action();
  } on gen_identity.DartRootKeyTransferError catch (error) {
    throw RootKeyTransferException(
      code: error.code,
      retryable: error.retryable,
    );
  }
}
