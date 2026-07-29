import 'dart:io';

import 'package:awiki_im_core/awiki_im_core.dart';
import 'package:test/test.dart';

void main() {
  test('profile keeps account and WNS versions independent', () {
    const profile = UserProfile(
      subject: 'did:wba:example.test:alice',
      tags: <String>[],
      profileVersion: '18446744073709551616',
      versionId: 'wns-profile-7',
    );

    expect(profile.profileVersion, '18446744073709551616');
    expect(profile.versionId, 'wns-profile-7');
  });

  test('generated profile bridge preserves both versions as strings', () {
    final generatedDto = File(
      'lib/src/generated/dto/profile.dart',
    ).readAsStringSync();
    final generatedBridge = File(
      'lib/src/generated/frb_generated.dart',
    ).readAsStringSync();
    final nativeFacade = File(
      'lib/src/awiki_im_core_native.dart',
    ).readAsStringSync();

    expect(generatedDto, contains('final String? profileVersion;'));
    expect(generatedDto, contains('final String? versionId;'));
    expect(
      generatedBridge,
      contains('profileVersion: dco_decode_opt_String(arr[13])'),
    );
    expect(
      generatedBridge,
      contains('versionId: dco_decode_opt_String(arr[14])'),
    );
    expect(nativeFacade, contains('profileVersion: profileVersion'));
    expect(nativeFacade, contains('versionId: versionId'));
  });
}
