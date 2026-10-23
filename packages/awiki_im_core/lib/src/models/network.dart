import 'dart:async';

enum AwikiNetworkPolicy { systemPreferred, direct }

enum AwikiNetworkStatus { systemProxy, direct, proxyFallback }

class AwikiNetworkResponse {
  AwikiNetworkResponse({
    required this.statusCode,
    required this.headers,
    required this.body,
    required this.close,
  });
  final int statusCode;
  final Map<String, String> headers;
  final Stream<List<int>> body;
  final Future<void> Function() close;
}

class AwikiNetworkDiagnostic {
  const AwikiNetworkDiagnostic({
    required this.at,
    required this.operation,
    required this.stage,
    required this.elapsedMs,
  });
  final String at;
  final String operation;
  final String stage;
  final int elapsedMs;
}
