import 'message.dart';

enum GroupIdentityMode { handle, didOnly }

class GroupSummary {
  const GroupSummary({
    this.id,
    required this.did,
    this.name,
    this.displayName,
    this.avatarUri,
    this.myRole,
    this.membershipStatus,
    this.memberCount,
    this.lastMessageAt,
  });

  final String? id;
  final String did;
  final String? name;
  final String? displayName;
  final String? avatarUri;
  final String? myRole;
  final String? membershipStatus;
  final int? memberCount;
  final String? lastMessageAt;
}

class GroupSnapshot extends GroupSummary {
  const GroupSnapshot({
    super.id,
    required super.did,
    super.name,
    super.displayName,
    super.avatarUri,
    super.myRole,
    this.description,
    super.membershipStatus,
    super.memberCount,
    super.lastMessageAt,
  });

  final String? description;
}

class GroupMember {
  const GroupMember({
    this.membershipId,
    this.peerPersonaId,
    this.did,
    this.credentialDid,
    this.handle,
    this.role,
    this.status,
    this.joinedAt,
    this.subjectType,
  });

  final String? membershipId;
  final String? peerPersonaId;
  final String? did;
  final String? credentialDid;
  final String? handle;
  final String? role;
  final String? status;
  final String? joinedAt;
  final String? subjectType;
}

class GroupDiscoverability {
  const GroupDiscoverability._(this.value);

  static const private = GroupDiscoverability._('private');
  static const public = GroupDiscoverability._('public');
  static const unlisted = GroupDiscoverability._('unlisted');

  factory GroupDiscoverability.custom(String value) {
    final trimmed = value.trim();
    if (trimmed.isEmpty) {
      throw ArgumentError.value(value, 'value', 'must not be empty');
    }
    return GroupDiscoverability._(trimmed);
  }

  final String value;
}

class GroupAdmissionMode {
  const GroupAdmissionMode._(this.value);

  static const openJoin = GroupAdmissionMode._('open-join');
  static const inviteOnly = GroupAdmissionMode._('invite-only');
  static const approvalRequired = GroupAdmissionMode._('approval');
  static const closed = GroupAdmissionMode._('closed');

  factory GroupAdmissionMode.custom(String value) {
    final trimmed = value.trim();
    if (trimmed.isEmpty) {
      throw ArgumentError.value(value, 'value', 'must not be empty');
    }
    return GroupAdmissionMode._(trimmed);
  }

  final String value;
}

class GroupMessageSecurityProfile {
  const GroupMessageSecurityProfile._(this.value);

  static const transportProtected = GroupMessageSecurityProfile._(
    'transport-protected',
  );
  static const groupE2ee = GroupMessageSecurityProfile._('group-e2ee');

  factory GroupMessageSecurityProfile.custom(String value) {
    final trimmed = value.trim();
    if (trimmed.isEmpty) {
      throw ArgumentError.value(value, 'value', 'must not be empty');
    }
    return GroupMessageSecurityProfile._(trimmed);
  }

  final String value;
}

class GroupMemberLimit {
  const GroupMemberLimit(this.value);

  final int value;
}

class CreateGroupRequest {
  const CreateGroupRequest({
    required this.name,
    this.identityMode = GroupIdentityMode.didOnly,
    this.identityHandle,
    this.description,
    this.avatarUri,
    this.discoverability,
    this.admissionMode,
    this.messageSecurityProfile,
    this.e2ee = false,
    this.slug,
    this.goal,
    this.rules,
    this.messagePrompt,
    this.docUrl,
    this.attachmentsAllowed,
    this.maxMembers,
    this.memberMaxMessages,
    this.memberMaxTotalChars,
  });

  final String name;
  final GroupIdentityMode identityMode;
  final String? identityHandle;
  final String? description;
  final String? avatarUri;
  final GroupDiscoverability? discoverability;
  final GroupAdmissionMode? admissionMode;
  final GroupMessageSecurityProfile? messageSecurityProfile;
  final bool e2ee;
  final String? slug;
  final String? goal;
  final String? rules;
  final String? messagePrompt;
  final String? docUrl;
  final bool? attachmentsAllowed;
  final GroupMemberLimit? maxMembers;
  final int? memberMaxMessages;
  final int? memberMaxTotalChars;
}

class JoinGroupRequest {
  const JoinGroupRequest({
    required this.groupDid,
    this.identityMode = GroupIdentityMode.didOnly,
    this.identityHandle,
  });

  final String groupDid;
  final GroupIdentityMode identityMode;
  final String? identityHandle;
}

class GroupRebindRecoveryItem {
  const GroupRebindRecoveryItem({
    required this.groupDid,
    required this.layer,
    required this.phase,
    required this.blocked,
  });

  final String groupDid;
  final String layer;
  final String phase;
  final bool blocked;
}

class GroupRebindRecoverySummary {
  const GroupRebindRecoverySummary({
    required this.processed,
    required this.completed,
    required this.pending,
    required this.blocked,
    this.sendPausedGroupDids = const [],
    this.items = const [],
    this.warnings = const [],
  });

  final int processed;
  final int completed;
  final int pending;
  final int blocked;
  final List<String> sendPausedGroupDids;
  final List<GroupRebindRecoveryItem> items;
  final List<String> warnings;
}

class GroupReadResult {
  const GroupReadResult({
    this.group,
    this.groups = const [],
    this.members = const [],
    required this.messages,
    this.total,
    this.source,
    this.warnings = const [],
  });

  final GroupSnapshot? group;
  final List<GroupSummary> groups;
  final List<GroupMember> members;
  final MessagePage messages;
  final int? total;
  final String? source;
  final List<String> warnings;
}
