enum LocalStateUpgradeEligibility { notRequired, required }

class LocalStateUpgradeInspection {
  const LocalStateUpgradeInspection({
    required this.eligibility,
    required this.sourceSchemaVersion,
    required this.targetSchemaVersion,
  });

  final LocalStateUpgradeEligibility eligibility;
  final int sourceSchemaVersion;
  final int targetSchemaVersion;
}

enum LocalStateUpgradeStatus { notRequired, completed }

class LocalStateUpgradeResult {
  const LocalStateUpgradeResult({
    required this.status,
    required this.sourceSchemaVersion,
    required this.targetSchemaVersion,
    required this.migratedPersonas,
    required this.migratedConversations,
    required this.unresolvedMessages,
    required this.aliasCount,
    required this.backupAvailable,
    this.aliasMappings = const <LocalStateConversationAliasMapping>[],
  });

  final LocalStateUpgradeStatus status;
  final int sourceSchemaVersion;
  final int targetSchemaVersion;
  final int migratedPersonas;
  final int migratedConversations;
  final int unresolvedMessages;
  final int aliasCount;
  final bool backupAvailable;
  final List<LocalStateConversationAliasMapping> aliasMappings;
}

class LocalStateConversationAliasMapping {
  const LocalStateConversationAliasMapping({
    required this.ownerIdentityId,
    required this.ownerDid,
    required this.legacyConversationId,
    required this.canonicalConversationId,
  });

  final String ownerIdentityId;
  final String ownerDid;
  final String legacyConversationId;
  final String canonicalConversationId;
}

class LocalStateRestoreResult {
  const LocalStateRestoreResult({
    required this.restoredSchemaVersion,
    required this.targetSafetyCopyAvailable,
  });

  final int restoredSchemaVersion;
  final bool targetSafetyCopyAvailable;
}
