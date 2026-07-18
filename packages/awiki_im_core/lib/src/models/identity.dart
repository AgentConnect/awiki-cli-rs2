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
