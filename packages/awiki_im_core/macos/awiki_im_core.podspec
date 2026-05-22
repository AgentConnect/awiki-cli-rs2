Pod::Spec.new do |s|
  s.name             = 'awiki_im_core'
  s.version          = '0.1.0'
  s.summary          = 'Awiki IM Core Flutter SDK'
  s.description      = 'Flutter FFI bindings for Rust im-core.'
  s.homepage         = 'https://github.com/AgentConnect/awiki-cli-rs2'
  s.license          = { :type => 'MIT' }
  s.author           = { 'AgentConnect' => 'dev@awiki.ai' }
  s.source           = { :path => '.' }
  s.platform         = :osx, '10.14'
  s.source_files     = 'Classes/**/*'
  s.vendored_frameworks = 'Frameworks/AwikiImCore.xcframework'
end
