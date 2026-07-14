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
  });

  final LocalStateUpgradeStatus status;
  final int sourceSchemaVersion;
  final int targetSchemaVersion;
  final int migratedPersonas;
  final int migratedConversations;
  final int unresolvedMessages;
  final int aliasCount;
  final bool backupAvailable;
}
