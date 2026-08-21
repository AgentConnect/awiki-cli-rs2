import 'dart:io';

import 'package:test/test.dart';

void main() {
  test('macOS Podspec force-loads the unique source XCFramework slice', () {
    final podspec = File('macos/awiki_im_core.podspec').readAsStringSync();

    expect(
      podspec,
      contains("s.vendored_frameworks = 'Frameworks/AwikiImCore.xcframework'"),
    );
    expect(podspec, contains('Dir.glob'));
    expect(podspec, contains('macos_libraries.length == 1'));
    expect(podspec, contains('macos_slice'));
    expect(podspec, contains(r'$(PODS_ROOT)/../Flutter/ephemeral/'));
    expect(
      'AwikiImCore.xcframework/#{macos_slice}/libawiki_im_core.a'
          .allMatches(podspec),
      hasLength(1),
    );
    expect(podspec, contains('-Wl,-export_dynamic'));
    expect(podspec, isNot(contains(r'$(PODS_XCFRAMEWORKS_BUILD_DIR)')));
  });
}
