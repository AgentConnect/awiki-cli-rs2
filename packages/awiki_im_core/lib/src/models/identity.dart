import 'dart:convert';
import 'dart:typed_data';

import 'config.dart';
import 'error.dart';

/// Single-use, write-only account-verification grant for device Join.
class DeviceJoinAccountVerificationGrant {
  factory DeviceJoinAccountVerificationGrant.fromToken(String token) {
    if (token.trim().isEmpty) {
      throw ArgumentError('account verification grant must not be empty');
    }
    return DeviceJoinAccountVerificationGrant._(
      Uint8List.fromList(utf8.encode(token)),
    );
  }

  DeviceJoinAccountVerificationGrant._(this._bytes);

  Uint8List? _bytes;

  /// Consumed by the native Join boundary. A second call always fails.
  Uint8List takeBytes() {
    final bytes = _bytes;
    if (bytes == null) {
      throw StateError(
        'DeviceJoinAccountVerificationGrant was already consumed',
      );
    }
    _bytes = null;
    return bytes;
  }

  @override
  String toString() => 'DeviceJoinAccountVerificationGrant(<redacted>)';
}

sealed class IdentitySelector {
  const IdentitySelector();

  const factory IdentitySelector.defaultIdentity() = DefaultIdentitySelector;
  const factory IdentitySelector.id(String id) = IdIdentitySelector;
  const factory IdentitySelector.did(String did) = DidIdentitySelector;
  const factory IdentitySelector.handle(String handle) = HandleIdentitySelector;
  const factory IdentitySelector.localAlias(String alias) =
      LocalAliasIdentitySelector;
}

class DefaultIdentitySelector extends IdentitySelector {
  const DefaultIdentitySelector();
}

class IdIdentitySelector extends IdentitySelector {
  const IdIdentitySelector(this.id);
  final String id;
}

class DidIdentitySelector extends IdentitySelector {
  const DidIdentitySelector(this.did);
  final String did;
}

class HandleIdentitySelector extends IdentitySelector {
  const HandleIdentitySelector(this.handle);
  final String handle;
}

class LocalAliasIdentitySelector extends IdentitySelector {
  const LocalAliasIdentitySelector(this.alias);
  final String alias;
}

class IdentitySummary {
  const IdentitySummary({
    required this.id,
    required this.did,
    this.handle,
    this.displayName,
    this.localAlias,
    this.deviceId,
    required this.isDefault,
    required this.readyForAuth,
    required this.readyForMessaging,
    this.missing = const [],
  });

  final String id;
  final String did;
  final String? handle;
  final String? displayName;
  final String? localAlias;
  final String? deviceId;
  final bool isDefault;
  final bool readyForAuth;
  final bool readyForMessaging;
  final List<String> missing;
}

class ActiveSyncAccountBinding {
  const ActiveSyncAccountBinding({
    required this.ownerIdentityId,
    required this.accountId,
    required this.currentDid,
    required this.protocolDeviceId,
    required this.identityGeneration,
    required this.deviceAuthGeneration,
  });

  final String ownerIdentityId;
  final String accountId;
  final String currentDid;
  final String protocolDeviceId;
  final String identityGeneration;
  final String deviceAuthGeneration;
}

class LegacyRegistryEpochAdoptionAuthority {
  const LegacyRegistryEpochAdoptionAuthority({
    required this.ownerIdentityId,
    required this.accountUserId,
    required this.currentDid,
    required this.bindingGeneration,
    required this.protocolDeviceId,
    required this.deviceAuthGeneration,
    required this.provenanceId,
  });

  final String ownerIdentityId;
  final String accountUserId;
  final String currentDid;
  final String bindingGeneration;
  final String protocolDeviceId;
  final String deviceAuthGeneration;
  final String provenanceId;
}

enum HandleRecoveryPhase {
  awaitingFactor,
  readyToCommit,
  remoteOutcomeUnknown,
  prepared,
  remoteCommitPending,
  remoteCommitted,
  identityTransitionPending,
  identitySwitched,
  completed,
  applied,
  quarantinedKeyUnavailable,
  blocked,
}

enum HandleRecoveryTransitionSourceKind { initiator, joinedDevice }

class HandleRecoveryImpact {
  const HandleRecoveryImpact({
    required this.localOrdinaryDataWillMigrate,
    required this.otherDevicesMustRejoin,
    required this.unsupportedE2eeGroupCount,
    required this.unsupportedDidOnlyGroupCount,
  });

  final bool localOrdinaryDataWillMigrate;
  final bool otherDevicesMustRejoin;
  final int unsupportedE2eeGroupCount;
  final int unsupportedDidOnlyGroupCount;
}

class HandleRecoveryRegistryEpochReset {
  const HandleRecoveryRegistryEpochReset({
    required this.accountUserId,
    required this.ownerIdentityId,
    required this.handle,
    required this.previousDid,
    required this.currentDid,
    required this.bindingGeneration,
    required this.sourceKind,
    required this.sourceId,
  });

  final String accountUserId;
  final String ownerIdentityId;
  final String handle;
  final String previousDid;
  final String currentDid;
  final String bindingGeneration;
  final HandleRecoveryTransitionSourceKind sourceKind;
  final String sourceId;
}

class HandleRecoveryProgress {
  const HandleRecoveryProgress({
    required this.operationId,
    required this.ownerIdentityId,
    this.accountUserId,
    required this.fullHandle,
    this.localPreviousDid,
    required this.currentDid,
    this.bindingGeneration,
    this.stateRootFingerprint,
    required this.phase,
    required this.impact,
    this.registryEpochReset,
    this.failureCode,
  });

  final String operationId;
  final String ownerIdentityId;
  final String? accountUserId;
  final String fullHandle;
  final String? localPreviousDid;
  final String currentDid;
  final String? bindingGeneration;
  final String? stateRootFingerprint;
  final HandleRecoveryPhase phase;
  final HandleRecoveryImpact impact;
  final HandleRecoveryRegistryEpochReset? registryEpochReset;
  final HandleRecoveryFailureCode? failureCode;
}

class HandleRecoveryOtpResult {
  const HandleRecoveryOtpResult({
    required this.fullHandle,
    required this.operationId,
    required this.accepted,
    required this.retryAfterSeconds,
    required this.retryAt,
  });

  final String fullHandle;
  final String operationId;
  final bool accepted;
  final int retryAfterSeconds;
  final DateTime retryAt;
}

enum HandleRecoveryOperationLifecycle {
  preCommit,
  remoteUnresolved,
  remoteCommitted,
  localTransitionPending,
  applied,
  discardedPreAttempt,
  quarantinedKeyUnavailable,
  supersededByStateChange,
  failedTerminal,
}

enum HandleRecoveryKeyState {
  available,
  temporarilyLocked,
  permanentlyUnavailable,
  destroyedPreAttempt,
}

class HandleRecoveryOperationSummary {
  const HandleRecoveryOperationSummary({
    required this.operationId,
    required this.ownerIdentityId,
    this.accountUserId,
    required this.fullHandle,
    required this.lifecycleClass,
    required this.commitAttempted,
    required this.keyState,
    this.intentHash,
    this.stateRootFingerprint,
    this.supersededByOperationId,
    this.lastErrorCode,
    required this.createdAt,
    required this.updatedAt,
  });

  final String operationId;
  final String ownerIdentityId;
  final String? accountUserId;
  final String fullHandle;
  final HandleRecoveryOperationLifecycle lifecycleClass;
  final bool commitAttempted;
  final HandleRecoveryKeyState keyState;
  final String? intentHash;
  final String? stateRootFingerprint;
  final String? supersededByOperationId;
  final String? lastErrorCode;
  final DateTime createdAt;
  final DateTime updatedAt;
}

class HandleRecoveryAccountEpochReceipt {
  const HandleRecoveryAccountEpochReceipt({
    required this.receiptSchemaVersion,
    required this.sourceKind,
    required this.sourceId,
    required this.accountUserId,
    required this.ownerIdentityId,
    required this.fullHandle,
    required this.localPreviousDid,
    required this.currentDid,
    required this.bindingGeneration,
    required this.currentDeviceId,
    required this.deviceAuthGeneration,
    required this.registryVersion,
    required this.stateRootFingerprint,
    required this.appliedAt,
    required this.metadataJson,
  });

  final String receiptSchemaVersion;
  final HandleRecoveryTransitionSourceKind sourceKind;
  final String sourceId;
  final String accountUserId;
  final String ownerIdentityId;
  final String fullHandle;
  final String localPreviousDid;
  final String currentDid;
  final String bindingGeneration;
  final String currentDeviceId;
  final int deviceAuthGeneration;
  final int registryVersion;
  final String stateRootFingerprint;
  final DateTime appliedAt;
  final String metadataJson;
}

class AuthorizedJoinActivationProgress {
  const AuthorizedJoinActivationProgress({
    required this.join,
    this.registryEpochReset,
  });

  final DeviceJoinProgress join;
  final HandleRecoveryRegistryEpochReset? registryEpochReset;
}

enum IdentityDeviceMode { legacy, vNext }

enum IdentityDeviceRole { member, admin }

enum IdentityDeviceReadiness {
  legacy,
  memberReady,
  adminAwaitingRoot,
  adminReady,
  blocked,
}

class IdentityDeviceSummary {
  const IdentityDeviceSummary({
    required this.identity,
    required this.mode,
    this.protocolDeviceId,
    this.role,
    this.signingKeyId,
    this.e2eeKeyId,
    required this.readiness,
    this.blockedReason,
  });

  final IdentitySummary identity;
  final IdentityDeviceMode mode;
  final String? protocolDeviceId;
  final IdentityDeviceRole? role;
  final String? signingKeyId;
  final String? e2eeKeyId;
  final IdentityDeviceReadiness readiness;
  final String? blockedReason;
}

sealed class LegacyUpgradeStatus {
  const LegacyUpgradeStatus();

  const factory LegacyUpgradeStatus.idle() = LegacyUpgradeIdle;
  const factory LegacyUpgradeStatus.running() = LegacyUpgradeRunning;
  const factory LegacyUpgradeStatus.retryRequired({
    required String identityId,
    required String code,
  }) = LegacyUpgradeRetryRequired;
  const factory LegacyUpgradeStatus.completed() = LegacyUpgradeCompleted;
}

final class LegacyUpgradeIdle extends LegacyUpgradeStatus {
  const LegacyUpgradeIdle();
}

final class LegacyUpgradeRunning extends LegacyUpgradeStatus {
  const LegacyUpgradeRunning();
}

final class LegacyUpgradeRetryRequired extends LegacyUpgradeStatus {
  const LegacyUpgradeRetryRequired({
    required this.identityId,
    required this.code,
  });

  final String identityId;
  final String code;
}

final class LegacyUpgradeCompleted extends LegacyUpgradeStatus {
  const LegacyUpgradeCompleted();
}

enum IdentitySecretStorageBackend { fileCompat, vault }

class IdentityVaultStatus {
  const IdentityVaultStatus({
    required this.identity,
    required this.storagePolicy,
    required this.selectedBackend,
    required this.vaultAvailable,
    required this.vaultMetadataPresent,
    required this.vaultMetadataVerified,
    this.workspaceId,
    this.deviceId,
    this.plaintextCompatRetained,
    this.missing = const [],
    this.warnings = const [],
  });

  final IdentitySummary identity;
  final IdentitySecretStoragePolicy storagePolicy;
  final IdentitySecretStorageBackend selectedBackend;
  final bool vaultAvailable;
  final bool vaultMetadataPresent;
  final bool vaultMetadataVerified;
  final String? workspaceId;
  final String? deviceId;
  final bool? plaintextCompatRetained;
  final List<String> missing;
  final List<String> warnings;
}

class IdentityVaultMigrationReport {
  const IdentityVaultMigrationReport({
    required this.identity,
    required this.status,
    required this.migrated,
    required this.verified,
    required this.plaintextCompatRetained,
    this.warnings = const [],
  });

  final IdentitySummary identity;
  final IdentityVaultStatus status;
  final bool migrated;
  final bool verified;
  final bool plaintextCompatRetained;
  final List<String> warnings;
}

class IdentityVaultVerificationReport {
  const IdentityVaultVerificationReport({
    required this.identity,
    required this.status,
    required this.verified,
    this.warnings = const [],
  });

  final IdentitySummary identity;
  final IdentityVaultStatus status;
  final bool verified;
  final List<String> warnings;
}

class InitialProfile {
  const InitialProfile({this.displayName, this.avatarUrl});

  final String? displayName;
  final String? avatarUrl;
}

class DefaultIdentityChange {
  const DefaultIdentityChange({
    this.previous,
    required this.next,
    required this.requiresDefaultIdentityWrite,
    this.warnings = const [],
  });

  final IdentitySummary? previous;
  final IdentitySummary next;
  final bool requiresDefaultIdentityWrite;
  final List<String> warnings;
}

class DeleteLocalIdentityResult {
  const DeleteLocalIdentityResult({
    required this.deleted,
    required this.wasDefault,
    this.nextDefault,
    this.warnings = const [],
  });

  final IdentitySummary deleted;
  final bool wasDefault;
  final IdentitySummary? nextDefault;
  final List<String> warnings;
}

class DaemonSubkeyPrivatePackage {
  const DaemonSubkeyPrivatePackage({
    required this.schema,
    required this.userDid,
    required this.verificationMethod,
    required this.keyType,
    this.keyAlgorithm,
    required this.publicKeyMultibase,
    this.privateKeyEncoding = 'pem',
    String? privateKeyPem,
    String? privateKeyMultibase,
  }) : privateKeyPem = privateKeyPem ?? privateKeyMultibase ?? '',
       privateKeyMultibase = privateKeyMultibase ?? privateKeyPem ?? '';

  final String schema;
  final String userDid;
  final String verificationMethod;
  final String keyType;
  final String? keyAlgorithm;
  final String publicKeyMultibase;
  final String privateKeyEncoding;
  final String privateKeyPem;
  @Deprecated('Use privateKeyPem for PEM v2 packages.')
  final String privateKeyMultibase;
}

class DaemonSubkeyAuthorizationRevokeResult {
  const DaemonSubkeyAuthorizationRevokeResult({
    required this.userDid,
    required this.verificationMethod,
    required this.updated,
  });

  final String userDid;
  final String verificationMethod;
  final bool updated;
}

class HandleRegistrationResult {
  const HandleRegistrationResult({
    this.identity,
    this.accountId,
    required this.handle,
    required this.method,
    required this.state,
    this.joinRequired,
    this.defaultIdentityChange,
    this.warnings = const [],
  });

  final IdentitySummary? identity;
  final String? accountId;
  final String handle;
  final String method;
  final String state;
  final HandleRegistrationJoinRequired? joinRequired;
  final DefaultIdentityChange? defaultIdentityChange;
  final List<String> warnings;
}

class HandleRegistrationJoinRequired {
  const HandleRegistrationJoinRequired({
    required this.did,
    required this.accountVerificationGrant,
  });

  final String did;
  final DeviceJoinAccountVerificationGrant accountVerificationGrant;
}

enum DeviceJoinSide { newDevice, admin }

enum DeviceJoinPhase {
  pending,
  challengePrepared,
  responsePrepared,
  responseVerified,
  approvalPrepared,
  authorized,
  cancelled,
  expired,
}

enum DeviceJoinRole { member, admin }

enum DeviceJoinAuthorizationStatus { active, revoked }

enum DeviceJoinRemoteState {
  pending,
  challengeSent,
  responseVerified,
  consumed,
  cancelled,
  rejected,
  expired,
}

class DeviceJoinSessionSummary {
  const DeviceJoinSessionSummary({
    required this.joinSessionId,
    required this.did,
    required this.protocolDeviceId,
    required this.side,
    required this.phase,
    required this.expiresAt,
  });

  final String joinSessionId;
  final String did;
  final String protocolDeviceId;
  final DeviceJoinSide side;
  final DeviceJoinPhase phase;
  final String expiresAt;
}

class DeviceJoinAuthorizedDeviceSummary {
  const DeviceJoinAuthorizedDeviceSummary({
    required this.protocolDeviceId,
    required this.signingKeyId,
    required this.e2eeKeyId,
    required this.status,
    required this.role,
    required this.managementReady,
    required this.isCurrent,
  });

  final String protocolDeviceId;
  final String signingKeyId;
  final String e2eeKeyId;
  final DeviceJoinAuthorizationStatus status;
  final DeviceJoinRole role;
  final bool managementReady;
  final bool isCurrent;
}

class DeviceRegistryAuthorizedDeviceSummary {
  const DeviceRegistryAuthorizedDeviceSummary({
    required this.protocolDeviceId,
    required this.signingKeyId,
    required this.e2eeKeyId,
    required this.status,
    required this.role,
    required this.managementReady,
    required this.isCurrent,
    required this.authGeneration,
  });

  final String protocolDeviceId;
  final String signingKeyId;
  final String e2eeKeyId;
  final DeviceJoinAuthorizationStatus status;
  final DeviceJoinRole role;
  final bool managementReady;
  final bool isCurrent;
  final String authGeneration;
}

class DeviceJoinRequestNotice {
  const DeviceJoinRequestNotice({
    required this.eventId,
    required this.joinSessionId,
    required this.did,
    required this.protocolDeviceId,
    required this.candidateKeyFingerprint,
    required this.issuedAt,
    required this.expiresAt,
    required this.state,
    required this.claimedByCurrentDevice,
    required this.canStartVerification,
  });

  final String eventId;
  final String joinSessionId;
  final String did;
  final String protocolDeviceId;
  final String candidateKeyFingerprint;
  final String issuedAt;
  final String expiresAt;
  final DeviceJoinRemoteState state;
  final bool claimedByCurrentDevice;
  final bool canStartVerification;

  @override
  String toString() =>
      'DeviceJoinRequestNotice(eventId: $eventId, '
      'joinSessionId: $joinSessionId, did: $did, '
      'protocolDeviceId: $protocolDeviceId, state: $state, '
      'claimedByCurrentDevice: $claimedByCurrentDevice, '
      'canStartVerification: $canStartVerification)';
}

class DeviceJoinRegistrySnapshot {
  const DeviceJoinRegistrySnapshot({
    required this.did,
    required this.registryVersion,
    required this.devices,
  });

  final String did;
  final String registryVersion;
  final List<DeviceRegistryAuthorizedDeviceSummary> devices;
}

enum DeviceJoinRejectReason { userRejected, sasMismatch }

class DeviceJoinProgress {
  const DeviceJoinProgress({
    required this.session,
    required this.remoteState,
    this.sas,
    this.authorizedDevice,
  });

  final DeviceJoinSessionSummary session;
  final DeviceJoinRemoteState remoteState;
  final String? sas;
  final DeviceJoinAuthorizedDeviceSummary? authorizedDevice;

  @override
  String toString() =>
      'DeviceJoinProgress(joinSessionId: ${session.joinSessionId}, '
      'remoteState: $remoteState, '
      'sas: ${sas == null ? 'null' : '<redacted>'})';
}

class DeviceJoinApprovalPrompt {
  const DeviceJoinApprovalPrompt({
    required this.approvalHandle,
    required this.joinSessionId,
    required this.sas,
    required this.expiresAt,
  });

  final String approvalHandle;
  final String joinSessionId;
  final String sas;
  final String expiresAt;

  @override
  String toString() =>
      'DeviceJoinApprovalPrompt(joinSessionId: $joinSessionId, '
      'approvalHandle: <redacted>, sas: <redacted>)';
}

/// Opaque authorization returned by [RootKeyTransferApi.prepare].
///
/// Applications may only pass it back to the same client's confirm operation.
abstract interface class RootKeyTransferAuthorizationHandle {}

/// Secret-free exact-device summary returned by Core before confirmation.
class RootKeyTransferRecipientSummary {
  const RootKeyTransferRecipientSummary({
    required this.did,
    required this.deviceId,
    required this.signingKeyId,
    required this.e2eeKeyId,
    required this.registryVersion,
  });

  final String did;
  final String deviceId;
  final String signingKeyId;
  final String e2eeKeyId;
  final int registryVersion;
}

/// Prepared, short-lived authorization for one exact recipient device.
class RootKeyTransferPreparation {
  const RootKeyTransferPreparation({
    required this.authorizationHandle,
    required this.recipient,
    required this.expiresAt,
  });

  final RootKeyTransferAuthorizationHandle authorizationHandle;
  final RootKeyTransferRecipientSummary recipient;
  final String expiresAt;

  @override
  String toString() =>
      'RootKeyTransferPreparation(authorizationHandle: <redacted>, '
      'recipientDeviceId: ${recipient.deviceId}, expiresAt: $expiresAt)';
}

/// Safe result for an accepted management-device root-key control delivery.
class RootKeyTransferSendResult {
  const RootKeyTransferSendResult({
    required this.did,
    required this.senderDeviceId,
    required this.recipientDeviceId,
    required this.messageId,
    required this.acceptedAt,
  });

  final String did;
  final String senderDeviceId;
  final String recipientDeviceId;
  final String messageId;
  final String acceptedAt;
}

/// Closed, secret-free root-transfer failure exposed to the host.
class RootKeyTransferException implements Exception {
  const RootKeyTransferException({required this.code, required this.retryable});

  final String code;
  final bool retryable;

  @override
  String toString() =>
      'RootKeyTransferException(code: $code, retryable: $retryable)';
}

enum DeviceRevokeStatus { revoked }

/// Secret-free result of permanently revoking one AWiki device.
class DeviceRevokeResult {
  const DeviceRevokeResult({
    required this.did,
    required this.targetDeviceId,
    required this.status,
  });

  final String did;
  final String targetDeviceId;
  final DeviceRevokeStatus status;
}
