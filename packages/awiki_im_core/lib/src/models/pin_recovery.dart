import 'identity.dart';

class PinRecoveryTenantConfig {
  const PinRecoveryTenantConfig({
    required this.tenantId,
    required this.deploymentId,
    required this.controlBaseUrl,
    required this.configSigningKeys,
  });
  final String tenantId;
  final String deploymentId;
  final String controlBaseUrl;
  final Map<String, String> configSigningKeys;
}

class PhoneChallengeSummary {
  const PhoneChallengeSummary({
    required this.challengeHandle,
    required this.expiresAt,
    required this.resendAfterSeconds,
    required this.maskedPhone,
  });
  final String challengeHandle;
  final String expiresAt;
  final int resendAfterSeconds;
  final String maskedPhone;
}

class DiscoverySessionSummary {
  const DiscoverySessionSummary({
    required this.discoveryHandle,
    required this.tenantId,
    required this.expiresAt,
  });
  final String discoveryHandle;
  final String tenantId;
  final String expiresAt;
}

class PhoneSessionSummary {
  const PhoneSessionSummary({
    required this.sessionHandle,
    required this.tenantId,
    required this.target,
    required this.expiresAt,
  });
  final String sessionHandle;
  final String tenantId;
  final PinRecoveryTarget target;
  final String expiresAt;
}

class PinRecoveryTarget {
  const PinRecoveryTarget({
    required this.targetId,
    required this.fullHandle,
    required this.did,
    required this.bindingGeneration,
    required this.activeEpoch,
    required this.canEnable,
    required this.canRecover,
    required this.unavailableReason,
  });
  final String targetId;
  final String fullHandle;
  final String did;
  final String bindingGeneration;
  final String? activeEpoch;
  final bool canEnable;
  final bool canRecover;
  final String? unavailableReason;
}

enum PinRecoveryKind { enable, recover }

enum PinRecoveryPhase {
  awaitingPin,
  authorizing,
  registering,
  verifyingBackup,
  recoveringKey,
  awaitingDeviceConfirmation,
  committing,
  commitUnknown,
  localActivationPending,
  activeSecurePending,
  cleanupPending,
  completed,
  failed,
  cancelled,
}

enum PinRecoveryAction {
  verifyPhone,
  submitPin,
  confirmDevice,
  resume,
  cancel,
  retryLocalActivation,
  inspect,
}

enum PinRecoverySecureReadiness {
  notStarted,
  prekeysPending,
  securePending,
  ready,
}

class RealmProgress {
  const RealmProgress({
    required this.realmId,
    required this.phase,
    required this.errorCode,
  });
  final String realmId;
  final String phase;
  final String? errorCode;
}

class PinRecoveryError {
  const PinRecoveryError({
    required this.code,
    required this.retryable,
    required this.retryAfterSeconds,
  });
  final String code;
  final bool retryable;
  final int? retryAfterSeconds;
}

class PinRecoveryProgress {
  const PinRecoveryProgress({
    required this.operationId,
    required this.kind,
    required this.phase,
    required this.allowedActions,
    required this.target,
    required this.nodeProgress,
    required this.attemptsUsed,
    required this.attemptsLimit,
    required this.needsPhoneVerification,
    required this.needsUserConfirmation,
    required this.recoveredIdentity,
    required this.newDeviceId,
    this.deviceLabel,
    required this.secureReadiness,
    required this.cleanupRemaining,
    required this.error,
  });
  final String operationId;
  final PinRecoveryKind kind;
  final PinRecoveryPhase phase;
  final List<PinRecoveryAction> allowedActions;
  final PinRecoveryTarget target;
  final List<RealmProgress> nodeProgress;
  final int attemptsUsed;
  final int attemptsLimit;
  final bool needsPhoneVerification;
  final bool needsUserConfirmation;
  final IdentitySelector? recoveredIdentity;
  final String? newDeviceId;
  final String? deviceLabel;
  final PinRecoverySecureReadiness secureReadiness;
  final int cleanupRemaining;
  final PinRecoveryError? error;
}

enum PinRecoveryBackupState { unknown, notEnabled, pending, active }

class PinRecoveryBackupStatus {
  const PinRecoveryBackupStatus({
    required this.did,
    required this.fullHandle,
    required this.bindingGeneration,
    required this.state,
    required this.activeEpoch,
    required this.error,
  });
  final String did;
  final String? fullHandle;
  final String? bindingGeneration;
  final PinRecoveryBackupState state;
  final String? activeEpoch;
  final PinRecoveryError? error;
}
