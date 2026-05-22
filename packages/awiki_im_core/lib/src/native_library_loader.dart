import 'dart:ffi';
import 'dart:io';

import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';

DynamicLibrary loadAwikiImCoreLibrary() {
  if (Platform.isAndroid) {
    return DynamicLibrary.open('libawiki_im_core.so');
  }
  if (Platform.isMacOS) {
    return DynamicLibrary.open('libawiki_im_core.dylib');
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
  if (Platform.isMacOS) {
    return ExternalLibrary.open('libawiki_im_core.dylib');
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
