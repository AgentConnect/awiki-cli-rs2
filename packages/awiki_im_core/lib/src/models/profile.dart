class UserProfile {
  const UserProfile({
    required this.subject,
    this.handle,
    this.fullHandle,
    this.displayName,
    this.bio,
    this.tags = const [],
    this.markdown,
    this.avatarUrl,
    this.updatedAt,
  });

  final String subject;
  final String? handle;
  final String? fullHandle;
  final String? displayName;
  final String? bio;
  final List<String> tags;
  final String? markdown;
  final String? avatarUrl;
  final String? updatedAt;
}

class ProfilePatch {
  const ProfilePatch({this.displayName, this.bio, this.tags, this.markdown});

  final String? displayName;
  final String? bio;
  final List<String>? tags;
  final String? markdown;
}
