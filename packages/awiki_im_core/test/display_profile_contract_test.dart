import 'package:awiki_im_core/awiki_im_core.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('display profile cache provenance is explicit and backward-safe', () {
    const current = DisplayProfile(cacheHit: true);
    const legacy = DisplayProfile(
      cacheHit: true,
      isStale: true,
      legacyFallback: true,
    );

    expect(current.isStale, isFalse);
    expect(current.legacyFallback, isFalse);
    expect(legacy.isStale, isTrue);
    expect(legacy.legacyFallback, isTrue);
  });
}
