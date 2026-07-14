import 'dart:convert';

import 'package:awiki_im_core/awiki_im_core.dart';
import 'package:test/test.dart';

void main() {
  test('SendPayloadRequest defaults to secure direct', () {
    const request = SendPayloadRequest(
      target: MessageTarget.direct('did:example:daemon'),
      payloadJson:
          '{"schema":"awiki.agent.command.v1","command":"agent.status.query"}',
    );

    expect(request.security, MessageSecurityMode.secureDirect);
    expect(request.waitForFinalAcceptance, isFalse);
    expect(request.delegatedSigning, isNull);
  });

  test('message delegated options remain optional and constructible', () {
    const delegated = DelegatedSigningOptions(
      logicalSenderDid: 'did:example:alice',
      signingVerificationMethod: 'did:example:alice#daemon-key-1',
      signingKeyRef: 'local:daemon-key-1',
      actorAgentDid: 'did:example:daemon',
    );
    const send = SendTextRequest(
      target: MessageTarget.direct('did:example:bob'),
      text: 'hello',
      delegatedSigning: delegated,
    );
    const inbox = InboxHistoryOptions(
      inboxOwnerDid: 'did:example:alice',
      inboxAuthVerificationMethod: 'did:example:alice#daemon-key-1',
      inboxAuthKeyRef: 'local:daemon-key-1',
    );
    const tokenAuth = InboxAuth.scopedInboxToken(
      ScopedInboxToken(token: 'token-1'),
    );

    expect(
      send.delegatedSigning?.signingVerificationMethod,
      endsWith('#daemon-key-1'),
    );
    expect(inbox.inboxOwnerDid, 'did:example:alice');
    expect(tokenAuth, isA<ScopedInboxTokenAuth>());
  });

  test('MessageBodyView exposes payloadJson', () {
    const payload =
        '{"schema":"awiki.agent.status.v1","status_scope":"snapshot"}';
    const body = MessageBodyView(kind: 'payload', payloadJson: payload);

    expect(body.text, isNull);
    expect(body.kind, 'payload');
    expect(body.payloadJson, payload);
  });

  test('conversation identity surface is additive and SDK-generic', () {
    const identity = ConversationIdentity(
      conversationId: 'dm:peer-scope:v1:abc',
      canonicalThreadKind: 'direct',
      canonicalThreadId: 'dm:peer-scope:v1:abc',
      storageThreadRef: ConversationStorageThreadRef(
        kind: 'thread',
        id: 'dm:peer-scope:v1:abc',
      ),
      aliases: [
        ConversationAlias(
          kind: 'thread',
          id: 'dm:did:example:alice:did:example:bob',
          source: ConversationAliasSource.oldFlutterSortedDirect,
        ),
      ],
      identityScope: ConversationIdentityScope.direct,
      migrationState: ConversationMigrationState.canonical,
    );
    const metadata = MessageMetadata(conversationIdentity: identity);
    const legacyMetadata = MessageMetadata();

    expect(
      metadata.conversationIdentity?.conversationId,
      'dm:peer-scope:v1:abc',
    );
    expect(metadata.conversationIdentity?.storageThreadRef.kind, 'thread');
    expect(
      metadata.conversationIdentity?.aliases.single.source,
      ConversationAliasSource.oldFlutterSortedDirect,
    );
    expect(
      metadata.conversationIdentity?.identityScope,
      ConversationIdentityScope.direct,
    );
    expect(legacyMetadata.conversationIdentity, isNull);
  });

  test('conversation read model requests are conversation-id keyed', () {
    const conversation = ConversationReadRef(
      conversationId: 'dm:peer-scope:v1:abc',
    );
    const sync = SyncConversationAfterRequest(
      conversation: conversation,
      afterServerSeq: '992',
      limit: 25,
    );

    expect(conversation.conversationId, 'dm:peer-scope:v1:abc');
    expect(sync.conversation, same(conversation));
    expect(sync.afterServerSeq, '992');
    expect(sync.limit, 25);
  });

  test('conversation projections expose canonical identity separately', () {
    const conversation = Conversation(
      conversationId: 'dm:persona:peer-1',
      peerPersonaId: 'peer-1',
      resolutionState: ConversationResolutionState.resolved,
      threadKind: 'direct',
      threadId: 'did:example:peer-device-2',
      unreadCount: 0,
      messageCount: 0,
    );
    const groupSnapshot = ConversationSnapshotItem(
      conversationId: 'group:did:example:group-1',
      canonicalGroupDid: 'did:example:group-1',
      resolutionState: ConversationResolutionState.resolved,
      threadKind: 'group',
      threadId: 'wire-group-thread-7',
      unreadCount: 0,
      messageCount: 0,
    );
    const member = GroupMember(
      membershipId: 'membership-1',
      peerPersonaId: 'peer-1',
      did: 'did:example:peer-device-2',
      credentialDid: 'did:example:peer-device-2',
    );

    expect(conversation.conversationId, 'dm:persona:peer-1');
    expect(conversation.peerPersonaId, 'peer-1');
    expect(conversation.threadId, 'did:example:peer-device-2');
    expect(groupSnapshot.canonicalGroupDid, 'did:example:group-1');
    expect(member.membershipId, 'membership-1');
    expect(member.peerPersonaId, 'peer-1');
    expect(member.credentialDid, member.did);
  });

  test('agent control helpers identify command and status payloads', () {
    const commandPayload =
        '{"schema":"awiki.agent.command.v1","command":"runtime.agent.create"}';
    const statusPayload =
        '{"schema":"awiki.agent.status.v1","status_scope":"snapshot"}';

    expect(isAwikiAgentCommandPayload(commandPayload), isTrue);
    expect(isAwikiAgentStatusPayload(statusPayload), isTrue);
    expect(isAwikiAgentControlPayload(commandPayload), isTrue);
    expect(isAwikiAgentControlPayload(statusPayload), isTrue);
    expect(awikiPayloadSchema(commandPayload), AwikiAgentSchemas.commandV1);
    expect(
      decodePayloadObject(statusPayload)?['schema'],
      AwikiAgentSchemas.statusV1,
    );
  });

  test('agent control helpers tolerate invalid payloads', () {
    expect(isAwikiAgentControlPayload(null), isFalse);
    expect(isAwikiAgentControlPayload('not-json'), isFalse);
    expect(isAwikiAgentControlPayload('[]'), isFalse);
    expect(
      isAwikiAgentControlPayload('{"command":"agent.status.query"}'),
      isFalse,
    );
    expect(awikiPayloadSchema('{"schema":""}'), isNull);
    expect(decodePayloadObject('not-json'), isNull);
  });

  test('schema-less ANP P9 mention payload validates without schema', () {
    const payload = MessageMentionPayload(
      text: '@agents 请总结这段讨论',
      mentions: <MessageMention>[
        MessageMention(
          id: 'men_1',
          range: MessageMentionRange(start: 0, end: 7),
          target: MessageMentionTarget.groupSelector(
            MessageMentionSelector.agents,
          ),
        ),
      ],
    );

    final payloadJson = payload.toPayloadJson();

    validateMessagePayloadJson(payloadJson);
    validateMessageMentionPayloadJson(payloadJson);
    expect(decodePayloadObject(payloadJson)?['schema'], isNull);
    final decoded = decodeMessageMentionPayloadJson(payloadJson);
    expect(decoded.mentions.single.mentionRole, MessageMentionRole.addressee);
    expect(
      decoded.mentions.single.target,
      isA<GroupSelectorMessageMentionTarget>(),
    );
  });

  test('ANP P9 mention payload keeps display name out of identity checks', () {
    const payload = MessageMentionPayload(
      text: '@Alice hello',
      mentions: <MessageMention>[
        MessageMention(
          id: 'men_1',
          range: MessageMentionRange(start: 0, end: 6),
          target: MessageMentionTarget.human(
            did: 'did:wba:example.com:user:alice',
            displayName: 'Alice',
          ),
          mentionRole: MessageMentionRole.cc,
        ),
      ],
    );

    final decoded = decodeMessageMentionPayloadJson(payload.toPayloadJson());
    final target = decoded.mentions.single.target;

    expect(decoded.mentions.single.mentionRole, MessageMentionRole.cc);
    expect(target, isA<HumanMessageMentionTarget>());
    expect(
      (target as HumanMessageMentionTarget).did,
      'did:wba:example.com:user:alice',
    );
    expect(target.displayName, 'Alice');
  });

  test('ANP P9 mention payload rejects forbidden sender and proof fields', () {
    for (final field in <String>[
      'sender',
      'sender_did',
      'from',
      'actor_did',
      'auth',
      'origin_proof',
      'proof',
      'signature',
    ]) {
      final payload = <String, Object?>{
        'text': '@agents hi',
        'mentions': <Object?>[
          <String, Object?>{
            'id': 'men_1',
            'range': <String, Object?>{
              'start': 0,
              'end': 7,
              'unit': messageMentionRangeUnitUnicodeCodePoint,
            },
            'target': <String, Object?>{
              'kind': 'group_selector',
              'selector': 'agents',
            },
            field: 'bad',
          },
        ],
      };

      expect(
        () => validateMessageMentionPayloadJson(jsonEncode(payload)),
        throwsA(isA<AwikiImCoreException>()),
        reason: '$field should be rejected',
      );
    }
  });

  test('ANP P9 mention payload rejects invalid range and selector shape', () {
    const invalidRange = MessageMentionPayload(
      text: '@agents hi',
      mentions: <MessageMention>[
        MessageMention(
          id: 'men_1',
          range: MessageMentionRange(start: 0, end: 99),
          target: MessageMentionTarget.groupSelector(
            MessageMentionSelector.agents,
          ),
        ),
      ],
    );
    expect(invalidRange.toPayloadJson, throwsA(isA<AwikiImCoreException>()));

    final selectorWithDid = <String, Object?>{
      'text': '@agents hi',
      'mentions': <Object?>[
        <String, Object?>{
          'id': 'men_1',
          'range': <String, Object?>{
            'start': 0,
            'end': 7,
            'unit': messageMentionRangeUnitUnicodeCodePoint,
          },
          'target': <String, Object?>{
            'kind': 'group_selector',
            'selector': 'agents',
            'did': 'did:wba:example.com:agent:x',
          },
        },
      ],
    };
    expect(
      () => validateMessageMentionPayloadJson(jsonEncode(selectorWithDid)),
      throwsA(isA<AwikiImCoreException>()),
    );
  });
}
