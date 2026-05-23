enum AuthScope { userProfile, messaging, groupMessaging }

class AuthStatus {
  const AuthStatus({
    required this.subject,
    required this.hasSession,
    this.expiresAt,
    required this.needsRefresh,
    this.warnings = const [],
  });

  final String subject;
  final bool hasSession;
  final String? expiresAt;
  final bool needsRefresh;
  final List<String> warnings;

  bool get authenticated => hasSession;
}

class SessionBundle {
  const SessionBundle({
    required this.subject,
    required this.scope,
    this.expiresAt,
    required this.refreshed,
  });

  final String subject;
  final AuthScope scope;
  final String? expiresAt;
  final bool refreshed;
}

class SessionUpdate {
  const SessionUpdate({
    required this.subject,
    this.previousExpiresAt,
    this.newExpiresAt,
    required this.refreshed,
  });

  final String subject;
  final String? previousExpiresAt;
  final String? newExpiresAt;
  final bool refreshed;
}
