import 'package:awiki_im_core/awiki_im_core.dart';
import 'package:test/test.dart';

void main() {
  test('multi-device group E2EE rollout gate defaults off', () {
    const defaults = AwikiImCoreOpenOptions();
    const enabled = AwikiImCoreOpenOptions(
      multiDeviceGroupE2eeEnabled: true,
    );

    expect(defaults.multiDeviceGroupE2eeEnabled, isFalse);
    expect(enabled.multiDeviceGroupE2eeEnabled, isTrue);
  });
}
