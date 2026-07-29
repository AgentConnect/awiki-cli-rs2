import 'message.dart';

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

enum RealtimeReconnectMode { disabled, fixed, exponential }

class RealtimeOptions {
  const RealtimeOptions({
    this.reconnect = RealtimeReconnectMode.disabled,
    this.eventBuffer = 128,
    this.reconnectDelayMs,
    this.reconnectBaseDelayMs,
    this.reconnectMaxDelayMs,
    this.reconnectMaxAttempts,
    this.subscriptions = const ['messages', 'groups', 'notifications'],
  });

  final RealtimeReconnectMode reconnect;
  final int eventBuffer;
  final int? reconnectDelayMs;
  final int? reconnectBaseDelayMs;
  final int? reconnectMaxDelayMs;
  final int? reconnectMaxAttempts;
  final List<String> subscriptions;
}

class RealtimeConnectionState {
  const RealtimeConnectionState({required this.state, this.reason});

  final String state;
  final String? reason;
}

class RealtimeSyncHint {
  const RealtimeSyncHint({
    this.eventId,
    this.eventSeq,
    this.eventType,
    required this.syncDirty,
    required this.gapDetected,
  });

  final String? eventId;
  final String? eventSeq;
  final String? eventType;
  final bool syncDirty;
  final bool gapDetected;
}

class RealtimeEvent {
  const RealtimeEvent({
    required this.kind,
    this.state,
    this.reason,
    this.message,
    this.messageId,
    this.threadKind,
    this.threadId,
    this.updateKind,
    this.group,
    this.notificationId,
    this.title,
    this.body,
    this.source,
    this.hostKind,
    this.contentType,
    this.notificationType,
    this.sync,
  });

  final String kind;
  final String? state;
  final String? reason;
  final Message? message;
  final String? messageId;
  final String? threadKind;
  final String? threadId;
  final String? updateKind;
  final String? group;
  final String? notificationId;
  final String? title;
  final String? body;
  final String? source;
  final String? hostKind;
  final String? contentType;
  final String? notificationType;
  final RealtimeSyncHint? sync;

  bool get isConnectionState => kind == 'connection_state_changed';

  bool get isSystemNotificationChanged => kind == 'system_notification_changed';
}

abstract interface class RealtimeSession {
  Future<void> stop();

  Future<void> dispose();
}
