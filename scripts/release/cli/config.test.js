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
    assert.equal(parsed.anp_commit, '45031b698e86e094dfef1f6d05fe9839a600854b');
    assert.equal(parsed.anp_identity_commit, '8dc65ccc388af0f0622263811776a6aadcd11d18');
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
  assert.match(lock.anp.didTransitionVectorsTreeSha256, /^[a-f0-9]{64}$/);

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

test('IM Core Node CI provisions the locked offline 0714 compatibility fixture', () => {
  const workflow = fs.readFileSync(
    path.resolve(__dirname, '../../../.github/workflows/im-core-node-ci.yml'),
    'utf8',
  );
  const checkoutStep = workflow.match(
    /^      - name: Checkout locked 0714 E2EE compatibility fixture\n[\s\S]*?(?=^      - name: )/m,
  )?.[0];
  assert.ok(checkoutStep, 'locked 0714 fixture checkout step must exist');
  assert.match(checkoutStep, /repository: AgentConnect\/awiki-system-test/);
  assert.match(checkoutStep, /ref: 5fdcbd62df78ca69f8de6399529fa7b36e0afeb5/);
  assert.match(checkoutStep, /path: awiki-system-test/);
  assert.match(checkoutStep, /sparse-checkout: suites\/fixtures\/0714-e2ee-compat-v1/);
  assert.doesNotMatch(checkoutStep, /ref: (?:main|release\/0815)/);

  const verifyStep = workflow.match(
    /^      - name: Verify Rust facade and Node bridge\n[\s\S]*?(?=^      - name: )/m,
  )?.[0];
  assert.ok(verifyStep, 'Rust verification step must exist');
  assert.match(
    verifyStep,
    /AWIKI_0714_E2EE_FIXTURE_DIR: \$\{\{ github\.workspace \}\}\/awiki-system-test\/suites\/fixtures\/0714-e2ee-compat-v1/,
  );
  assert.match(verifyStep, /cargo test -p awiki-im-core/);
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
