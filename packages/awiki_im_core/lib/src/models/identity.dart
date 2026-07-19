import 'config.dart';

sealed class IdentitySelector {
  const IdentitySelector();

  const factory IdentitySelector.defaultIdentity() = DefaultIdentitySelector;
  const factory IdentitySelector.id(String id) = IdIdentitySelector;
  const factory IdentitySelector.did(String did) = DidIdentitySelector;
  const factory IdentitySelector.handle(String handle) = HandleIdentitySelector;
  const factory IdentitySelector.localAlias(String alias) =
      LocalAliasIdentitySelector;
}

class DefaultIdentitySelector extends IdentitySelector {
  const DefaultIdentitySelector();
}

class IdIdentitySelector extends IdentitySelector {
  const IdIdentitySelector(this.id);
  final String id;
}

class DidIdentitySelector extends IdentitySelector {
  const DidIdentitySelector(this.did);
  final String did;
}

class HandleIdentitySelector extends IdentitySelector {
  const HandleIdentitySelector(this.handle);
  final String handle;
}

class LocalAliasIdentitySelector extends IdentitySelector {
  const LocalAliasIdentitySelector(this.alias);
  final String alias;
}

class IdentitySummary {
  const IdentitySummary({
    required this.id,
    required this.did,
    this.handle,
    this.displayName,
    this.localAlias,
    this.deviceId,
    required this.isDefault,
    required this.readyForAuth,
    required this.readyForMessaging,
    this.missing = const [],
  });

  final String id;
  final String did;
  final String? handle;
  final String? displayName;
  final String? localAlias;
  final String? deviceId;
  final bool isDefault;
  final bool readyForAuth;
  final bool readyForMessaging;
  final List<String> missing;
}

enum IdentityDeviceMode { legacy, vNext }

enum IdentityDeviceRole { member, admin }

enum IdentityDeviceReadiness {
  legacy,
  memberReady,
  adminAwaitingRoot,
  adminReady,
  blocked,
}

class IdentityDeviceSummary {
  const IdentityDeviceSummary({
    required this.identity,
    required this.mode,
    this.protocolDeviceId,
    this.role,
    this.signingKeyId,
    this.e2eeKeyId,
    required this.readiness,
    this.blockedReason,
  });

  final IdentitySummary identity;
  final IdentityDeviceMode mode;
  final String? protocolDeviceId;
  final IdentityDeviceRole? role;
  final String? signingKeyId;
  final String? e2eeKeyId;
  final IdentityDeviceReadiness readiness;
  final String? blockedReason;
}

enum IdentitySecretStorageBackend { fileCompat, vault }

class IdentityVaultStatus {
  const IdentityVaultStatus({
    required this.identity,
    required this.storagePolicy,
    required this.selectedBackend,
    required this.vaultAvailable,
    required this.vaultMetadataPresent,
    required this.vaultMetadataVerified,
    this.workspaceId,
    this.deviceId,
    this.plaintextCompatRetained,
    this.missing = const [],
    this.warnings = const [],
  });

  final IdentitySummary identity;
  final IdentitySecretStoragePolicy storagePolicy;
  final IdentitySecretStorageBackend selectedBackend;
  final bool vaultAvailable;
  final bool vaultMetadataPresent;
  final bool vaultMetadataVerified;
  final String? workspaceId;
  final String? deviceId;
  final bool? plaintextCompatRetained;
  final List<String> missing;
  final List<String> warnings;
}

class IdentityVaultMigrationReport {
  const IdentityVaultMigrationReport({
    required this.identity,
    required this.status,
    required this.migrated,
    required this.verified,
    required this.plaintextCompatRetained,
    this.warnings = const [],
  });

  final IdentitySummary identity;
  final IdentityVaultStatus status;
  final bool migrated;
  final bool verified;
  final bool plaintextCompatRetained;
  final List<String> warnings;
}

class IdentityVaultVerificationReport {
  const IdentityVaultVerificationReport({
    required this.identity,
    required this.status,
    required this.verified,
    this.warnings = const [],
  });

  final IdentitySummary identity;
  final IdentityVaultStatus status;
  final bool verified;
  final List<String> warnings;
}

class InitialProfile {
  const InitialProfile({this.displayName, this.avatarUrl});

  final String? displayName;
  final String? avatarUrl;
}

class DefaultIdentityChange {
  const DefaultIdentityChange({
    this.previous,
    required this.next,
    required this.requiresDefaultIdentityWrite,
    this.warnings = const [],
  });

  final IdentitySummary? previous;
  final IdentitySummary next;
  final bool requiresDefaultIdentityWrite;
  final List<String> warnings;
}

class DeleteLocalIdentityResult {
  const DeleteLocalIdentityResult({
    required this.deleted,
    required this.wasDefault,
    this.nextDefault,
    this.warnings = const [],
  });

  final IdentitySummary deleted;
  final bool wasDefault;
  final IdentitySummary? nextDefault;
  final List<String> warnings;
}

class DaemonSubkeyPrivatePackage {
  const DaemonSubkeyPrivatePackage({
    required this.schema,
    required this.userDid,
    required this.verificationMethod,
    required this.keyType,
    this.keyAlgorithm,
    required this.publicKeyMultibase,
    this.privateKeyEncoding = 'pem',
    String? privateKeyPem,
    String? privateKeyMultibase,
  }) : privateKeyPem = privateKeyPem ?? privateKeyMultibase ?? '',
       privateKeyMultibase = privateKeyMultibase ?? privateKeyPem ?? '';

  final String schema;
  final String userDid;
  final String verificationMethod;
  final String keyType;
  final String? keyAlgorithm;
  final String publicKeyMultibase;
  final String privateKeyEncoding;
  final String privateKeyPem;
  @Deprecated('Use privateKeyPem for PEM v2 packages.')
  final String privateKeyMultibase;
}

class DaemonSubkeyAuthorizationRevokeResult {
  const DaemonSubkeyAuthorizationRevokeResult({
    required this.userDid,
    required this.verificationMethod,
    required this.updated,
  });

  final String userDid;
  final String verificationMethod;
  final bool updated;
}

class HandleRegistrationResult {
  const HandleRegistrationResult({
    this.identity,
    required this.handle,
    required this.method,
    required this.state,
    this.defaultIdentityChange,
    this.warnings = const [],
  });

  final IdentitySummary? identity;
  final String handle;
  final String method;
  final String state;
  final DefaultIdentityChange? defaultIdentityChange;
  final List<String> warnings;
}

class RecoverHandleResult {
  const RecoverHandleResult({
    required this.handle,
    required this.phone,
    required this.state,
    this.recoveredIdentity,
    this.userId,
    required this.accessTokenPresent,
    this.warnings = const [],
  });

  final String handle;
  final String phone;
  final String state;
  final IdentitySummary? recoveredIdentity;
  final String? userId;
  final bool accessTokenPresent;
  final List<String> warnings;
}

enum DeviceJoinSide { newDevice, admin }

enum DeviceJoinPhase {
  pending,
  challengePrepared,
  responsePrepared,
  responseVerified,
  approvalPrepared,
  authorized,
  cancelled,
  expired,
}

enum DeviceJoinRole { member, admin }

enum DeviceJoinAuthorizationStatus { active, revoked }

enum DeviceJoinRemoteState {
  pending,
  claimed,
  challengeSent,
  responseVerified,
  consumed,
  expired,
}

class DeviceJoinSessionSummary {
  const DeviceJoinSessionSummary({
    required this.joinSessionId,
    required this.did,
    required this.protocolDeviceId,
    required this.side,
    required this.phase,
    required this.expiresAt,
  });

  final String joinSessionId;
  final String did;
  final String protocolDeviceId;
  final DeviceJoinSide side;
  final DeviceJoinPhase phase;
  final String expiresAt;
}

class DeviceJoinAuthorizedDeviceSummary {
  const DeviceJoinAuthorizedDeviceSummary({
    required this.protocolDeviceId,
    required this.signingKeyId,
    required this.e2eeKeyId,
    required this.status,
    required this.role,
    required this.managementReady,
    required this.isCurrent,
  });

  final String protocolDeviceId;
  final String signingKeyId;
  final String e2eeKeyId;
  final DeviceJoinAuthorizationStatus status;
  final DeviceJoinRole role;
  final bool managementReady;
  final bool isCurrent;
}

class DeviceJoinPendingSummary {
  const DeviceJoinPendingSummary({
    required this.joinSessionId,
    required this.protocolDeviceId,
    required this.signingKeyId,
    required this.e2eeKeyId,
    required this.requestedRole,
    required this.issuedAt,
    required this.expiresAt,
  });

  final String joinSessionId;
  final String protocolDeviceId;
  final String signingKeyId;
  final String e2eeKeyId;
  final DeviceJoinRole requestedRole;
  final String issuedAt;
  final String expiresAt;
}

class DeviceJoinRegistrySnapshot {
  const DeviceJoinRegistrySnapshot({
    required this.did,
    required this.devices,
    required this.pendingJoinRequests,
  });

  final String did;
  final List<DeviceJoinAuthorizedDeviceSummary> devices;
  final List<DeviceJoinPendingSummary> pendingJoinRequests;
}

class DeviceJoinProgress {
  const DeviceJoinProgress({
    required this.session,
    required this.remoteState,
    this.sas,
    this.authorizedDevice,
  });

  final DeviceJoinSessionSummary session;
  final DeviceJoinRemoteState remoteState;
  final String? sas;
  final DeviceJoinAuthorizedDeviceSummary? authorizedDevice;
}

class DeviceJoinApprovalPrompt {
  const DeviceJoinApprovalPrompt({
    required this.approvalHandle,
    required this.joinSessionId,
    required this.role,
    required this.sas,
    required this.expiresAt,
  });

  final String approvalHandle;
  final String joinSessionId;
  final DeviceJoinRole role;
  final String sas;
  final String expiresAt;

  @override
  String toString() =>
      'DeviceJoinApprovalPrompt(joinSessionId: $joinSessionId, role: $role, '
      'approvalHandle: <redacted>, sas: <redacted>)';
}
