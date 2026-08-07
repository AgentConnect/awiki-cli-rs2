enum DeviceRevokeOutcomeCategory {
  cancelledBeforeSubmit,
  rejectedBeforeCommit,
  outcomeUnknown,
}

class AwikiImCoreException implements Exception {
  const AwikiImCoreException({
    required this.code,
    required this.message,
    this.field,
    this.statusCode,
    this.capability,
    this.serviceCode,
    this.serviceDataJson,
    this.deviceRevokeOutcomeCategory,
    this.handleRecoveryFailureCode,
  });

  final String code;
  final String message;
  final String? field;
  final int? statusCode;
  final String? capability;
  final String? serviceCode;
  final String? serviceDataJson;
  final DeviceRevokeOutcomeCategory? deviceRevokeOutcomeCategory;
  final HandleRecoveryFailureCode? handleRecoveryFailureCode;

  @override
  String toString() => 'AwikiImCoreException($code): $message';
}

enum HandleRecoveryFailureCode {
  factorRetryRequired,
  resultAbsent,
  outcomeUnknown,
  localKeyUnavailable,
  localTransitionPending,
  localMigrationUnsupported,
  unknownEpoch,
  notPrepared,
  userPresenceRequired,
  transitionMismatch,
  transitionChainUnsupported,
  remoteStateChanged,
  localStateUnavailable,
  blocked,
}

extension HandleRecoveryFailureCodeRetryability on HandleRecoveryFailureCode {
  bool get retryable => switch (this) {
    HandleRecoveryFailureCode.factorRetryRequired ||
    HandleRecoveryFailureCode.resultAbsent ||
    HandleRecoveryFailureCode.outcomeUnknown ||
    HandleRecoveryFailureCode.localTransitionPending => true,
    _ => false,
  };
}
