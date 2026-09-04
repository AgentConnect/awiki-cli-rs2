'use strict';

const assert = require('node:assert/strict');
const childProcess = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const test = require('node:test');
const { parse: parseYaml } = require('yaml');
const {
  parseFlatToml, readAnpCandidateLock, readReleaseConfig, readServerConfig,
} = require('./config.js');

const FIXTURE_REPOSITORY = 'AgentConnect/awiki-system-test';
const FIXTURE_COMMIT = '5fdcbd62df78ca69f8de6399529fa7b36e0afeb5';
const FIXTURE_CHECKOUT_PATH = 'awiki-system-test';
const FIXTURE_SPARSE_PATH = 'suites/fixtures/0714-e2ee-compat-v1';
const FIXTURE_ENV = '${{ github.workspace }}/awiki-system-test/suites/fixtures/0714-e2ee-compat-v1';
const READ_TOKEN_SECRET = '${{ secrets.AWIKI_CI_READ_TOKEN }}';

function readImCoreNodeCiWorkflow() {
  const source = fs.readFileSync(
    path.resolve(__dirname, '../../../.github/workflows/im-core-node-ci.yml'),
    'utf8',
  );
  return parseYaml(source);
}

function workflowStep(workflow, name) {
  const steps = workflow?.jobs?.verify?.steps;
  assert.ok(Array.isArray(steps), 'IM Core Node CI verify steps must be an array');
  const matches = steps.filter(step => step?.name === name);
  assert.equal(matches.length, 1, `expected exactly one ${name} step`);
  return matches[0];
}

function assertCannotSkip(value, label) {
  assert.equal(Object.hasOwn(value, 'if'), false, `${label} must not be conditional`);
  assert.equal(
    Object.hasOwn(value, 'continue-on-error'),
    false,
    `${label} must not allow failure`,
  );
}

function assertTokenGateFailsClosed(step) {
  assert.equal(step.shell, 'bash');
  assert.deepEqual(step.env, { READ_TOKEN: READ_TOKEN_SECRET });
  const withoutToken = childProcess.spawnSync(
    'bash',
    ['-euo', 'pipefail', '-c', step.run],
    { encoding: 'utf8', env: { ...process.env, READ_TOKEN: '' } },
  );
  assert.notEqual(withoutToken.status, 0, 'missing private checkout token must fail');
  const withToken = childProcess.spawnSync(
    'bash',
    ['-euo', 'pipefail', '-c', step.run],
    { encoding: 'utf8', env: { ...process.env, READ_TOKEN: 'test-read-token' } },
  );
  assert.equal(withToken.status, 0, 'a present private checkout token must pass the gate');
}

function validateImCoreNodeCiWorkflow(workflow) {
  const verifyJob = workflow?.jobs?.verify;
  assert.ok(verifyJob && typeof verifyJob === 'object', 'verify job must exist');
  assertCannotSkip(verifyJob, 'verify job');

  const tokenGate = workflowStep(workflow, 'Require read-only fixture checkout token');
  assertCannotSkip(tokenGate, 'fixture token gate');
  assertTokenGateFailsClosed(tokenGate);

  const checkout = workflowStep(workflow, 'Checkout locked 0714 E2EE compatibility fixture');
  assertCannotSkip(checkout, 'fixture checkout');
  assert.equal(checkout.uses, 'actions/checkout@v6');
  assert.deepEqual(checkout.with, {
    repository: FIXTURE_REPOSITORY,
    ref: FIXTURE_COMMIT,
    token: READ_TOKEN_SECRET,
    path: FIXTURE_CHECKOUT_PATH,
    'sparse-checkout': FIXTURE_SPARSE_PATH,
    'persist-credentials': false,
  });

  const ciVerify = workflowStep(workflow, 'Verify CI configuration');
  assertCannotSkip(ciVerify, 'CI configuration verification');
  assert.equal(ciVerify['working-directory'], 'awiki-cli-rs2');
  assert.equal(ciVerify.shell, 'bash');
  assert.equal(ciVerify.run, 'node --test scripts/release/cli/config.test.js');

  const rustVerify = workflowStep(workflow, 'Verify Rust facade and Node bridge');
  assertCannotSkip(rustVerify, 'Rust verification');
  assert.equal(rustVerify['working-directory'], 'awiki-cli-rs2');
  assert.equal(rustVerify.shell, 'bash');
  assert.deepEqual(rustVerify.env, { AWIKI_0714_E2EE_FIXTURE_DIR: FIXTURE_ENV });
  assert.deepEqual(
    rustVerify.run.split('\n').map(line => line.trim()).filter(Boolean),
    [
      'cargo fmt --check',
      'cargo clippy -p awiki-im-core --all-targets --all-features -- -D warnings',
      'cargo clippy -p awiki-im-core-node --all-targets --all-features -- -D warnings',
      'cargo test -p awiki-im-core',
      'cargo test -p awiki-im-core-node',
    ],
  );
}

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
  validateImCoreNodeCiWorkflow(readImCoreNodeCiWorkflow());
});

test('IM Core Node CI validation rejects fixture and cargo-test bypass mutations', () => {
  const mutationCases = [
    ['floating fixture ref', workflow => {
      workflowStep(workflow, 'Checkout locked 0714 E2EE compatibility fixture').with.ref =
        'release/0815';
    }],
    ['GitHub token fallback', workflow => {
      workflowStep(workflow, 'Checkout locked 0714 E2EE compatibility fixture').with.token =
        '${{ secrets.AWIKI_CI_READ_TOKEN || github.token }}';
    }],
    ['non-failing token gate', workflow => {
      workflowStep(workflow, 'Require read-only fixture checkout token').run = 'true';
    }],
    ['library-only Core tests', workflow => {
      const step = workflowStep(workflow, 'Verify Rust facade and Node bridge');
      step.run = step.run.replace(
        'cargo test -p awiki-im-core',
        'cargo test -p awiki-im-core --lib',
      );
    }],
    ['shell success fallback', workflow => {
      const step = workflowStep(workflow, 'Verify Rust facade and Node bridge');
      step.run = step.run.replace(
        'cargo test -p awiki-im-core',
        'cargo test -p awiki-im-core || true',
      );
    }],
    ['test skip filter', workflow => {
      const step = workflowStep(workflow, 'Verify Rust facade and Node bridge');
      step.run = step.run.replace(
        'cargo test -p awiki-im-core',
        'cargo test -p awiki-im-core -- --skip phase2_0714_migration',
      );
    }],
    ['continue-on-error', workflow => {
      workflowStep(workflow, 'Verify Rust facade and Node bridge')['continue-on-error'] = true;
    }],
    ['conditional skip', workflow => {
      workflowStep(workflow, 'Verify Rust facade and Node bridge').if = false;
    }],
  ];

  for (const [label, mutate] of mutationCases) {
    const workflow = structuredClone(readImCoreNodeCiWorkflow());
    mutate(workflow);
    assert.throws(
      () => validateImCoreNodeCiWorkflow(workflow),
      { name: 'AssertionError' },
      label,
    );
  }
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
