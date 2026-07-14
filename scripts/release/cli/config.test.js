'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const test = require('node:test');
const { parseFlatToml, readReleaseConfig, readServerConfig } = require('./config.js');

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
    assert.equal(readServerConfig(server).public_base_path, '/cli');
    fs.appendFileSync(server, 'unknown = "value"\n');
    assert.throws(() => readServerConfig(server), /unknown publish-server keys/);

    const release = path.resolve(__dirname, 'release-config.json');
    const parsed = readReleaseConfig(release);
    assert.equal(parsed.channels.beta.version, '1.0.20-beta.1');
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
