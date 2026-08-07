macos_libraries = Dir.glob(
  File.join(
    __dir__,
    'Frameworks/AwikiImCore.xcframework/macos-*/libawiki_im_core.a',
  ),
)
unless macos_libraries.length == 1
  raise "Expected exactly one macOS AwikiImCore XCFramework slice, found #{macos_libraries.length}"
end
macos_slice = File.basename(File.dirname(macos_libraries.first))

Pod::Spec.new do |s|
  s.name             = 'awiki_im_core'
  s.version          = '0.1.0'
  s.summary          = 'Awiki IM Core Flutter SDK'
  s.description      = 'Flutter FFI bindings for Rust im-core.'
  s.homepage         = 'https://github.com/AgentConnect/awiki-cli-rs2'
  s.license          = { :type => 'AGPL-3.0-only', :file => '../LICENSE' }
  s.author           = { 'AgentConnect' => 'dev@awiki.ai' }
  s.source           = { :path => '.' }
  s.platform         = :osx, '10.14'
  s.source_files     = 'Classes/**/*'
  s.vendored_frameworks = 'Frameworks/AwikiImCore.xcframework'
  # FRB resolves native symbols from the process. Keep one Runner-side
  # force-load so the static archive is not dead-stripped.
  s.user_target_xcconfig = {
    'OTHER_LDFLAGS' => "$(inherited) -force_load $(PODS_ROOT)/../Flutter/ephemeral/.symlinks/plugins/awiki_im_core/macos/Frameworks/AwikiImCore.xcframework/#{macos_slice}/libawiki_im_core.a -Wl,-export_dynamic"
  }
end
