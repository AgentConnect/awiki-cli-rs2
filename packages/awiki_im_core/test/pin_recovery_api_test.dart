import 'package:awiki_im_core/awiki_im_core.dart';
import 'package:test/test.dart';

void main() {
  test('PIN recovery trust is explicitly installed and defaults empty', () {
    const defaults = AwikiImCoreOpenOptions();
    expect(defaults.pinRecoveryTenants, isEmpty);
    const trust = PinRecoveryTenantConfig(
      tenantId: 'awiki',
      deploymentId: 'testing',
      controlBaseUrl: 'https://awiki.info',
      configSigningKeys: {'trusted-kid': 'public-key'},
    );
    const options = AwikiImCoreOpenOptions(pinRecoveryTenants: [trust]);
    expect(
      options.pinRecoveryTenants.single.configSigningKeys['trusted-kid'],
      'public-key',
    );
  });

  test('target generations and epochs retain full unsigned precision', () {
    const target = PinRecoveryTarget(
      targetId: 'target',
      fullHandle: 'alice.awiki.info',
      did: 'did:wba:awiki.info:alice',
      bindingGeneration: '18446744073709551615',
      activeEpoch: '18446744073709551615',
      canEnable: false,
      canRecover: true,
      unavailableReason: null,
    );
    expect(target.activeEpoch, target.bindingGeneration);
    expect(target.canRecover, isTrue);
  });
  test('unavailable authoritative status remains unknown', () {
    const status = PinRecoveryBackupStatus(
      did: 'did:wba:awiki.info:alice',
      fullHandle: null,
      bindingGeneration: null,
      state: PinRecoveryBackupState.unknown,
      activeEpoch: null,
      error: PinRecoveryError(
        code: 'network_unavailable',
        retryable: true,
        retryAfterSeconds: null,
      ),
    );
    expect(status.state, isNot(PinRecoveryBackupState.notEnabled));
    expect(status.error?.code, 'network_unavailable');
  });
}
