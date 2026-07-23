import 'dart:typed_data';

enum MessageTransportPolicy { auto, httpOnly, realtimePreferred }

enum IdentitySecretStoragePolicy { fileCompat, vaultPreferred, vaultRequired }

class AwikiImCoreConfig {
  const AwikiImCoreConfig({
    required this.serviceBaseUrl,
    required this.didDomain,
    this.userServiceEndpoint,
    this.messageServiceEndpoint,
    this.mailServiceEndpoint,
    this.anpServiceEndpoint,
    this.anpServiceDid,
    this.transportPolicy = MessageTransportPolicy.auto,
  });

  final String serviceBaseUrl;
  final String didDomain;
  final String? userServiceEndpoint;
  final String? messageServiceEndpoint;
  final String? mailServiceEndpoint;
  final String? anpServiceEndpoint;
  final String? anpServiceDid;
  final MessageTransportPolicy transportPolicy;
}

class AwikiImCoreOpenOptions {
  const AwikiImCoreOpenOptions({
    this.identitySecretStoragePolicy = IdentitySecretStoragePolicy.fileCompat,
    this.identitySecretVault,
    this.multiDeviceJoinEnabled = false,
    this.multiDeviceRootTransferEnabled = false,
    this.multiDeviceDeviceRevokeEnabled = false,
    this.multiDeviceDirectE2eeEnabled = false,
    this.multiDeviceGroupE2eeEnabled = false,
  });

  const AwikiImCoreOpenOptions.fileCompat()
    : identitySecretStoragePolicy = IdentitySecretStoragePolicy.fileCompat,
      identitySecretVault = null,
      multiDeviceJoinEnabled = false,
      multiDeviceRootTransferEnabled = false,
      multiDeviceDeviceRevokeEnabled = false,
      multiDeviceDirectE2eeEnabled = false,
      multiDeviceGroupE2eeEnabled = false;

  const AwikiImCoreOpenOptions.vaultPreferred({
    required ImCoreSecretVaultOptions this.identitySecretVault,
    this.multiDeviceJoinEnabled = false,
    this.multiDeviceRootTransferEnabled = false,
    this.multiDeviceDeviceRevokeEnabled = false,
    this.multiDeviceDirectE2eeEnabled = false,
    this.multiDeviceGroupE2eeEnabled = false,
  }) : identitySecretStoragePolicy = IdentitySecretStoragePolicy.vaultPreferred;

  const AwikiImCoreOpenOptions.vaultRequired({
    required ImCoreSecretVaultOptions this.identitySecretVault,
    this.multiDeviceJoinEnabled = false,
    this.multiDeviceRootTransferEnabled = false,
    this.multiDeviceDeviceRevokeEnabled = false,
    this.multiDeviceDirectE2eeEnabled = false,
    this.multiDeviceGroupE2eeEnabled = false,
  }) : identitySecretStoragePolicy = IdentitySecretStoragePolicy.vaultRequired;

  final IdentitySecretStoragePolicy identitySecretStoragePolicy;
  final ImCoreSecretVaultOptions? identitySecretVault;
  final bool multiDeviceJoinEnabled;
  final bool multiDeviceRootTransferEnabled;
  final bool multiDeviceDeviceRevokeEnabled;
  final bool multiDeviceDirectE2eeEnabled;
  final bool multiDeviceGroupE2eeEnabled;
}

class ImCoreSecretVaultOptions {
  const ImCoreSecretVaultOptions({
    required this.rootKey,
    required this.vaultDir,
    required this.workspaceId,
    required this.deviceId,
  });

  final DeviceVaultRootKey rootKey;
  final String vaultDir;
  final String workspaceId;
  final String deviceId;
}

class DeviceVaultRootKey {
  DeviceVaultRootKey(Uint8List bytes) : _bytes = Uint8List.fromList(bytes);

  DeviceVaultRootKey.fromList(List<int> bytes)
    : _bytes = Uint8List.fromList(bytes);

  final Uint8List _bytes;

  Uint8List get bytes => Uint8List.fromList(_bytes);

  @override
  String toString() =>
      'DeviceVaultRootKey(len: ${_bytes.length}, value: <redacted>)';
}

class AwikiImCorePaths {
  const AwikiImCorePaths({
    required this.identityRootDir,
    required this.registryPath,
    this.defaultIdentityPath,
    required this.sqlitePath,
    required this.cacheDir,
    required this.tempDir,
  });

  final String identityRootDir;
  final String registryPath;
  final String? defaultIdentityPath;
  final String sqlitePath;
  final String cacheDir;
  final String tempDir;
}
