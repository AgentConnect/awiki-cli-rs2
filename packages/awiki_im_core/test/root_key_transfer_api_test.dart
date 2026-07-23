import 'dart:convert';
import 'dart:io';

import 'package:awiki_im_core/awiki_im_core.dart';
import 'package:test/test.dart';

void main() {
  test('root-key transfer result contains delivery metadata only', () {
    const result = RootKeyTransferSendResult(
      did: 'did:wba:example.test:alice',
      senderDeviceId: 'device-admin',
      recipientDeviceId: 'device-member',
      messageId: 'root-transfer-message-1',
      acceptedAt: '2026-07-20T01:00:00Z',
    );

    expect(result.messageId, 'root-transfer-message-1');
    expect(result.toString(), isNot(contains('root_private_key')));
    expect(result.toString(), isNot(contains('transport_context')));
    expect(result.toString(), isNot(contains('completion')));
  });

  test('root-key transfer recipient summary contains frozen safe fields', () {
    const recipient = RootKeyTransferRecipientSummary(
      did: 'did:wba:example.test:alice',
      deviceId: 'device-member',
      signingKeyId: 'did:wba:example.test:alice#device-member-signing',
      e2eeKeyId: 'did:wba:example.test:alice#device-member-e2ee',
      registryVersion: 7,
    );

    expect(recipient.deviceId, 'device-member');
    expect(recipient.registryVersion, 7);
    expect(recipient.toString(), isNot(contains('authorization_handle')));
  });

  test('root-key transfer prepare and confirm APIs are client-scoped', () {
    expect(_prepareRootKeyTransferApiShape, isA<Function>());
    expect(_confirmRootKeyTransferApiShape, isA<Function>());
  });

  test(
    'root-key transfer public error contains code and retryability only',
    () {
      const error = RootKeyTransferException(
        code: 'root_transfer.prekey_unavailable',
        retryable: true,
      );

      expect(error.code, 'root_transfer.prekey_unavailable');
      expect(error.retryable, isTrue);
      expect(error.toString(), isNot(contains('prekey_bundle')));
    },
  );

  test('consumes the frozen Dart-safe host fixture and manifest digest', () {
    final manifest = _rootTransferManifest();
    expect(manifest['fixture_id'], 'awiki-root-key-admin-upgrade-v1-20260724');
    final digest = manifest['manifest_digest']! as Map<String, Object?>;
    expect(
      digest['sha256_hex'],
      '9122207bb1903b670beb28e9ad65ab85180b7dc0193b99924230d43d5e66c9a2',
    );

    final host = manifest['host_api']! as Map<String, Object?>;
    final prepared = host['prepare_success']! as Map<String, Object?>;
    final recipientJson = prepared['recipient']! as Map<String, Object?>;
    final preparation = RootKeyTransferPreparation(
      authorizationHandle: const _FixtureAuthorizationHandle(),
      recipient: RootKeyTransferRecipientSummary(
        did: recipientJson['did']! as String,
        deviceId: recipientJson['device_id']! as String,
        signingKeyId: recipientJson['signing_key_id']! as String,
        e2eeKeyId: recipientJson['e2ee_key_id']! as String,
        registryVersion: recipientJson['registry_version']! as int,
      ),
      expiresAt: prepared['expires_at']! as String,
    );
    expect(preparation.recipient.deviceId, 'dev-recipient-step3');
    expect(preparation.toString(), contains('<redacted>'));
    expect(
      preparation.toString(),
      isNot(contains(prepared['authorization_handle']! as String)),
    );

    final receiptJson = host['accepted_receipt']! as Map<String, Object?>;
    final receipt = RootKeyTransferSendResult(
      did: receiptJson['did']! as String,
      senderDeviceId: receiptJson['sender_device_id']! as String,
      recipientDeviceId: receiptJson['recipient_device_id']! as String,
      messageId: receiptJson['message_id']! as String,
      acceptedAt: receiptJson['accepted_at']! as String,
    );
    expect(receipt.messageId, 'root-transfer-fixture-init-001');

    final errorShape = host['error_shape']! as Map<String, Object?>;
    expect(errorShape['fields'], <Object?>['code', 'retryable']);
    expect(errorShape['additional_properties'], isFalse);
    final errors = host['error_union']! as List<Object?>;
    expect(errors, hasLength(15));
    for (final value in errors.cast<Map<String, Object?>>()) {
      final error = RootKeyTransferException(
        code: value['code']! as String,
        retryable: value['retryable']! as bool,
      );
      expect(error.code, startsWith('root_transfer.'));
      expect(error.toString(), isNot(contains('proof')));
    }
  });
}

class _FixtureAuthorizationHandle
    implements RootKeyTransferAuthorizationHandle {
  const _FixtureAuthorizationHandle();
}

Map<String, Object?> _rootTransferManifest() {
  const relative =
      'plan/20260718-awiki-multi-device-implementation/refactor/fixtures/'
      'root-key-admin-upgrade-v1.json';
  final candidates = <File>[File('../../../$relative'), File('../$relative')];
  final file = candidates.firstWhere(
    (candidate) => candidate.existsSync(),
    orElse: () => throw StateError('root-key fixture not found'),
  );
  return jsonDecode(file.readAsStringSync()) as Map<String, Object?>;
}

Future<RootKeyTransferPreparation> _prepareRootKeyTransferApiShape(
  AwikiImClient client,
) {
  return client.rootKeyTransfer.prepare(recipientDeviceId: 'device-member');
}

Future<RootKeyTransferSendResult> _confirmRootKeyTransferApiShape(
  AwikiImClient client,
  RootKeyTransferAuthorizationHandle authorizationHandle,
) {
  return client.rootKeyTransfer.confirmAndSend(
    authorizationHandle: authorizationHandle,
    userPresenceConfirmed: true,
  );
}
