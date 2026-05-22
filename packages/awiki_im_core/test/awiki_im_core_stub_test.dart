import 'package:flutter_test/flutter_test.dart';
import 'package:awiki_im_core/awiki_im_core.dart';

void main() {
  test('config model can be constructed', () {
    const config = AwikiImCoreConfig(
      serviceBaseUrl: 'https://awiki.ai',
      didDomain: 'awiki.ai',
    );
    expect(config.serviceBaseUrl, 'https://awiki.ai');
  });

  test('web/native API exposes disposable core type', () {
    expect(AwikiImCore, isNotNull);
  });

  test('realtime unsupported error shape is stable', () {
    const err = AwikiImCoreException(
      code: 'unsupported_capability',
      message: 'unsupported capability: realtime-runner',
      capability: 'realtime-runner',
    );
    expect(err.capability, 'realtime-runner');
  });
}
