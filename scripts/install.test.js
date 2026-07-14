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

test('maps only the four supported release targets', () => {
  assert.deepEqual(_internal.mapTarget('darwin', 'arm64'), {
    osName: 'darwin', archName: 'arm64', target: 'darwin-arm64',
  });
  assert.deepEqual(_internal.mapTarget('linux', 'x64'), {
    osName: 'linux', archName: 'amd64', target: 'linux-amd64',
  });
  assert.throws(() => _internal.mapTarget('linux', 'arm64'), /Unsupported platform/);
  assert.throws(() => _internal.mapTarget('win32', 'arm64'), /Unsupported platform/);
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
  fs.writeFileSync(binary, '#!/bin/sh\nprintf \'installer smoke\\n\'\n');
  fs.chmodSync(binary, 0o755);
  const archive = path.join(root, 'awiki-cli.tar.gz');
  const tar = await execFileAsync('tar', ['-C', archiveStage, '-czf', archive, 'awiki-cli']);
  assert.equal(tar.stderr, '');
  const digest = crypto.createHash('sha256').update(fs.readFileSync(archive)).digest('hex');
  const target = _internal.mapTarget().target;

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
      packages: {
        [target]: {
          url: `http://127.0.0.1:${port}/awiki-cli.tar.gz`,
          sha256: digest,
        },
      },
    };
    fs.writeFileSync(metadataPath, JSON.stringify(metadata));
    const install = path.join(scriptsDir, 'install.js');
    const success = await execFileAsync(process.execPath, [install], { cwd: packageRoot });
    assert.match(success.stdout, /binary is installed/);
    assert.ok(fs.statSync(path.join(packageRoot, 'bin', 'awiki-cli')).isFile());

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
