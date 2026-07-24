'use strict';

const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const { execFile } = require('node:child_process');
const fs = require('node:fs');
const http = require('node:http');
const os = require('node:os');
const path = require('node:path');
const { promisify } = require('node:util');
const test = require('node:test');
const { _internal } = require('./install.js');

const execFileAsync = promisify(execFile);

test('normalizes supported executable architectures', () => {
  assert.equal(_internal.normalizeArchitecture('x64'), 'amd64');
  assert.equal(_internal.normalizeArchitecture(' X86_64 '), 'amd64');
  assert.equal(_internal.normalizeArchitecture('amd64'), 'amd64');
  assert.equal(_internal.normalizeArchitecture('arm64'), 'arm64');
  assert.equal(_internal.normalizeArchitecture('AARCH64'), 'arm64');
  assert.equal(_internal.normalizeArchitecture('ia32'), '');
  assert.equal(_internal.normalizeArchitecture('unknown'), '');
});

test('prefers a recognized machine architecture over the Node binary architecture', () => {
  assert.equal(_internal.detectHostArchitecture(() => 'arm64', 'x64'), 'arm64');
  assert.equal(_internal.detectHostArchitecture(() => 'x86_64', 'arm64'), 'amd64');
});

test('falls back to the Node binary architecture when machine detection is unavailable', () => {
  assert.equal(_internal.detectHostArchitecture(() => 'unknown', 'arm64'), 'arm64');
  assert.equal(_internal.detectHostArchitecture(() => '', 'x64'), 'amd64');
  assert.equal(_internal.detectHostArchitecture(() => '   ', 'arm64'), 'arm64');
  assert.equal(_internal.detectHostArchitecture(() => null, 'x64'), 'amd64');
  assert.equal(_internal.detectHostArchitecture(() => undefined, 'arm64'), 'arm64');
  assert.equal(_internal.detectHostArchitecture(() => 'riscv64', 'x64'), 'amd64');
  assert.equal(_internal.detectHostArchitecture(null, 'arm64'), 'arm64');
  assert.equal(_internal.detectHostArchitecture(() => {
    throw new Error('machine detection unavailable');
  }, 'x64'), 'amd64');
});

test('rejects architecture detection when neither source is supported', () => {
  assert.equal(_internal.detectHostArchitecture(() => 'unknown', 'ia32'), '');
  assert.equal(_internal.detectHostArchitecture(() => 'ia32', 'unknown'), '');
  assert.throws(() => _internal.mapHost('win32', ''), /Unsupported platform: win32\/unknown/);
});

test('maps the host independently from available release artifacts', () => {
  assert.deepEqual(_internal.mapHost('darwin', 'arm64'), {
    osName: 'darwin', archName: 'arm64', hostTarget: 'darwin-arm64',
  });
  assert.deepEqual(_internal.mapHost('darwin', 'x86_64'), {
    osName: 'darwin', archName: 'amd64', hostTarget: 'darwin-amd64',
  });
  assert.deepEqual(_internal.mapHost('linux', 'x64'), {
    osName: 'linux', archName: 'amd64', hostTarget: 'linux-amd64',
  });
  assert.deepEqual(_internal.mapHost('win32', 'x64'), {
    osName: 'windows', archName: 'amd64', hostTarget: 'windows-amd64',
  });
  assert.deepEqual(_internal.mapHost('win32', 'arm64'), {
    osName: 'windows', archName: 'arm64', hostTarget: 'windows-arm64',
  });
  assert.deepEqual(_internal.mapHost('win32', 'aarch64'), {
    osName: 'windows', archName: 'arm64', hostTarget: 'windows-arm64',
  });
  assert.throws(() => _internal.mapHost('linux', 'arm64'), /Unsupported platform/);
  assert.throws(() => _internal.mapHost('freebsd', 'x64'), /Unsupported platform/);
  assert.throws(() => _internal.mapHost('win32', 'ia32'), /Unsupported platform/);
});

test('prefers a native Windows ARM64 artifact when release metadata provides one', () => {
  const host = _internal.mapHost('win32', 'arm64');
  const arm64 = { url: 'https://downloads.example/arm64.zip', sha256: 'a'.repeat(64) };
  const amd64 = { url: 'https://downloads.example/amd64.zip', sha256: 'b'.repeat(64) };

  assert.deepEqual(_internal.selectArtifactForHost(host, {
    'windows-amd64': amd64,
    'windows-arm64': arm64,
  }), {
    artifact: arm64,
    artifactTarget: 'windows-arm64',
    compatibilityFallback: false,
  });
});

test('selects the real Windows x64 artifact as the ARM64 compatibility fallback', () => {
  const host = _internal.mapHost('win32', 'arm64');
  const amd64 = { url: 'https://downloads.example/amd64.zip', sha256: 'b'.repeat(64) };

  assert.deepEqual(_internal.selectArtifactForHost(host, {
    'windows-amd64': amd64,
  }), {
    artifact: amd64,
    artifactTarget: 'windows-amd64',
    compatibilityFallback: true,
  });
});

test('recovers an unknown Windows machine type and selects the x64 compatibility artifact', () => {
  const architecture = _internal.detectHostArchitecture(() => 'unknown', 'arm64');
  const host = _internal.mapHost('win32', architecture);
  const amd64 = { url: 'https://downloads.example/amd64.zip', sha256: 'b'.repeat(64) };

  assert.deepEqual(_internal.selectArtifactForHost(host, {
    'windows-amd64': amd64,
  }), {
    artifact: amd64,
    artifactTarget: 'windows-amd64',
    compatibilityFallback: true,
  });
});

test('selects Windows x64 directly without marking it as a compatibility fallback', () => {
  const host = _internal.mapHost('win32', 'x64');
  const amd64 = { url: 'https://downloads.example/amd64.zip', sha256: 'b'.repeat(64) };

  assert.deepEqual(_internal.selectArtifactForHost(host, {
    'windows-amd64': amd64,
  }), {
    artifact: amd64,
    artifactTarget: 'windows-amd64',
    compatibilityFallback: false,
  });
});

test('fails closed when release metadata declares an invalid Windows ARM64 artifact', () => {
  const host = _internal.mapHost('win32', 'arm64');
  const amd64 = { url: 'https://downloads.example/amd64.zip', sha256: 'b'.repeat(64) };

  assert.throws(
    () => _internal.selectArtifactForHost(host, {
      'windows-amd64': amd64,
      'windows-arm64': { url: 'https://downloads.example/arm64.zip', sha256: 'invalid' },
    }),
    /invalid package entry for windows-arm64/,
  );
});

test('rejects a supported host when release metadata has no compatible artifact', () => {
  const host = _internal.mapHost('darwin', 'arm64');
  assert.throws(
    () => _internal.selectArtifactForHost(host, {}),
    /no valid package entry for darwin-arm64/,
  );
});

test('requires structured release metadata', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'awiki-installer-test-'));
  try {
    fs.writeFileSync(path.join(root, 'awiki-release.json'), JSON.stringify({
      schema_version: 1,
      version: '1.0.17-beta.1',
      packages: {},
    }));
    assert.equal(_internal.readReleaseMetadata(root).version, '1.0.17-beta.1');
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('downloads an artifact, verifies SHA-256, and rejects a mismatched digest', async t => {
  if (process.platform === 'win32') {
    t.skip('the portable installer test uses a POSIX tar archive');
    return;
  }
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'awiki-installer-download-'));
  const archiveStage = path.join(root, 'archive-stage');
  const packageRoot = path.join(root, 'package');
  const scriptsDir = path.join(packageRoot, 'scripts');
  fs.mkdirSync(archiveStage);
  fs.mkdirSync(scriptsDir, { recursive: true });
  fs.copyFileSync(path.resolve(__dirname, 'install.js'), path.join(scriptsDir, 'install.js'));
  const binary = path.join(archiveStage, 'awiki-cli');
  fs.writeFileSync(binary, `#!/bin/sh
set -eu
mkdir -p "$AWIKI_CLI_WORKSPACE_HOME_DIR"
printf '%s\n%s\n%s\n%s\n%s\n' \
  "$HOME" \
  "$AWIKI_CLI_WORKSPACE_HOME_DIR" \
  "$AWIKI_CLI_UPDATE_BASE_URL" \
  "$AWIKI_CLI_DEFAULT_BACKEND_BASE_URL" \
  "$AWIKI_CLI_DEFAULT_DID_HOST" > "$AWIKI_TEST_PROBE_OUTPUT"
printf 'installer smoke\n'
`);
  fs.chmodSync(binary, 0o755);
  const archive = path.join(root, 'awiki-cli.tar.gz');
  const tar = await execFileAsync('tar', ['-C', archiveStage, '-czf', archive, 'awiki-cli']);
  assert.equal(tar.stderr, '');
  const digest = crypto.createHash('sha256').update(fs.readFileSync(archive)).digest('hex');
  const target = _internal.mapHost().hostTarget;

  const server = http.createServer((request, response) => {
    if (request.url !== '/awiki-cli.tar.gz') {
      response.writeHead(404).end();
      return;
    }
    response.writeHead(200, { 'Content-Type': 'application/gzip' });
    fs.createReadStream(archive).pipe(response);
  });
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });

  try {
    const { port } = server.address();
    const metadataPath = path.join(packageRoot, 'awiki-release.json');
    const metadata = {
      schema_version: 1,
      version: '1.0.17-beta.1',
      update_base_url: 'https://downloads.example.com/cli/beta',
      default_tenant: {
        backend_base_url: 'https://tenant.example.com',
        did_host: 'tenant.example.com',
      },
      packages: {
        [target]: {
          url: `http://127.0.0.1:${port}/awiki-cli.tar.gz`,
          sha256: digest,
        },
      },
    };
    fs.writeFileSync(metadataPath, JSON.stringify(metadata));
    const install = path.join(scriptsDir, 'install.js');
    const realHome = path.join(root, 'real-home');
    const inheritedWorkspace = path.join(realHome, 'inherited-workspace');
    const probeOutput = path.join(root, 'probe-output.txt');
    fs.mkdirSync(realHome);
    const success = await execFileAsync(process.execPath, [install], {
      cwd: packageRoot,
      env: {
        ...process.env,
        HOME: realHome,
        AWIKI_CLI_WORKSPACE_HOME_DIR: inheritedWorkspace,
        AWIKI_TEST_PROBE_OUTPUT: probeOutput,
      },
    });
    assert.match(success.stdout, /binary is installed/);
    assert.ok(fs.statSync(path.join(packageRoot, 'bin', 'awiki-cli')).isFile());
    const [probeHome, probeWorkspace, updateBaseUrl, backendBaseUrl, didHost] =
      fs.readFileSync(probeOutput, 'utf8').trim().split('\n');
    assert.notEqual(probeHome, realHome);
    assert.equal(probeWorkspace, path.join(probeHome, '.awiki-cli'));
    assert.equal(updateBaseUrl, metadata.update_base_url);
    assert.equal(backendBaseUrl, metadata.default_tenant.backend_base_url);
    assert.equal(didHost, metadata.default_tenant.did_host);
    assert.equal(fs.existsSync(probeHome), false);
    assert.equal(fs.existsSync(inheritedWorkspace), false);
    assert.equal(fs.existsSync(path.join(realHome, '.awiki-cli')), false);

    fs.rmSync(path.join(packageRoot, 'bin'), { recursive: true, force: true });
    metadata.packages[target].sha256 = '0'.repeat(64);
    fs.writeFileSync(metadataPath, JSON.stringify(metadata));
    await assert.rejects(
      execFileAsync(process.execPath, [install], { cwd: packageRoot }),
      error => /SHA-256 mismatch/.test(error.stderr),
    );
  } finally {
    await new Promise(resolve => server.close(resolve));
    fs.rmSync(root, { recursive: true, force: true });
  }
});
