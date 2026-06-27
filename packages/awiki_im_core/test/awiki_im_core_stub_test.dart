import 'package:test/test.dart';
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

  test('thread mark-read model exposes best-effort state', () {
    const result = MarkThreadReadResult(
      updatedCount: 1,
      remoteAcknowledged: false,
      partial: true,
      fallbackUsed: true,
      pendingRemoteAck: true,
      effectiveWatermark: ReadWatermark(lastReadThreadSeq: '42'),
      legacyMessageIds: ['msg-1'],
      warnings: ['Remote read-state mark-read failed'],
    );

    expect(result.updatedCount, 1);
    expect(result.remoteAcknowledged, isFalse);
    expect(result.partial, isTrue);
    expect(result.fallbackUsed, isTrue);
    expect(result.pendingRemoteAck, isTrue);
    expect(result.effectiveWatermark?.lastReadThreadSeq, '42');
    expect(result.legacyMessageIds, ['msg-1']);
    expect(result.warnings.single, contains('Remote'));
  });

  test('thread mark-read API shape remains app-usable', () {
    expect(_markThreadReadApiShape, isA<Function>());
  });

  test('local history API shape remains app-usable', () {
    expect(_localHistoryApiShape, isA<Function>());
  });

  test('sync API models expose no global checkpoint controls', () {
    const deltaRequest = SyncDeltaRequest(
      limit: 100,
      deviceId: 'device-main',
      reason: 'app_resumed',
    );
    expect(deltaRequest.limit, 100);
    expect(deltaRequest.reason, 'app_resumed');

    const deltaResult = SyncDeltaResult(
      eventsApplied: 3,
      pagesFetched: 1,
      lastAppliedEventSeq: '42',
      hasMore: false,
      snapshotRequired: false,
      retentionFloorEventSeq: '10',
      warnings: ['ok'],
    );
    expect(deltaResult.lastAppliedEventSeq, '42');
    expect(deltaResult.snapshotRequired, isFalse);

    const threadRequest = SyncThreadAfterRequest(
      thread: ThreadRef.direct('did:example:bob'),
      afterServerSeq: '991',
      limit: 50,
    );
    expect(threadRequest.afterServerSeq, '991');

    const threadResult = SyncThreadAfterResult(
      messages: [],
      nextAfterServerSeq: '992',
      hasMore: false,
    );
    expect(threadResult.nextAfterServerSeq, '992');
  });

  test('sync API shape remains app-usable', () {
    expect(_syncDeltaApiShape, isA<Function>());
    expect(_syncThreadAfterApiShape, isA<Function>());
  });

  test('realtime options and event models stay transport agnostic', () {
    const options = RealtimeOptions();
    expect(options.reconnect, RealtimeReconnectMode.disabled);
    expect(options.subscriptions, ['messages', 'groups', 'notifications']);

    const event = RealtimeEvent(
      kind: 'connection_state_changed',
      state: 'connected',
      sync: RealtimeSyncHint(
        eventId: 'sev-1',
        eventSeq: '42',
        eventType: 'message.created',
        syncDirty: true,
        gapDetected: false,
      ),
    );
    expect(event.isConnectionState, isTrue);
    expect(event.sync?.eventSeq, '42');
    expect(event.sync?.syncDirty, isTrue);
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

  test('attachment send model exposes upload metadata', () {
    const result = AttachmentSendResult(
      message: SendMessageResult(
        message: Message(
          id: 'msg-1',
          threadKind: 'direct',
          threadId: 'did:example:bob',
          direction: MessageDirection.outgoing,
          sender: 'did:example:alice',
          body: MessageBodyView(unsupportedContentType: 'application/json'),
          metadata: MessageMetadata(),
        ),
        deliveryState: 'sent',
      ),
      targetKind: 'direct',
      targetDid: 'did:example:bob',
      attachment: UploadedAttachment(
        attachmentId: 'att-1',
        filename: 'note.txt',
        mimeType: 'text/plain',
        sizeBytes: 5,
        size: '5',
        digestB64u: 'digest',
        objectUri: 'object://att-1',
        objectEncryptionMode: 'object-e2ee',
        plaintextSizeBytes: 4,
      ),
      manifestJson: '{"attachments":[{"id":"att-1"}]}',
    );

    expect(result.message.message.id, 'msg-1');
    expect(result.attachment.attachmentId, 'att-1');
    expect(result.attachment.objectEncryptionMode, 'object-e2ee');
    expect(result.attachment.plaintextSizeBytes, 4);
    expect(result.manifestJson, contains('att-1'));

    const request = AttachmentSendRequest(
      target: MessageTarget.direct('did:example:bob'),
      input: AttachmentInput.localFile('secret.pdf'),
      security: MessageSecurityMode.e2eeRequired,
    );
    expect(request.security, MessageSecurityMode.e2eeRequired);
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
      pendingWork: GroupSecurePendingWork(pendingNotices: 0, pendingCommits: 0),
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

Future<MarkThreadReadResult> _markThreadReadApiShape(MessageApi api) {
  return api.markThreadRead(
    const ThreadRef.direct('did:example:bob'),
    watermark: const ReadWatermark(lastReadThreadSeq: '42'),
    fallbackMaxMessageIds: 100,
  );
}

Future<MessagePage> _localHistoryApiShape(MessageApi api) {
  return api.localHistory(
    const ThreadRef.direct('did:example:bob'),
    limit: 100,
  );
}

Future<SyncDeltaResult> _syncDeltaApiShape(MessageApi api) {
  return api.syncDelta(
    const SyncDeltaRequest(limit: 100, reason: 'app_resumed'),
  );
}

Future<SyncThreadAfterResult> _syncThreadAfterApiShape(MessageApi api) {
  return api.syncThreadAfter(
    const SyncThreadAfterRequest(
      thread: ThreadRef.direct('did:example:bob'),
      afterServerSeq: '42',
      limit: 100,
    ),
  );
}
