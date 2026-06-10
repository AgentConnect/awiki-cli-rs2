import 'profile.dart';

sealed class IdentitySubject {
  const IdentitySubject();

  const factory IdentitySubject.did(String did) = DidIdentitySubject;
  const factory IdentitySubject.handle(String handle) = HandleIdentitySubject;
  const factory IdentitySubject.any(String value) = AnyIdentitySubject;
}

class DidIdentitySubject extends IdentitySubject {
  const DidIdentitySubject(this.did);
  final String did;
}

class HandleIdentitySubject extends IdentitySubject {
  const HandleIdentitySubject(this.handle);
  final String handle;
}

class AnyIdentitySubject extends IdentitySubject {
  const AnyIdentitySubject(this.value);
  final String value;
}

class DirectoryResolution {
  const DirectoryResolution({
    required this.input,
    required this.did,
    this.handle,
    this.profile,
    this.warnings = const [],
  });

  final String input;
  final String did;
  final String? handle;
  final UserProfile? profile;
  final List<String> warnings;
}

class RelationStatus {
  const RelationStatus({
    required this.peer,
    this.relationship,
    this.displayName,
  });

  final String peer;
  final String? relationship;
  final String? displayName;
}

class DisplayProfile {
  const DisplayProfile({
    this.did,
    this.handle,
    this.displayName,
    this.avatarUri,
    this.avatarUrl,
    this.profileUri,
    this.subjectType,
    required this.cacheHit,
    this.warnings = const [],
  });

  final String? did;
  final String? handle;
  final String? displayName;
  final String? avatarUri;
  final String? avatarUrl;
  final String? profileUri;
  final String? subjectType;
  final bool cacheHit;
  final List<String> warnings;
}

class RelationshipListItem {
  const RelationshipListItem({
    required this.did,
    this.handle,
    this.displayName,
    this.avatarUri,
    this.avatarUrl,
    this.profileUri,
    this.subjectType,
    required this.relationship,
    this.createdAt,
    this.warnings = const [],
  });

  final String did;
  final String? handle;
  final String? displayName;
  final String? avatarUri;
  final String? avatarUrl;
  final String? profileUri;
  final String? subjectType;
  final String relationship;
  final String? createdAt;
  final List<String> warnings;
}

class RelationshipPage {
  const RelationshipPage({
    required this.items,
    this.nextCursor,
    required this.hasMore,
  });

  final List<RelationshipListItem> items;
  final String? nextCursor;
  final bool hasMore;
}
