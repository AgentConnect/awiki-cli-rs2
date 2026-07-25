import 'package:awiki_im_core/awiki_im_core.dart';
import 'package:test/test.dart';

void main() {
  test('multi-device Direct rollout gate is default-off and independent', () {
    const defaults = AwikiImCoreOpenOptions();
    const directOnly = AwikiImCoreOpenOptions(
      multiDeviceDirectE2eeEnabled: true,
    );

    expect(defaults.multiDeviceDirectE2eeEnabled, isFalse);
    expect(directOnly.multiDeviceDirectE2eeEnabled, isTrue);
    expect(directOnly.multiDeviceDeviceRevokeEnabled, isFalse);
    expect(directOnly.multiDeviceGroupE2eeEnabled, isFalse);
  });
}
