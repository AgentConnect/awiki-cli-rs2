import 'models/config.dart';

class AwikiImCore {
  static Future<AwikiImCore> open({
    required AwikiImCoreConfig config,
    required AwikiImCorePaths paths,
  }) async {
    throw UnsupportedError(
      'awiki_im_core native Rust backend is not supported on Flutter Web.',
    );
  }

  Future<void> dispose() async {}
}
