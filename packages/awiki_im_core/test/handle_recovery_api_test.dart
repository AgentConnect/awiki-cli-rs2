import 'dart:convert';

import 'package:awiki_im_core/awiki_im_core.dart';
import 'package:test/test.dart';

void main() {
  test('Handle Recovery rollout gate defaults off', () {
    const defaults = AwikiImCoreOpenOptions();
    const enabled = AwikiImCoreOpenOptions(
      multiDeviceHandleRecoveryEnabled: true,
    );

    expect(defaults.multiDeviceHandleRecoveryEnabled, isFalse);
    expect(enabled.multiDeviceHandleRecoveryEnabled, isTrue);
  });

  test('begin and finalize grants are distinct write-only redacted types', () {
    const beginToken = 'begin-token-must-never-appear';
    const finalizeToken = 'finalize-token-must-never-appear';
    final begin = HandleRecoveryBeginVerificationGrant.fromToken(beginToken);
    final finalize = HandleRecoveryFinalizeVerificationGrant.fromToken(
      finalizeToken,
    );

    expect(begin.runtimeType, isNot(finalize.runtimeType));
    expect(begin.toString(), contains('<redacted>'));
    expect(finalize.toString(), contains('<redacted>'));
    expect(begin.toString(), isNot(contains(beginToken)));
    expect(finalize.toString(), isNot(contains(finalizeToken)));
    expect(() => jsonEncode(begin), throwsA(isA<JsonUnsupportedObjectError>()));
    expect(
      () => jsonEncode(finalize),
      throwsA(isA<JsonUnsupportedObjectError>()),
    );
  });

  test('invalid grants reject without echoing their input', () {
    const invalid = '   \t';
    for (final build in <Object Function()>[
      () => HandleRecoveryBeginVerificationGrant.fromToken(invalid),
      () => HandleRecoveryFinalizeVerificationGrant.fromToken(invalid),
    ]) {
      expect(
        build,
        throwsA(
          isA<ArgumentError>().having(
            (error) => error.toString(),
            'message',
            isNot(contains(invalid)),
          ),
        ),
      );
    }
  });

  test('progress exposes local status without internal checkpoints', () {
    const progress = HandleRecoveryProgress(
      recoverySessionId: 'recovery-safe-id',
      handle: 'alice.awiki.info',
      oldDid: 'did:wba:awiki.info:user:alice:e1_old',
      side: HandleRecoverySide.requester,
      phase: HandleRecoveryPhase.cooling,
      coolingUntil: '2026-07-21T00:00:00Z',
      expiresAt: '2026-07-22T00:00:00Z',
      canCancelFromThisDevice: false,
      localActivationPending: false,
    );

    expect(progress.recoverySessionId, 'recovery-safe-id');
    final text = progress.toString();
    expect(text, isNot(contains('account_verification_token')));
    expect(text, isNot(contains('reconfirmation_token')));
    expect(text, isNot(contains('document_hash')));
    expect(text, isNot(contains('registry_version')));
  });

  test('Recovery lifecycle facade shape remains typed', () {
    expect(_localSessions, isA<Function>());
    expect(_begin, isA<Function>());
    expect(_cancel, isA<Function>());
    expect(_finalize, isA<Function>());
    expect(_resume, isA<Function>());
    expect(_markComplete, isA<Function>());
  });
}

Future<List<HandleRecoveryProgress>> _localSessions(AwikiImCore core) {
  return core.localHandleRecoverySessions();
}

Future<HandleRecoveryProgress> _begin(
  AwikiImCore core,
  HandleRecoveryBeginVerificationGrant grant,
) {
  return core.beginHandleRecovery(
    handle: 'alice.awiki.info',
    verificationGrant: grant,
  );
}

Future<HandleRecoveryCancelResult> _cancel(AwikiImCore core) {
  return core.cancelHandleRecovery(
    oldIdentity: const IdentitySelector.did('did:example:old'),
    recoverySessionId: 'recovery-safe-id',
    userPresenceConfirmed: true,
  );
}

Future<HandleRecoveryFinalizeResult> _finalize(
  AwikiImCore core,
  HandleRecoveryFinalizeVerificationGrant grant,
) {
  return core.finalizeHandleRecovery(
    recoverySessionId: 'recovery-safe-id',
    verificationGrant: grant,
    userPresenceConfirmed: true,
  );
}

Future<IdentitySummary> _resume(AwikiImCore core) {
  return core.resumeHandleRecoveryActivation('recovery-safe-id');
}

Future<void> _markComplete(AwikiImCore core) {
  return core.markHandleRecoveryActivationComplete('recovery-safe-id');
}
