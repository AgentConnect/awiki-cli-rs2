import 'package:awiki_im_core/awiki_im_core.dart';
import 'package:awiki_im_core/src/awiki_im_core_web_stub.dart' as web;
import 'package:test/test.dart';

Future<ActiveSyncAccountBinding> _activeBindingApiShape(AwikiImClient client) =>
    client.activeSyncAccountBinding();

Future<DeleteLocalIdentityResult> _deleteIdentityDataApiShape(
  AwikiImCore core,
) => core.deleteLocalIdentityData(const IdentitySelector.id('identity-alice'));

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
      core.deleteLocalIdentityData(
        const IdentitySelector.id('identity-alice'),
      ),
      throwsA(isA<UnsupportedError>()),
    );
  });
}
