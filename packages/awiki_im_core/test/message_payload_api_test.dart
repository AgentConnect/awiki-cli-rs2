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
