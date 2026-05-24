enum MessageTransportPolicy { auto, httpOnly, realtimePreferred }

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
