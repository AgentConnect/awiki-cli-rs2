import 'dart:io';

import 'package:test/test.dart';

void main() {
  test('macOS Podspec links the CocoaPods-selected XCFramework library', () {
    final podspec = File('macos/awiki_im_core.podspec').readAsStringSync();
    const copiedLibrary =
        r'$(PODS_XCFRAMEWORKS_BUILD_DIR)/awiki_im_core/libawiki_im_core.a';

    expect(
      podspec,
      contains("s.vendored_frameworks = 'Frameworks/AwikiImCore.xcframework'"),
    );
    expect(copiedLibrary.allMatches(podspec), hasLength(2));
    expect(podspec, contains('-Wl,-export_dynamic'));
    expect(podspec, isNot(contains('Dir.glob')));
    expect(podspec, isNot(contains('macos_slice')));
    expect(podspec, isNot(contains(r'$(PODS_TARGET_SRCROOT)')));
    expect(podspec, isNot(contains('.xcframework/macos-')));
  });
}
