import 'message.dart';

class GroupSummary {
  const GroupSummary({
    this.id,
    required this.did,
    this.name,
    this.membershipStatus,
    this.memberCount,
    this.lastMessageAt,
  });

  final String? id;
  final String did;
  final String? name;
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
    this.serviceDid,
  });

  final String name;
  final String? description;
  final String? discoverability;
  final String? admissionMode;
  final String? messageSecurityProfile;
  final bool e2ee;
  final String? slug;
  final String? goal;
  final String? rules;
  final String? messagePrompt;
  final String? docUrl;
  final bool? attachmentsAllowed;
  final String? maxMembers;
  final int? memberMaxMessages;
  final int? memberMaxTotalChars;
  final String? serviceDid;
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
