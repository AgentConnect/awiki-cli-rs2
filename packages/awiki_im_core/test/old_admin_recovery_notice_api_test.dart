import 'package:awiki_im_core/awiki_im_core.dart';
import 'package:test/test.dart';

void main() {
  test('old-admin recovery notice contains only safe discovery fields', () {
    const notice = OldAdminRecoveryNotice(
      eventId: 'recovery-event-1',
      recoverySessionId: 'recovery-session-1',
      handle: 'alice.awiki.info',
      oldDid: 'did:wba:awiki.info:user:alice:e1_old',
      requestedAt: '2026-07-20T00:00:00Z',
      cancellableUntil: '2026-07-21T00:00:00Z',
    );

    expect(notice.eventId, 'recovery-event-1');
    final text = notice.toString().toLowerCase();
    for (final forbidden in <String>[
      'sync_checkpoint',
      'token',
      'proof',
      'email',
      'secret',
    ]) {
      expect(text, isNot(contains(forbidden)));
    }
  });

  test('old-admin notice list get and dismiss are typed facade methods', () {
    expect(_list, isA<Function>());
    expect(_get, isA<Function>());
    expect(_dismiss, isA<Function>());
  });
}

Future<List<OldAdminRecoveryNotice>> _list(AwikiImCore core) =>
    core.listOldAdminRecoveryNotices(
      oldIdentity: const IdentitySelector.did(
        'did:wba:awiki.info:user:alice:e1_old',
      ),
    );

Future<OldAdminRecoveryNotice?> _get(AwikiImCore core) =>
    core.getOldAdminRecoveryNotice(
      oldIdentity: const IdentitySelector.did(
        'did:wba:awiki.info:user:alice:e1_old',
      ),
      eventId: 'recovery-event-1',
    );

Future<OldAdminRecoveryNoticeDismissResult> _dismiss(AwikiImCore core) =>
    core.dismissOldAdminRecoveryNotice(
      oldIdentity: const IdentitySelector.did(
        'did:wba:awiki.info:user:alice:e1_old',
      ),
      eventId: 'recovery-event-1',
    );
