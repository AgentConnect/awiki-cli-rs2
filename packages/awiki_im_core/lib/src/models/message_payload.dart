import 'dart:convert';

import 'error.dart';

const int maxMessagePayloadJsonBytes = 64 * 1024;

void validateMessagePayloadJson(String payloadJson) {
  if (utf8.encode(payloadJson).length > maxMessagePayloadJsonBytes) {
    throw const AwikiImCoreException(
      code: 'invalid_payload',
      message: 'payloadJson must not exceed 64 KB',
    );
  }
  try {
    final decoded = jsonDecode(payloadJson);
    if (decoded is! Map) {
      throw const AwikiImCoreException(
        code: 'invalid_payload',
        message: 'payloadJson must be a JSON object',
      );
    }
  } on AwikiImCoreException {
    rethrow;
  } on Object {
    throw const AwikiImCoreException(
      code: 'invalid_payload',
      message: 'payloadJson must be valid JSON',
    );
  }
}

Map<String, Object?> decodeMessagePayloadObject(String payloadJson) {
  validateMessagePayloadJson(payloadJson);
  final decoded = jsonDecode(payloadJson) as Map;
  return decoded.cast<String, Object?>();
}
