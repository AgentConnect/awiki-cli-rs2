Pod::Spec.new do |s|
  s.name             = 'awiki_im_core'
  s.version          = '0.1.0'
  s.summary          = 'Awiki IM Core Flutter SDK'
  s.description      = 'Flutter FFI bindings for Rust im-core.'
  s.homepage         = 'https://github.com/AgentConnect/awiki-cli-rs2'
  s.license          = { :type => 'AGPL-3.0-only', :file => '../LICENSE' }
  s.author           = { 'AgentConnect' => 'dev@awiki.ai' }
  s.source           = { :path => '.' }
  s.platform         = :ios, '13.0'
  s.source_files     = 'Classes/**/*'
  s.vendored_frameworks = 'Frameworks/AwikiImCore.xcframework'
  s.user_target_xcconfig = {
    'OTHER_LDFLAGS[sdk=iphoneos*]' => '$(inherited) -force_load $(PODS_ROOT)/../.symlinks/plugins/awiki_im_core/ios/Frameworks/AwikiImCore.xcframework/ios-arm64/libawiki_im_core.a',
    'OTHER_LDFLAGS[sdk=iphonesimulator*]' => '$(inherited) -force_load $(PODS_ROOT)/../.symlinks/plugins/awiki_im_core/ios/Frameworks/AwikiImCore.xcframework/ios-arm64_x86_64-simulator/libawiki_im_core.a'
  }
end
