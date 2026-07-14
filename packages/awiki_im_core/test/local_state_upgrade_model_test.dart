import 'package:awiki_im_core/awiki_im_core.dart';
import 'package:test/test.dart';

void main() {
  test('upgrade result keeps bootstrap-safe facts and alias mapping', () {
    const result = LocalStateUpgradeResult(
      status: LocalStateUpgradeStatus.completed,
      sourceSchemaVersion: 27,
      targetSchemaVersion: 28,
      migratedPersonas: 2,
      migratedConversations: 3,
      unresolvedMessages: 1,
      aliasCount: 4,
      backupAvailable: true,
      aliasMappings: <LocalStateConversationAliasMapping>[
        LocalStateConversationAliasMapping(
          ownerIdentityId: 'owner-a',
          ownerDid: 'did:wba:example.com:alice',
          legacyConversationId: 'dm:legacy',
          canonicalConversationId: 'dm:canonical',
        ),
      ],
    );

    expect(result.status, LocalStateUpgradeStatus.completed);
    expect(result.sourceSchemaVersion, 27);
    expect(result.targetSchemaVersion, 28);
    expect(result.unresolvedMessages, 1);
    expect(result.backupAvailable, isTrue);
    expect(result.aliasMappings.single.canonicalConversationId, 'dm:canonical');
  });
}
