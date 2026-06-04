import 'dart:convert';

class AwikiAgentSchemas {
  static const commandV1 = 'awiki.agent.command.v1';
  static const statusV1 = 'awiki.agent.status.v1';
}

Map<String, Object?>? decodePayloadObject(String? payloadJson) {
  if (payloadJson == null || payloadJson.trim().isEmpty) {
    return null;
  }
  try {
    final decoded = jsonDecode(payloadJson);
    if (decoded is! Map) {
      return null;
    }
    return decoded.map(
      (key, value) => MapEntry(key.toString(), value as Object?),
    );
  } on Object {
    return null;
  }
}

String? awikiPayloadSchema(String? payloadJson) {
  final payload = decodePayloadObject(payloadJson);
  final schema = payload?['schema'];
  if (schema is! String || schema.trim().isEmpty) {
    return null;
  }
  return schema;
}

bool isAwikiAgentCommandPayload(String? payloadJson) =>
    awikiPayloadSchema(payloadJson) == AwikiAgentSchemas.commandV1;

bool isAwikiAgentStatusPayload(String? payloadJson) =>
    awikiPayloadSchema(payloadJson) == AwikiAgentSchemas.statusV1;

bool isAwikiAgentControlPayload(String? payloadJson) =>
    isAwikiAgentCommandPayload(payloadJson) ||
    isAwikiAgentStatusPayload(payloadJson);
