class UserProfile {
  const UserProfile({
    required this.subject,
    this.handle,
    this.fullHandle,
    this.displayName,
    this.bio,
    this.description,
    this.tags = const [],
    this.markdown,
    this.avatarUri,
    this.avatarUrl,
    this.profileUri,
    this.subjectType,
    this.agentKind,
    this.agentCapabilities = const [],
    this.updatedAt,
    this.profileVersion,
    this.versionId,
    this.ttl,
  });

  final String subject;
  final String? handle;
  final String? fullHandle;
  final String? displayName;
  final String? bio;
  final String? description;
  final List<String> tags;
  final String? markdown;
  final String? avatarUri;
  final String? avatarUrl;
  final String? profileUri;
  final String? subjectType;
  final String? agentKind;
  final List<String> agentCapabilities;
  final String? updatedAt;
  final String? profileVersion;
  final String? versionId;
  final int? ttl;
}

class ProfilePatch {
  const ProfilePatch({
    this.displayName,
    this.bio,
    this.tags,
    this.markdown,
    this.avatarUri,
    this.avatarUrl,
  });

  final String? displayName;
  final String? bio;
  final List<String>? tags;
  final String? markdown;
  final String? avatarUri;
  final String? avatarUrl;
}
