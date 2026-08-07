'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const test = require('node:test');

const root = path.resolve(__dirname, '..');
const bundle = 'LICENSE LICENSE-APACHE COMMERCIAL-LICENSING.md SOURCE.md';

function run(relativePath, args) {
  return spawnSync('bash', [path.join(root, relativePath), ...args], {
    cwd: root,
    encoding: 'utf8',
  });
}

test('CLI and Daemon release plans include the complete AWiki license bundle', () => {
  const cli = run('scripts/release/build-release-artifact.sh', [
    '--dry-run', '--version', '1.0.41', '--os', 'linux', '--arch', 'amd64',
  ]);
  assert.equal(cli.status, 0, cli.stderr || cli.stdout);
  assert.match(cli.stdout, new RegExp(`Would include: awiki-cli ${bundle}`));

  const daemon = run('scripts/release/daemon/_build-artifact.sh', [
    '--dry-run', '--version', '0.1.84', '--os', 'linux', '--arch', 'amd64',
  ]);
  assert.equal(daemon.status, 0, daemon.stderr || daemon.stdout);
  assert.match(
    daemon.stdout,
    new RegExp(`Would include: awiki-deamon awiki-deamon-runtime README\\.txt ${bundle} checksums\\.txt`),
  );
});

test('packaged license copies match the canonical repository licenses', () => {
  const packages = [
    'crates/user-dirs',
    'crates/im-core',
    'crates/im-core-dart',
    'crates/awiki-cli',
    'crates/awiki-deamon',
    'xtask',
  ];
  for (const [canonicalPath, packagedName] of [
    ['LICENSE', 'LICENSE'],
    ['LICENSES/Apache-2.0.txt', 'LICENSE-APACHE'],
  ]) {
    const canonical = fs.readFileSync(path.join(root, canonicalPath));
    for (const packagePath of packages) {
      const packagedPath = path.join(packagePath, packagedName);
      assert.deepEqual(
        fs.readFileSync(path.join(root, packagedPath)),
        canonical,
        `${packagedPath} must match ${canonicalPath}`,
      );
    }
  }

  for (const packagePath of packages) {
    const manifest = fs.readFileSync(path.join(root, packagePath, 'Cargo.toml'), 'utf8');
    assert.match(manifest, /^license\.workspace = true$/m);
    assert.doesNotMatch(manifest, /^license-file/m);
  }
});
