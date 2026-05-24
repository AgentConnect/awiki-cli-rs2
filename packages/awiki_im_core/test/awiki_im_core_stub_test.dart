import 'package:flutter_test/flutter_test.dart';
import 'package:awiki_im_core/awiki_im_core.dart';

void main() {
  test('config model can be constructed', () {
    const config = AwikiImCoreConfig(
      serviceBaseUrl: 'https://awiki.ai',
      didDomain: 'awiki.ai',
      mailServiceEndpoint: 'https://mail.awiki.ai',
    );
    expect(config.serviceBaseUrl, 'https://awiki.ai');
    expect(config.mailServiceEndpoint, 'https://mail.awiki.ai');
  });

  test('web/native API exposes disposable core type', () {
    expect(AwikiImCore, isNotNull);
  });

  test('unsupported capability error shape is stable', () {
    const err = AwikiImCoreException(
      code: 'unsupported_capability',
      message: 'unsupported capability: realtime-runner',
      capability: 'realtime-runner',
    );
    expect(err.capability, 'realtime-runner');
  });

  test('realtime options and event models stay transport agnostic', () {
    const options = RealtimeOptions();
    expect(options.reconnect, RealtimeReconnectMode.disabled);
    expect(options.subscriptions, ['messages', 'groups', 'notifications']);

    const event = RealtimeEvent(
      kind: 'connection_state_changed',
      state: 'connected',
    );
    expect(event.isConnectionState, isTrue);
  });

  test('email models can be constructed without CLI-only fields', () {
    const request = SendEmailRequest(
      to: ['bob@awiki.ai'],
      subject: 'Hello',
      bodyText: 'Body',
    );
    expect(request.to, ['bob@awiki.ai']);

    const page = EmailMessageSummaryPage(
      items: [
        EmailMessageSummary(
          id: 'mail-1',
          subject: 'Hello',
          unread: true,
          hasAttachments: false,
        ),
      ],
      hasMore: false,
    );
    expect(page.items.single.id, 'mail-1');
  });

  test('secure e2ee API shape is stable', () {
    const request = SendTextRequest(
      target: MessageTarget.direct('did:example:bob'),
      text: 'hello',
      security: MessageSecurityMode.e2eeRequired,
    );
    expect(request.security, MessageSecurityMode.e2eeRequired);

    const direct = DirectSecureStatus(
      peer: 'did:example:bob',
      state: DirectSecureState.ready,
      canSendSecure: true,
      pendingOutboxCount: 0,
    );
    expect(direct.canSendSecure, isTrue);

    const group = GroupSecureStatus(
      group: 'did:example:group',
      state: GroupSecureState.ready,
      canSendSecure: true,
      localReadiness: GroupSecureLocalReadiness(
        hasLocalState: true,
        hasActiveMembership: true,
      ),
      pendingWork: GroupSecurePendingWork(
        pendingNotices: 0,
        pendingCommits: 0,
      ),
    );
    expect(group.pendingWork.pendingNotices, 0);

    const outbox = SecureOutboxEntry(
      id: 'outbox-1',
      target: MessageTarget.direct('did:example:bob'),
      messageKind: 'text',
      status: SecureOutboxStatus.failed,
      attemptCount: 1,
    );
    expect(outbox.status, SecureOutboxStatus.failed);
  });
}
