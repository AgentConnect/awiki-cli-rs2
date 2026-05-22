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
