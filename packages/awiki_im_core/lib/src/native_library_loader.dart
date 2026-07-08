import 'dart:ffi';
import 'dart:io';

import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';

DynamicLibrary loadAwikiImCoreLibrary() {
  if (Platform.isAndroid) {
    return DynamicLibrary.open('libawiki_im_core.so');
  }
  if (Platform.isLinux) {
    return DynamicLibrary.open('libawiki_im_core.so');
  }
  if (Platform.isMacOS) {
    final dylibPath = _configuredMacOsDylibPath();
    if (dylibPath != null) {
      return DynamicLibrary.open(dylibPath);
    }
    return DynamicLibrary.process();
  }
  if (Platform.isIOS) {
    return DynamicLibrary.process();
  }
  if (Platform.isWindows) {
    throw UnsupportedError('Windows is not supported by awiki_im_core v0.1.');
  }
  throw UnsupportedError(
    'Unsupported platform for awiki_im_core native library.',
  );
}

ExternalLibrary loadAwikiImCoreExternalLibrary() {
  if (Platform.isAndroid) {
    return ExternalLibrary.open('libawiki_im_core.so');
  }
  if (Platform.isLinux) {
    return ExternalLibrary.open('libawiki_im_core.so');
  }
  if (Platform.isMacOS) {
    final dylibPath = _configuredMacOsDylibPath();
    if (dylibPath != null) {
      return ExternalLibrary.open(dylibPath);
    }
    return ExternalLibrary.process(iKnowHowToUseIt: true);
  }
  if (Platform.isIOS) {
    return ExternalLibrary.process(iKnowHowToUseIt: true);
  }
  if (Platform.isWindows) {
    throw UnsupportedError('Windows is not supported by awiki_im_core v0.1.');
  }
  throw UnsupportedError(
    'Unsupported platform for awiki_im_core native library.',
  );
}

String? _configuredMacOsDylibPath() {
  final value = Platform.environment['AWIKI_IM_CORE_DYLIB']?.trim();
  if (value == null || value.isEmpty) {
    return null;
  }
  return value;
}
