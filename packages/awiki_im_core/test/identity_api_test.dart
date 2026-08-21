import 'package:awiki_im_core/awiki_im_core.dart';
import 'package:awiki_im_core/src/awiki_im_core_web_stub.dart' as web;
import 'package:test/test.dart';

Future<ActiveSyncAccountBinding> _activeBindingApiShape(AwikiImClient client) =>
    client.activeSyncAccountBinding();

Future<DeleteLocalIdentityResult> _deleteIdentityDataApiShape(
  AwikiImCore core,
) => core.deleteLocalIdentityData(const IdentitySelector.id('identity-alice'));

Future<IdentityCustodyStatus> _identityCustodyStatusApiShape(
  AwikiImCore core,
) => core.identityCustodyStatus(const IdentitySelector.id('identity-alice'));

void main() {
  test(
    'active sync account binding exposes exactly the six stable strings',
    () {
      const binding = ActiveSyncAccountBinding(
        ownerIdentityId: 'owner-alice',
        accountId: 'account-alice',
        currentDid: 'did:wba:awiki.info:user:alice',
        protocolDeviceId: 'device-desktop',
        identityGeneration: '184467440737095516160000000000000000001',
        deviceAuthGeneration: '184467440737095516160000000000000000002',
      );

      expect(binding.ownerIdentityId, 'owner-alice');
      expect(binding.accountId, 'account-alice');
      expect(binding.currentDid, 'did:wba:awiki.info:user:alice');
      expect(binding.protocolDeviceId, 'device-desktop');
      expect(
        binding.identityGeneration,
        '184467440737095516160000000000000000001',
      );
      expect(
        binding.deviceAuthGeneration,
        '184467440737095516160000000000000000002',
      );
      expect(_activeBindingApiShape, isA<Function>());
    },
  );

  test('web account binding boundary fails closed as unavailable', () async {
    final client = web.AwikiImClient();

    await expectLater(
      client.activeSyncAccountBinding(),
      throwsA(
        isA<UnsupportedError>().having(
          (error) => error.message,
          'message',
          contains('native Rust backend is not supported'),
        ),
      ),
    );
  });

  test('registered Handle result carries the canonical account id', () {
    const result = HandleRegistrationResult(
      accountId: 'account-alice',
      handle: 'alice.awiki.info',
      method: 'email',
      state: 'registered',
    );

    expect(result.accountId, 'account-alice');
  });

  test('owner data deletion is exposed by the native facade only', () async {
    expect(_deleteIdentityDataApiShape, isA<Function>());
    final core = web.AwikiImCore();

    await expectLater(
      core.deleteLocalIdentityData(const IdentitySelector.id('identity-alice')),
      throwsA(isA<UnsupportedError>()),
    );
  });

  test('identity custody status is independent from legacy vault policy', () {
    const status = IdentityCustodyStatus(
      identity: IdentitySummary(
        id: 'id-alice',
        did: 'did:example:alice',
        isDefault: true,
        readyForAuth: true,
        readyForMessaging: true,
      ),
      backend: IdentityCustodyBackend.anpIdentity,
      state: IdentityCustodyState.active,
      ready: true,
      rootControlAvailable: true,
      pendingOperation: false,
      storeId: 'store-public-id',
      custodyIdentityId: 'custody-public-id',
    );

    expect(status.backend, IdentityCustodyBackend.anpIdentity);
    expect(status.state, IdentityCustodyState.active);
    expect(status.ready, isTrue);
    expect(status.rootControlAvailable, isTrue);
    expect(status.missing, isEmpty);
    expect(_identityCustodyStatusApiShape, isA<Function>());
  });

  test('web identity custody status fails closed as unavailable', () async {
    final core = web.AwikiImCore();

    await expectLater(
      core.identityCustodyStatus(const IdentitySelector.id('identity-alice')),
      throwsA(isA<UnsupportedError>()),
    );
  });
}
