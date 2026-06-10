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
