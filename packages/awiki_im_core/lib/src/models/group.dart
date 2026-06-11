import 'message.dart';

class GroupSummary {
  const GroupSummary({
    this.id,
    required this.did,
    this.name,
    this.myRole,
    this.membershipStatus,
    this.memberCount,
    this.lastMessageAt,
  });

  final String? id;
  final String did;
  final String? name;
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
    this.description,
    this.myRole,
    super.membershipStatus,
    super.memberCount,
    super.lastMessageAt,
  });

  final String? description;
  final String? myRole;
}

class GroupMember {
  const GroupMember({
    this.did,
    this.handle,
    this.role,
    this.status,
    this.joinedAt,
  });

  final String? did;
  final String? handle;
  final String? role;
  final String? status;
  final String? joinedAt;
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
    this.description,
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
  final String? description;
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
