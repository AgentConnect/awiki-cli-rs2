import 'package:awiki_im_core/awiki_im_core.dart';
import 'package:test/test.dart';

void main() {
  test('root-key transfer rollout gate defaults off', () {
    const defaults = AwikiImCoreOpenOptions();
    const enabled = AwikiImCoreOpenOptions(
      multiDeviceRootTransferEnabled: true,
    );

    expect(defaults.multiDeviceRootTransferEnabled, isFalse);
    expect(enabled.multiDeviceRootTransferEnabled, isTrue);
  });

  test('root-key transfer result contains delivery metadata only', () {
    const result = RootKeyTransferSendResult(
      did: 'did:wba:example.test:alice',
      senderDeviceId: 'device-admin',
      recipientDeviceId: 'device-member',
      messageId: 'root-control-message-1',
      acceptedAt: '2026-07-20T01:00:00Z',
    );

    expect(result.messageId, 'root-control-message-1');
    expect(result.toString(), isNot(contains('root_private_key')));
    expect(result.toString(), isNot(contains('transport_context')));
    expect(result.toString(), isNot(contains('completion')));
  });

  test('root-key transfer summary contains restart-safe status only', () {
    const summary = RootKeyTransferSummary(
      did: 'did:wba:example.test:alice',
      messageId: 'root-control-message-1',
      senderDeviceId: 'device-admin',
      recipientDeviceId: 'device-member',
      status: RootKeyTransferStatus.awaitingImport,
      createdAt: '2026-07-20T00:59:00Z',
      acceptedAt: '2026-07-20T01:00:00Z',
      retryable: true,
    );

    expect(summary.status, RootKeyTransferStatus.awaitingImport);
    expect(summary.retryable, isTrue);
    expect(summary.completedAt, isNull);
    expect(summary.toString(), isNot(contains('root_private_key')));
    expect(summary.toString(), isNot(contains('transport_context')));
  });

  test('root-key transfer list and exact retry APIs are public', () {
    expect(_listRootKeyTransfersApiShape, isA<Function>());
    expect(_retryRootKeyTransferApiShape, isA<Function>());
  });
}

Future<List<RootKeyTransferSummary>> _listRootKeyTransfersApiShape(
  AwikiImCore core,
  IdentitySelector selector,
) {
  return core.listRootKeyTransfers(selector: selector);
}

Future<RootKeyTransferSummary> _retryRootKeyTransferApiShape(
  AwikiImCore core,
  IdentitySelector selector,
) {
  return core.retryRootKeyTransfer(
    selector: selector,
    messageId: 'root-control-message-1',
    userPresenceConfirmed: true,
  );
}
