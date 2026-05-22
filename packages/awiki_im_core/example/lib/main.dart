import 'package:awiki_im_core/awiki_im_core.dart';
import 'package:flutter/widgets.dart';

void main() {
  const config = AwikiImCoreConfig(
    serviceBaseUrl: 'https://awiki.ai',
    didDomain: 'awiki.ai',
  );
  runApp(
    Directionality(
      textDirection: TextDirection.ltr,
      child: Text(config.didDomain),
    ),
  );
}
