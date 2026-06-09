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
}
