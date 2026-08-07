import 'dart:io';

import 'package:awiki_im_core/awiki_im_core.dart';
import 'package:test/test.dart';

Future<DeviceJoinProgress> _localDeviceJoinVerificationProgressApiShape(
  AwikiImCore core,
) => core.localDeviceJoinVerificationProgress(
  selector: const IdentitySelector.defaultIdentity(),
  joinSessionId: 'join-safe-id',
);

void main() {
  test('account verification grant has a write-only redacted surface', () {
    const token = 'verification-grant-must-never-appear';
    final grant = DeviceJoinAccountVerificationGrant.fromToken(token);

    expect(grant.toString(), contains('<redacted>'));
    expect(grant.toString(), isNot(contains(token)));
  });

  test('invalid grant errors do not echo input', () {
    const invalid = '   \t';
    expect(
      () => DeviceJoinAccountVerificationGrant.fromToken(invalid),
      throwsA(
        isA<ArgumentError>().having(
          (error) => error.toString(),
          'message',
          isNot(contains(invalid)),
        ),
      ),
    );
  });

  test('approval prompt string does not reveal handle or SAS', () {
    const prompt = DeviceJoinApprovalPrompt(
      approvalHandle: 'approval-handle-must-never-appear',
      joinSessionId: 'join-safe-id',
      sas: '123456',
      expiresAt: '2026-07-19T12:00:00Z',
    );

    expect(prompt.toString(), contains('join-safe-id'));
    expect(
      prompt.toString(),
      isNot(contains('approval-handle-must-never-appear')),
    );
    expect(prompt.toString(), isNot(contains('123456')));
  });

  test('host session model contains no internal version or hash fields', () {
    const session = DeviceJoinSessionSummary(
      joinSessionId: 'join-safe-id',
      did: 'did:wba:example.test:alice',
      protocolDeviceId: 'device-new',
      side: DeviceJoinSide.newDevice,
      phase: DeviceJoinPhase.pending,
      expiresAt: '2026-07-19T12:00:00Z',
    );

    expect(session.joinSessionId, 'join-safe-id');
    expect(session.toString(), isNot(contains('document_hash')));
    expect(session.toString(), isNot(contains('registry_version')));
  });

  test('registry snapshot keeps monotonic versions as decimal strings', () {
    const snapshot = DeviceJoinRegistrySnapshot(
      did: 'did:wba:example.test:alice',
      registryVersion: '18446744073709551615',
      devices: [
        DeviceRegistryAuthorizedDeviceSummary(
          protocolDeviceId: 'device-current',
          signingKeyId: 'did:wba:example.test:alice#device-current-sign',
          e2eeKeyId: 'did:wba:example.test:alice#device-current-e2ee',
          status: DeviceJoinAuthorizationStatus.active,
          role: DeviceJoinRole.admin,
          managementReady: true,
          isCurrent: true,
          authGeneration: '18446744073709551615',
        ),
      ],
    );

    expect(snapshot.registryVersion, '18446744073709551615');
    expect(snapshot.devices.single.authGeneration, '18446744073709551615');
    expect(snapshot.devices.single.isCurrent, isTrue);
  });

  test('generated Registry bridge preserves String version fields', () {
    final generatedDto = File(
      'lib/src/generated/dto/identity.dart',
    ).readAsStringSync();
    final generatedBridge = File(
      'lib/src/generated/frb_generated.dart',
    ).readAsStringSync();
    final nativeFacade = File(
      'lib/src/awiki_im_core_native.dart',
    ).readAsStringSync();

    expect(generatedDto, contains('final String registryVersion;'));
    expect(generatedDto, contains('final String authGeneration;'));
    expect(
      generatedDto,
      contains('List<DartDeviceRegistryAuthorizedDeviceSummary> devices;'),
    );
    expect(generatedDto, isNot(contains('final int registryVersion;')));
    expect(generatedDto, isNot(contains('final int authGeneration;')));
    expect(
      generatedBridge,
      contains('registryVersion: dco_decode_String(arr[1])'),
    );
    expect(
      generatedBridge,
      contains('authGeneration: dco_decode_String(arr[7])'),
    );
    expect(nativeFacade, contains('registryVersion: registryVersion'));
    expect(nativeFacade, contains('authGeneration: authGeneration'));
  });

  test('verified request notice exposes only host-safe fields', () {
    const notice = DeviceJoinRequestNotice(
      eventId: 'event-join-1',
      joinSessionId: 'join-safe-id',
      did: 'did:wba:example.test:alice',
      protocolDeviceId: 'device-new',
      candidateKeyFingerprint: 'fingerprint-safe',
      issuedAt: '2026-07-23T12:00:00Z',
      expiresAt: '2026-07-23T12:10:00Z',
      state: DeviceJoinRemoteState.pending,
      claimedByCurrentDevice: false,
      canStartVerification: true,
    );

    expect(notice.canStartVerification, isTrue);
    expect(notice.toString(), contains('event-join-1'));
    for (final forbidden in [
      'join_request_proof',
      'admin_proof',
      'challenge_ciphertext',
      'pairing_private_key',
      'shared_secret',
      'token',
      'sas',
    ]) {
      expect(notice.toString(), isNot(contains(forbidden)));
    }
  });

  test('remote Join state is a closed seven-state set', () {
    expect(DeviceJoinRemoteState.values, [
      DeviceJoinRemoteState.pending,
      DeviceJoinRemoteState.challengeSent,
      DeviceJoinRemoteState.responseVerified,
      DeviceJoinRemoteState.consumed,
      DeviceJoinRemoteState.cancelled,
      DeviceJoinRemoteState.rejected,
      DeviceJoinRemoteState.expired,
    ]);
  });

  test('local verification progress is exposed on the stable API', () {
    expect(_localDeviceJoinVerificationProgressApiShape, isA<Function>());
  });

  test('verification progress string redacts short-lived SAS', () {
    const progress = DeviceJoinProgress(
      session: DeviceJoinSessionSummary(
        joinSessionId: 'join-safe-id',
        did: 'did:wba:example.test:alice',
        protocolDeviceId: 'device-new',
        side: DeviceJoinSide.admin,
        phase: DeviceJoinPhase.responseVerified,
        expiresAt: '2026-07-23T12:10:00Z',
      ),
      remoteState: DeviceJoinRemoteState.responseVerified,
      sas: '123456',
    );

    expect(progress.toString(), contains('join-safe-id'));
    expect(progress.toString(), contains('<redacted>'));
    expect(progress.toString(), isNot(contains('123456')));
  });
}
