import 'package:awiki_im_core/awiki_im_core.dart';
import 'package:test/test.dart';

void main() {
  test('device revoke rollout gate defaults off', () {
    const defaults = AwikiImCoreOpenOptions();
    const enabled = AwikiImCoreOpenOptions(
      multiDeviceDeviceRevokeEnabled: true,
    );

    expect(defaults.multiDeviceDeviceRevokeEnabled, isFalse);
    expect(enabled.multiDeviceDeviceRevokeEnabled, isTrue);
  });

  test('device revoke result contains no internal control-plane state', () {
    const result = DeviceRevokeResult(
      did: 'did:wba:example.test:alice',
      targetDeviceId: 'device-member',
      status: DeviceRevokeStatus.revoked,
    );

    expect(result.status, DeviceRevokeStatus.revoked);
    final text = result.toString();
    for (final forbidden in <String>[
      'auth_generation',
      'document_version',
      'document_hash',
      'registry_version',
      'root_proof',
      'admin_proof',
    ]) {
      expect(text, isNot(contains(forbidden)));
    }
  });

  test('device revoke facade shape requires explicit presence result', () {
    expect(_revoke, isA<Function>());
  });
}

Future<DeviceRevokeResult> _revoke(AwikiImCore core) {
  return core.revokeDevice(
    selector: const IdentitySelector.did('did:wba:example.test:alice'),
    targetDeviceId: 'device-member',
    userPresenceConfirmed: true,
  );
}
