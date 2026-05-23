class RealtimeCapability {
  const RealtimeCapability({
    required this.statusSupported,
    required this.connectSupported,
    required this.runnerExposed,
    this.reason,
  });

  final bool statusSupported;
  final bool connectSupported;
  final bool runnerExposed;
  final String? reason;
}

class RealtimeStatus {
  const RealtimeStatus({
    required this.connected,
    required this.state,
    this.subscriptions = const [],
    this.lastError,
    this.warnings = const [],
  });

  final bool connected;
  final String state;
  final List<String> subscriptions;
  final String? lastError;
  final List<String> warnings;
}
