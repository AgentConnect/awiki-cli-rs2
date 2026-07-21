import 'dart:ffi';
import 'dart:io';

import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';

enum AwikiImCoreLibrarySource { open, process }

final class AwikiImCoreLibrarySelection {
  const AwikiImCoreLibrarySelection.open(this.path)
    : source = AwikiImCoreLibrarySource.open;

  const AwikiImCoreLibrarySelection.process()
    : source = AwikiImCoreLibrarySource.process,
      path = null;

  final AwikiImCoreLibrarySource source;
  final String? path;
}

AwikiImCoreLibrarySelection resolveAwikiImCoreLibrary({
  required String operatingSystem,
  String? macOsDylibPath,
}) {
  switch (operatingSystem) {
    case 'android':
    case 'linux':
      return const AwikiImCoreLibrarySelection.open('libawiki_im_core.so');
    case 'windows':
      return const AwikiImCoreLibrarySelection.open('awiki_im_core.dll');
    case 'macos':
      final configuredPath = macOsDylibPath?.trim();
      if (configuredPath != null && configuredPath.isNotEmpty) {
        return AwikiImCoreLibrarySelection.open(configuredPath);
      }
      return const AwikiImCoreLibrarySelection.process();
    case 'ios':
      return const AwikiImCoreLibrarySelection.process();
    default:
      throw UnsupportedError(
        'Unsupported platform for awiki_im_core native library: '
        '$operatingSystem.',
      );
  }
}

DynamicLibrary loadAwikiImCoreLibrary() {
  final selection = resolveAwikiImCoreLibrary(
    operatingSystem: Platform.operatingSystem,
    macOsDylibPath: _configuredMacOsDylibPath(),
  );
  return switch (selection.source) {
    AwikiImCoreLibrarySource.open => DynamicLibrary.open(selection.path!),
    AwikiImCoreLibrarySource.process => DynamicLibrary.process(),
  };
}

ExternalLibrary loadAwikiImCoreExternalLibrary() {
  final selection = resolveAwikiImCoreLibrary(
    operatingSystem: Platform.operatingSystem,
    macOsDylibPath: _configuredMacOsDylibPath(),
  );
  return switch (selection.source) {
    AwikiImCoreLibrarySource.open => ExternalLibrary.open(selection.path!),
    AwikiImCoreLibrarySource.process =>
      ExternalLibrary.process(iKnowHowToUseIt: true),
  };
}

String? _configuredMacOsDylibPath() {
  final value = Platform.environment['AWIKI_IM_CORE_DYLIB']?.trim();
  if (value == null || value.isEmpty) {
    return null;
  }
  return value;
}
