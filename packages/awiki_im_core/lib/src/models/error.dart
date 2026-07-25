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
  });

  final String code;
  final String message;
  final String? field;
  final int? statusCode;
  final String? capability;
  final String? serviceCode;
  final String? serviceDataJson;
  final DeviceRevokeOutcomeCategory? deviceRevokeOutcomeCategory;

  @override
  String toString() => 'AwikiImCoreException($code): $message';
}
