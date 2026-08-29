const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const test = require("node:test");

const root = path.resolve(__dirname, "../..");

function runScript(relativePath, args) {
  return spawnSync("bash", [path.join(root, relativePath), ...args], {
    cwd: root,
    encoding: "utf8",
  });
}

function expectSuccess(result) {
  assert.equal(result.status, 0, result.stderr || result.stdout);
}

test("Flutter package carries the canonical license bundle", () => {
  const packageRoot = path.join(root, "packages/awiki_im_core");
  for (const [canonical, packaged] of [
    ["LICENSE", "LICENSE"],
    ["LICENSES/Apache-2.0.txt", "LICENSE-APACHE"],
    ["COMMERCIAL-LICENSING.md", "COMMERCIAL-LICENSING.md"],
  ]) {
    assert.deepEqual(
      fs.readFileSync(path.join(packageRoot, packaged)),
      fs.readFileSync(path.join(root, canonical)),
      `${packaged} must match the canonical repository file`,
    );
  }
  for (const platform of ["ios", "macos"]) {
    const podspec = fs.readFileSync(
      path.join(packageRoot, platform, "awiki_im_core.podspec"),
      "utf8",
    );
    assert.match(podspec, /:type => 'AGPL-3\.0-only'/);
    assert.match(podspec, /:file => '\.\.\/LICENSE'/);
    assert.doesNotMatch(podspec, /:type => 'MIT'/);
  }
});

test("macOS static IM Core has one Runner force-load boundary", () => {
  const podspec = fs.readFileSync(
    path.join(
      root,
      "packages/awiki_im_core/macos/awiki_im_core.podspec",
    ),
    "utf8",
  );
  assert.match(podspec, /vendored_frameworks/);
  assert.doesNotMatch(podspec, /pod_target_xcconfig/);
  assert.equal((podspec.match(/-force_load/g) || []).length, 1);
  assert.equal((podspec.match(/-export_dynamic/g) || []).length, 1);
});

test("Apple builds write a verifiable native artifact manifest", () => {
  const buildScript = fs.readFileSync(
    path.join(root, "scripts/flutter/build-apple.sh"),
    "utf8",
  );
  const verifier = fs.readFileSync(
    path.join(root, "scripts/flutter/verify-native-artifact.sh"),
    "utf8",
  );
  assert.match(buildScript, /native-artifact-manifest\.py/);
  assert.match(buildScript, /--platform ios/);
  assert.match(buildScript, /--platform macos/);
  assert.match(buildScript, /--targets "\$\{macos_targets\}"/);
  assert.match(verifier, /verify --platform "\$platform"/);

  const help = spawnSync(
    "python3",
    [
      path.join(root, "scripts/flutter/native-artifact-manifest.py"),
      "--help",
    ],
    { cwd: root, encoding: "utf8" },
  );
  expectSuccess(help);
  assert.match(help.stdout, /write/);
  assert.match(help.stdout, /verify/);
});

test(
  "Apple dry-run limits a macOS XCFramework to the requested architecture",
  { skip: process.platform === "win32" },
  () => {
    const arm64 = runScript("scripts/flutter/build-apple.sh", [
      "--dry-run",
      "--macos",
      "--macos-arch",
      "arm64",
    ]);
    expectSuccess(arm64);
    assert.match(arm64.stdout, /Would rustup target add: aarch64-apple-darwin/);
    assert.doesNotMatch(arm64.stdout, /x86_64-apple-darwin/);

    const x64 = runScript("scripts/flutter/build-sdk-native.sh", [
      "--dry-run",
      "--macos-only",
      "--macos-arch",
      "x86_64",
      "--skip-codegen-check",
    ]);
    expectSuccess(x64);
    assert.match(
      x64.stdout,
      /build-apple\.sh --macos --macos-arch x86_64/,
    );
  },
);

test(
  "platform build defaults retain every supported native architecture",
  { skip: process.platform === "win32" },
  () => {
    const macOS = runScript("scripts/flutter/build-apple.sh", [
      "--dry-run",
      "--macos",
    ]);
    expectSuccess(macOS);
    assert.match(
      macOS.stdout,
      /Would rustup target add: aarch64-apple-darwin x86_64-apple-darwin/,
    );

    const android = runScript("scripts/flutter/build-android.sh", [
      "--dry-run",
    ]);
    expectSuccess(android);
    assert.match(
      android.stdout,
      /Would cargo ndk build arm64-v8a x86_64 armeabi-v7a /,
    );
  },
);

test(
  "Android dry-run limits native output to arm64-v8a",
  { skip: process.platform === "win32" },
  () => {
    const direct = runScript("scripts/flutter/build-android.sh", [
      "--dry-run",
      "--abi",
      "arm64-v8a",
    ]);
    expectSuccess(direct);
    assert.match(direct.stdout, /Would rustup target add: aarch64-linux-android/);
    assert.match(direct.stdout, /Would cargo ndk build arm64-v8a /);
    assert.doesNotMatch(direct.stdout, /x86_64-linux-android/);
    assert.doesNotMatch(direct.stdout, /armeabi-v7a/);

    const wrapper = runScript("scripts/flutter/build-sdk-native.sh", [
      "--dry-run",
      "--android-only",
      "--android-abi",
      "arm64-v8a",
      "--skip-codegen-check",
    ]);
    expectSuccess(wrapper);
    assert.match(wrapper.stdout, /build-android\.sh --abi arm64-v8a/);
  },
);

test(
  "architecture filters require their matching platform-only mode",
  { skip: process.platform === "win32" },
  () => {
    const macOS = runScript("scripts/flutter/build-sdk-native.sh", [
      "--dry-run",
      "--macos-arch",
      "arm64",
    ]);
    assert.notEqual(macOS.status, 0);
    assert.match(macOS.stderr, /requires --macos-only/);

    const android = runScript("scripts/flutter/build-sdk-native.sh", [
      "--dry-run",
      "--android-abi",
      "arm64-v8a",
    ]);
    assert.notEqual(android.status, 0);
    assert.match(android.stderr, /requires --android-only/);
  },
);

test("Android builds remove every generated shared library before compiling", () => {
  const source = fs.readFileSync(
    path.join(root, "scripts/flutter/build-android.sh"),
    "utf8",
  );
  assert.match(source, /find "\$\{OUT_DIR\}" -type f -name "\*\.so" -delete/);
});

test("Linux native IM Core includes group E2EE and secure-direct support", () => {
  const source = fs.readFileSync(
    path.join(root, "scripts/flutter/build-linux.sh"),
    "utf8",
  );
  assert.match(
    source,
    /--features blocking,sqlite,http,linux,group-e2ee,secure-direct,identity-native-anp/,
  );
});

test("Android native IM Core includes group E2EE and secure-direct support", () => {
  const source = fs.readFileSync(
    path.join(root, "scripts/flutter/build-android.sh"),
    "utf8",
  );
  assert.match(
    source,
    /--features blocking,sqlite,http,android,group-e2ee,secure-direct,identity-native-anp/,
  );
});

test("iOS native IM Core includes group E2EE and secure-direct support", () => {
  const source = fs.readFileSync(
    path.join(root, "scripts/flutter/build-apple.sh"),
    "utf8",
  );
  assert.equal(
    source.match(
      /blocking,sqlite,http,ios,group-e2ee,secure-direct,identity-native-anp/g,
    )?.length,
    2,
  );
});

test("macOS native IM Core includes group E2EE and secure-direct support", () => {
  const source = fs.readFileSync(
    path.join(root, "scripts/flutter/build-apple.sh"),
    "utf8",
  );
  assert.equal(
    source.match(
      /blocking,sqlite,http,macos,group-e2ee,secure-direct,identity-native-anp/g,
    )?.length,
    2,
  );
});

test("Windows native IM Core includes group E2EE and secure-direct support", () => {
  const source = fs.readFileSync(
    path.join(root, "scripts/flutter/build-windows.ps1"),
    "utf8",
  );
  assert.match(
    source,
    /\$Features = 'blocking,sqlite,http,windows,group-e2ee,secure-direct,identity-native-anp'/,
  );
});
