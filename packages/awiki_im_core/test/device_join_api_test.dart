import 'package:awiki_im_core/awiki_im_core.dart';
import 'package:test/test.dart';

void main() {
  test('device Join rollout gate defaults off', () {
    const options = AwikiImCoreOpenOptions();
    expect(options.multiDeviceJoinEnabled, isFalse);
  });

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
      role: DeviceJoinRole.member,
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
}
