import 'message.dart';

enum DirectSecureState {
  ready,
  preparing,
  waitingForPeer,
  needsRepair,
  unavailable,
  unknown,
}

class DirectSecureStatus {
  const DirectSecureStatus({
    required this.peer,
    this.resolvedPeer,
    required this.state,
    required this.canSendSecure,
    required this.pendingOutboxCount,
    this.problem,
    this.warnings = const [],
  });

  final String peer;
  final String? resolvedPeer;
  final DirectSecureState state;
  final bool canSendSecure;
  final int pendingOutboxCount;
  final SecureProblem? problem;
  final List<String> warnings;
}

class DirectSecurePrepareResult {
  const DirectSecurePrepareResult({
    required this.peer,
    required this.state,
    required this.canSendSecure,
    this.warnings = const [],
  });

  final String peer;
  final DirectSecureState state;
  final bool canSendSecure;
  final List<String> warnings;
}

class DirectSecureRepairResult {
  const DirectSecureRepairResult({
    required this.peer,
    required this.state,
    required this.repaired,
    this.problem,
    this.warnings = const [],
  });

  final String peer;
  final DirectSecureState state;
  final bool repaired;
  final SecureProblem? problem;
  final List<String> warnings;
}

enum GroupSecureState {
  ready,
  syncing,
  needsRepair,
  waitingForMembershipUpdate,
  missingLocalState,
  unavailable,
  unknown,
}

class GroupSecureStatus {
  const GroupSecureStatus({
    required this.group,
    required this.state,
    required this.canSendSecure,
    required this.localReadiness,
    required this.pendingWork,
    this.problem,
    this.warnings = const [],
  });

  final String group;
  final GroupSecureState state;
  final bool canSendSecure;
  final GroupSecureLocalReadiness localReadiness;
  final GroupSecurePendingWork pendingWork;
  final SecureProblem? problem;
  final List<String> warnings;
}

class GroupSecureLocalReadiness {
  const GroupSecureLocalReadiness({
    required this.hasLocalState,
    required this.hasActiveMembership,
  });

  final bool hasLocalState;
  final bool hasActiveMembership;
}

class GroupSecurePendingWork {
  const GroupSecurePendingWork({
    required this.pendingNotices,
    required this.pendingCommits,
  });

  final int pendingNotices;
  final int pendingCommits;
}

class GroupSecurePrepareResult {
  const GroupSecurePrepareResult({
    required this.group,
    required this.state,
    required this.canSendSecure,
    this.warnings = const [],
  });

  final String group;
  final GroupSecureState state;
  final bool canSendSecure;
  final List<String> warnings;
}

class GroupSecureRepairResult {
  const GroupSecureRepairResult({
    required this.group,
    required this.state,
    required this.repaired,
    this.addedDevices = 0,
    this.removedDevices = 0,
    this.remainingDevices = 0,
    this.problem,
    this.warnings = const [],
  });

  final String group;
  final GroupSecureState state;
  final bool repaired;
  final int addedDevices;
  final int removedDevices;
  final int remainingDevices;
  final SecureProblem? problem;
  final List<String> warnings;
}

class SecureOutboxEntry {
  const SecureOutboxEntry({
    required this.id,
    required this.target,
    required this.messageKind,
    required this.status,
    required this.attemptCount,
    this.lastError,
    this.createdAt,
    this.updatedAt,
  });

  final String id;
  final MessageTarget target;
  final String messageKind;
  final SecureOutboxStatus status;
  final int attemptCount;
  final SecureProblem? lastError;
  final String? createdAt;
  final String? updatedAt;
}

enum SecureOutboxStatus { queued, sending, failed, sent, dropped }

class SecureOutboxResult {
  const SecureOutboxResult({
    required this.id,
    required this.status,
    this.delivery,
    this.warnings = const [],
  });

  final String id;
  final SecureOutboxStatus status;
  final SecureDelivery? delivery;
  final List<String> warnings;
}

class SecureDelivery {
  const SecureDelivery({this.messageId, required this.state});

  final String? messageId;
  final String state;
}

class SecureProblem {
  const SecureProblem({
    required this.code,
    required this.message,
    required this.retryable,
  });

  final SecureProblemCode code;
  final String message;
  final bool retryable;
}

enum SecureProblemCode {
  identityNotReady,
  peerNotFound,
  peerKeysUnavailable,
  sessionNeedsRepair,
  groupStateUnavailable,
  localStateUnavailable,
  transportUnavailable,
  unsupported,
  unknown,
}
