class AwikiImCoreException implements Exception {
  const AwikiImCoreException({
    required this.code,
    required this.message,
    this.field,
    this.statusCode,
    this.capability,
  });

  final String code;
  final String message;
  final String? field;
  final int? statusCode;
  final String? capability;

  @override
  String toString() => 'AwikiImCoreException($code): $message';
}
