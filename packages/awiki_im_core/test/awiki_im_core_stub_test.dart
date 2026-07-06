import 'dart:typed_data';

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

  test('identity vault open options remain optional and constructible', () {
    final rootKey = DeviceVaultRootKey.fromList(List<int>.filled(32, 9));
    final options = AwikiImCoreOpenOptions.vaultRequired(
      identitySecretVault: ImCoreSecretVaultOptions(
        rootKey: rootKey,
        vaultDir: '/tmp/awiki-vault',
        workspaceId: 'workspace-a',
        deviceId: 'device-a',
      ),
    );

    expect(
      options.identitySecretStoragePolicy,
      IdentitySecretStoragePolicy.vaultRequired,
    );
    expect(options.identitySecretVault?.vaultDir, '/tmp/awiki-vault');
    expect(options.identitySecretVault?.workspaceId, 'workspace-a');
    expect(options.identitySecretVault?.deviceId, 'device-a');

    const compat = AwikiImCoreOpenOptions.fileCompat();
    expect(
      compat.identitySecretStoragePolicy,
      IdentitySecretStoragePolicy.fileCompat,
    );
    expect(compat.identitySecretVault, isNull);
  });

  test('device vault root key does not leak via toString or mutable input', () {
    final source = Uint8List.fromList(List<int>.filled(32, 7));
    final rootKey = DeviceVaultRootKey(source);
    source[0] = 99;

    expect(rootKey.bytes.first, 7);
    expect(rootKey.bytes, hasLength(32));

    final copy = rootKey.bytes;
    copy[1] = 42;
    expect(rootKey.bytes[1], 7);

    final text = rootKey.toString();
    expect(text, contains('DeviceVaultRootKey'));
    expect(text, contains('<redacted>'));
    expect(text, isNot(contains('7, 7')));
    expect(text, isNot(contains('99')));
  });

  test('identity vault status model exposes only safe status fields', () {
    const status = IdentityVaultStatus(
      identity: IdentitySummary(
        id: 'id-alice',
        did: 'did:example:alice',
        localAlias: 'alice',
        isDefault: true,
        readyForAuth: true,
        readyForMessaging: true,
      ),
      storagePolicy: IdentitySecretStoragePolicy.vaultPreferred,
      selectedBackend: IdentitySecretStorageBackend.vault,
      vaultAvailable: true,
      vaultMetadataPresent: true,
      vaultMetadataVerified: true,
      workspaceId: 'workspace-a',
      deviceId: 'device-a',
      plaintextCompatRetained: false,
    );

    expect(status.identity.did, 'did:example:alice');
    expect(status.storagePolicy, IdentitySecretStoragePolicy.vaultPreferred);
    expect(status.selectedBackend, IdentitySecretStorageBackend.vault);
    expect(status.vaultAvailable, isTrue);
    expect(status.vaultMetadataVerified, isTrue);
    expect(status.plaintextCompatRetained, isFalse);
    expect(status.missing, isEmpty);
    expect(status.warnings, isEmpty);
  });

  test(
    'identity vault migration and verification reports expose safe fields',
    () {
      const status = IdentityVaultStatus(
        identity: IdentitySummary(
          id: 'id-alice',
          did: 'did:example:alice',
          localAlias: 'alice',
          isDefault: true,
          readyForAuth: true,
          readyForMessaging: true,
        ),
        storagePolicy: IdentitySecretStoragePolicy.vaultRequired,
        selectedBackend: IdentitySecretStorageBackend.vault,
        vaultAvailable: true,
        vaultMetadataPresent: true,
        vaultMetadataVerified: true,
        workspaceId: 'workspace-a',
        deviceId: 'device-a',
        plaintextCompatRetained: true,
      );
      final migration = IdentityVaultMigrationReport(
        identity: status.identity,
        status: status,
        migrated: true,
        verified: true,
        plaintextCompatRetained: true,
        warnings: ['plaintext compatibility files are still retained'],
      );
      final verification = IdentityVaultVerificationReport(
        identity: status.identity,
        status: status,
        verified: true,
        warnings: ['plaintext compatibility files are still retained'],
      );

      expect(migration.migrated, isTrue);
      expect(migration.verified, isTrue);
      expect(migration.plaintextCompatRetained, isTrue);
      expect(verification.verified, isTrue);
      expect(migration.identity.did, 'did:example:alice');
      expect(
        verification.status.selectedBackend,
        IdentitySecretStorageBackend.vault,
      );
      expect('$migration $verification', isNot(contains('SecretRef')));
    },
  );

  test('unsupported capability error shape is stable', () {
    const err = AwikiImCoreException(
      code: 'unsupported_capability',
      message: 'unsupported capability: realtime-runner',
      capability: 'realtime-runner',
    );
    expect(err.capability, 'realtime-runner');
  });

  test('service error details are available to app callers', () {
    const err = AwikiImCoreException(
      code: 'service_error',
      message: 'target did is inactive',
      statusCode: 409,
      serviceCode: '1007',
      serviceDataJson: '{"did":"did:example:old","handle":"alice"}',
    );

    expect(err.statusCode, 409);
    expect(err.serviceCode, '1007');
    expect(err.serviceDataJson, '{"did":"did:example:old","handle":"alice"}');
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

  test('conversation mark-read API shape remains app-usable', () {
    expect(_markConversationReadApiShape, isA<Function>());
  });

  test('conversation send API shape remains app-usable', () {
    const text = SendConversationTextRequest(
      conversation: ConversationReadRef(conversationId: 'dm:did:example:bob'),
      text: 'hello',
      clientMessageId: 'msg-client-text',
      idempotencyKey: 'op-client-text',
    );
    expect(text.conversation.conversationId, 'dm:did:example:bob');
    expect(text.security, MessageSecurityMode.defaultPlain);

    const payload = SendConversationPayloadRequest(
      conversation: ConversationReadRef(
        conversationId: 'group:did:example:group',
      ),
      payloadJson: '{"schema":"awiki.agent.mention.v1"}',
      clientMessageId: 'msg-client-payload',
      idempotencyKey: 'op-client-payload',
    );
    expect(payload.conversation.conversationId, 'group:did:example:group');
    expect(payload.security, MessageSecurityMode.defaultPlain);

    expect(_sendConversationTextApiShape, isA<Function>());
    expect(_sendConversationPayloadApiShape, isA<Function>());
  });

  test('conversation attachment send API shape remains app-usable', () {
    const request = SendConversationAttachmentRequest(
      conversation: ConversationReadRef(conversationId: 'dm:did:example:bob'),
      input: AttachmentInput.bytes(
        filename: 'note.txt',
        mimeType: 'text/plain',
        bytes: [104, 105],
      ),
      caption: 'hello',
      clientMessageId: 'msg-client-attachment',
      idempotencyKey: 'op-client-attachment',
    );

    expect(request.conversation.conversationId, 'dm:did:example:bob');
    expect(request.security, MessageSecurityMode.defaultPlain);
    expect(_sendConversationAttachmentApiShape, isA<Function>());
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

Future<MarkThreadReadResult> _markConversationReadApiShape(MessageApi api) {
  return api.markConversationRead(
    const ConversationReadRef(conversationId: 'dm:did:example:bob'),
    watermark: const ReadWatermark(lastReadThreadSeq: '42'),
    fallbackMaxMessageIds: 100,
  );
}

Future<SendMessageResult> _sendConversationTextApiShape(MessageApi api) {
  return api.sendConversationText(
    const SendConversationTextRequest(
      conversation: ConversationReadRef(conversationId: 'dm:did:example:bob'),
      text: 'hello',
      clientMessageId: 'msg-client-text',
      idempotencyKey: 'op-client-text',
    ),
  );
}

Future<SendMessageResult> _sendConversationPayloadApiShape(MessageApi api) {
  return api.sendConversationPayload(
    const SendConversationPayloadRequest(
      conversation: ConversationReadRef(
        conversationId: 'group:did:example:group',
      ),
      payloadJson: '{"schema":"awiki.agent.mention.v1"}',
      clientMessageId: 'msg-client-payload',
      idempotencyKey: 'op-client-payload',
    ),
  );
}

Future<AttachmentSendResult> _sendConversationAttachmentApiShape(
  AttachmentApi api,
) {
  return api.sendConversation(
    const SendConversationAttachmentRequest(
      conversation: ConversationReadRef(conversationId: 'dm:did:example:bob'),
      input: AttachmentInput.bytes(
        filename: 'note.txt',
        mimeType: 'text/plain',
        bytes: [104, 105],
      ),
      clientMessageId: 'msg-client-attachment',
      idempotencyKey: 'op-client-attachment',
    ),
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
