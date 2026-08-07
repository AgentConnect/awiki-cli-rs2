import 'package:awiki_im_core/awiki_im_core.dart';
import 'package:test/test.dart';

void main() {
  test('Handle Recovery rollout is explicit and defaults off', () {
    const defaults = AwikiImCoreOpenOptions();
    const enabled = AwikiImCoreOpenOptions(
      multiDeviceHandleRecoveryEnabled: true,
    );

    expect(defaults.multiDeviceHandleRecoveryEnabled, isFalse);
    expect(enabled.multiDeviceHandleRecoveryEnabled, isTrue);
  });

  test('public progress carries impact and Core-authorized epoch reset', () {
    const reset = HandleRecoveryRegistryEpochReset(
      accountUserId: 'user-1',
      ownerIdentityId: 'owner-1',
      handle: 'alice.awiki.info',
      previousDid: 'did:wba:awiki.info:users:alice-old',
      currentDid: 'did:wba:awiki.info:users:alice-new',
      bindingGeneration: '8',
      sourceKind: HandleRecoveryTransitionSourceKind.initiator,
      sourceId: 'operation-1',
    );
    const progress = HandleRecoveryProgress(
      operationId: 'operation-1',
      ownerIdentityId: 'owner-1',
      accountUserId: 'user-1',
      fullHandle: 'alice.awiki.info',
      localPreviousDid: 'did:wba:awiki.info:users:alice-old',
      currentDid: 'did:wba:awiki.info:users:alice-new',
      bindingGeneration: '8',
      stateRootFingerprint: 'sha256:test',
      phase: HandleRecoveryPhase.applied,
      impact: HandleRecoveryImpact(
        localOrdinaryDataWillMigrate: true,
        otherDevicesMustRejoin: true,
        unsupportedE2eeGroupCount: 2,
        unsupportedDidOnlyGroupCount: 3,
      ),
      registryEpochReset: reset,
    );

    expect(progress.registryEpochReset?.sourceId, progress.operationId);
    expect(progress.impact.unsupportedE2eeGroupCount, 2);
    expect(progress.impact.unsupportedDidOnlyGroupCount, 3);
  });

  test('Recovery failure mapping is a closed public enum', () {
    const error = AwikiImCoreException(
      code: 'service_error',
      message: 'handle_recovery_outcome_unknown',
      serviceCode: 'handle_recovery_outcome_unknown',
      handleRecoveryFailureCode: HandleRecoveryFailureCode.outcomeUnknown,
    );

    expect(
      error.handleRecoveryFailureCode,
      HandleRecoveryFailureCode.outcomeUnknown,
    );
  });
}
