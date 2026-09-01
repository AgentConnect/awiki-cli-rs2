'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const test = require('node:test');
const {
  parseFlatToml, readAnpCandidateLock, readReleaseConfig, readServerConfig,
} = require('./config.js');

test('flat TOML parser accepts quoted values and rejects executable syntax', () => {
  assert.deepEqual(parseFlatToml('public_origin = "https://example.com" # comment\n'), {
    public_origin: 'https://example.com',
  });
  assert.throws(() => parseFlatToml('public_origin = process.env.SECRET\n'), /unsupported TOML syntax/);
  assert.throws(() => parseFlatToml('a = "one"\na = "two"\n'), /duplicate TOML key/);
});

test('server and release configuration schemas are strict', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'awiki-release-config-'));
  try {
    const server = path.join(root, 'server.toml');
    const example = path.resolve(__dirname, 'publish-server.example.toml');
    fs.copyFileSync(example, server);
    const parsedServer = readServerConfig(server);
    assert.equal(parsedServer.public_base_path, '/cli');
    assert.equal(parsedServer.cli_download_max_per_ip, 2);
    assert.equal(parsedServer.cli_download_max_total, 4);
    assert.equal(parsedServer.cli_download_rate_after, '1m');
    assert.equal(parsedServer.cli_download_rate, '512k');
    fs.appendFileSync(server, 'unknown = "value"\n');
    assert.throws(() => readServerConfig(server), /unknown publish-server keys/);

    const invalidGateway = path.join(root, 'invalid-gateway.toml');
    fs.writeFileSync(
      invalidGateway,
      fs.readFileSync(example, 'utf8').replace(
        'protocol_gateway_origin = "http://127.0.0.1:9896"',
        'protocol_gateway_origin = "http://user@127.0.0.1:9896/path"',
      ),
    );
    assert.throws(() => readServerConfig(invalidGateway), /protocol_gateway_origin/);

    const invalidDownloadLimit = path.join(root, 'invalid-download-limit.toml');
    fs.writeFileSync(
      invalidDownloadLimit,
      fs.readFileSync(example, 'utf8').replace(
        'cli_download_max_total = "4"',
        'cli_download_max_total = "1"',
      ),
    );
    assert.throws(() => readServerConfig(invalidDownloadLimit), /greater than or equal/);

    const invalidDownloadRate = path.join(root, 'invalid-download-rate.toml');
    fs.writeFileSync(
      invalidDownloadRate,
      fs.readFileSync(example, 'utf8').replace(
        'cli_download_rate = "512k"',
        'cli_download_rate = "512kb; include bad.conf"',
      ),
    );
    assert.throws(() => readServerConfig(invalidDownloadRate), /positive Nginx size/);

    const release = path.resolve(__dirname, 'release-config.json');
    const parsed = readReleaseConfig(release);
    assert.equal(parsed.channels.beta.version, '1.0.20-beta.1');
    assert.equal(parsed.channels.stable.version, '1.0.48');
    assert.equal(parsed.channels.stable.min_supported_version, '1.0.48');
    assert.equal(parsed.anp_commit, 'c64bfb086acb1930803bb95db8b4e28b67cc2983');
    assert.equal(parsed.anp_identity_commit, 'afcbe1a36b5e6b060d072961d59115b2bb3ed14f');
    assert.deepEqual(parsed.targets, [
      'darwin-amd64', 'darwin-arm64', 'linux-amd64', 'windows-amd64',
    ]);

    const invalidRelease = path.join(root, 'invalid-release.json');
    fs.writeFileSync(invalidRelease, JSON.stringify({
      ...parsed,
      targets: ['darwin-amd64', 'darwin-arm64', 'linux-amd64', 'linux-amd64'],
    }));
    assert.throws(() => readReleaseConfig(invalidRelease), /targets must contain exactly/);

    fs.writeFileSync(invalidRelease, JSON.stringify({ ...parsed, archive_keep_versions: 0 }));
    assert.throws(() => readReleaseConfig(invalidRelease), /archive_keep_versions/);

    fs.writeFileSync(invalidRelease, JSON.stringify({ ...parsed, unexpected: true }));
    assert.throws(() => readReleaseConfig(invalidRelease), /unknown release-config keys/);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('release config is bound to the closed ANP candidate lock', () => {
  const release = path.resolve(__dirname, 'release-config.json');
  const lockPath = path.resolve(__dirname, '../../../anp-release.lock.json');
  const lock = readAnpCandidateLock(lockPath);
  const parsed = readReleaseConfig(release, lockPath);
  assert.equal(parsed.anp_commit, lock.anp.commit);
  assert.equal(parsed.anp_identity_commit, lock.identity.commit);

  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'awiki-candidate-lock-'));
  try {
    const mismatched = path.join(root, 'release.json');
    fs.writeFileSync(mismatched, JSON.stringify({ ...parsed, anp_commit: 'f'.repeat(40) }));
    assert.throws(
      () => readReleaseConfig(mismatched, lockPath),
      /does not match ANP candidate lock/,
    );
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('publisher remains compatible with the production gh run-list surface', () => {
  const publisher = fs.readFileSync(
    path.resolve(__dirname, 'publish-cli-release.sh'),
    'utf8',
  );
  assert.doesNotMatch(publisher, /gh run list[^\n]*--event/);
  assert.doesNotMatch(publisher, /displayTitle/);
  assert.match(publisher, /\.name ==/);
  assert.match(publisher, /headSha ==/);
  assert.match(publisher, /createdAt >=/);
});

test('daemon release checks out the configured immutable ANP revision', () => {
  const workflow = fs.readFileSync(
    path.resolve(__dirname, '../../../.github/workflows/build-daemon-release.yml'),
    'utf8',
  );
  assert.match(workflow, /name: Read pinned ANP SDK revision/);
  assert.match(workflow, /ref: \$\{\{ steps\.release\.outputs\.anp_commit \}\}/);
  assert.match(workflow, /ref: \$\{\{ steps\.release\.outputs\.anp_identity_commit \}\}/);
  assert.match(workflow, /path: anp\/anp(?:\s|$)/);
  assert.match(workflow, /path: anp\/anp-identity(?:\s|$)/);
  assert.doesNotMatch(workflow, /repository: agent-network-protocol\/anp\s+ref: master/);
});


test('CLI release uses the canonical nested ANP workspace layout', () => {
  const workflow = fs.readFileSync(
    path.resolve(__dirname, '../../../.github/workflows/build-cli-release.yml'),
    'utf8',
  );
  assert.match(workflow, /ref: \$\{\{ steps\.release\.outputs\.anp_commit \}\}/);
  assert.match(workflow, /ref: \$\{\{ steps\.release\.outputs\.anp_identity_commit \}\}/);
  assert.match(workflow, /path: anp\/anp(?:\s|$)/);
  assert.match(workflow, /path: anp\/anp-identity(?:\s|$)/);
  assert.doesNotMatch(workflow, /path: anp-identity(?:\s|$)/);
});
