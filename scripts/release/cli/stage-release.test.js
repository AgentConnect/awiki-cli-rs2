'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const test = require('node:test');

const scriptDir = __dirname;
const rootDir = path.resolve(scriptDir, '../../..');
const releaseConfig = path.join(scriptDir, 'release-config.json');

function run(command, args, options = {}) {
  return spawnSync(command, args, { encoding: 'utf8', ...options });
}

function writeServerConfig(filePath, root) {
  const values = {
    public_origin: 'https://downloads.example.com',
    public_base_path: '/cli',
    default_backend_base_url: 'https://tenant.example.com',
    default_did_host: 'tenant.example.com',
    web_root: `${root}/web`,
    archive_root: `${root}/archive`,
    nginx_config: `${root}/nginx.conf`,
    nginx_snippet: `${root}/nginx-snippet.conf`,
    protocol_gateway_checkout: `${root}/gateway`,
    protocol_gateway_service: 'protocol-gateway',
    github_repo: 'AgentConnect/awiki-cli-rs2',
    github_workflow: 'build-cli-release.yml',
    github_token: 'test-token',
  };
  fs.writeFileSync(filePath, `${Object.entries(values).map(([key, value]) => `${key} = ${JSON.stringify(value)}`).join('\n')}\n`);
}

function writeArtifacts(directory, version) {
  for (const target of ['darwin-amd64', 'darwin-arm64', 'linux-amd64', 'windows-amd64']) {
    const extension = target.startsWith('windows-') ? 'zip' : 'tar.gz';
    fs.writeFileSync(path.join(directory, `awiki-cli-${version}-${target}.${extension}`), `artifact:${target}\n`);
  }
}

test('stages a complete self-hosted package, manifest, Skill, and onboarding snapshot', () => {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'awiki-stage-release-'));
  try {
    const artifacts = path.join(temp, 'artifacts');
    const output = path.join(temp, 'output');
    const serverConfig = path.join(temp, 'server.toml');
    fs.mkdirSync(artifacts);
    writeServerConfig(serverConfig, temp);
    writeArtifacts(artifacts, '1.0.19-beta.2');

    const result = run(process.execPath, [
      path.join(scriptDir, 'stage-release.js'),
      '--channel', 'beta',
      '--release-config', releaseConfig,
      '--server-config', serverConfig,
      '--artifacts', artifacts,
      '--output', output,
      '--source-tag', 'cli-v1.0.19-beta.2',
      '--source-commit', 'a'.repeat(40),
    ], { cwd: rootDir });
    assert.equal(result.status, 0, result.stderr || result.stdout);

    const manifest = JSON.parse(fs.readFileSync(path.join(output, 'manifest.json'), 'utf8'));
    assert.equal(manifest.latest, '1.0.19-beta.2');
    assert.equal(manifest.installer.url, 'https://downloads.example.com/cli/beta/awiki-cli.tgz');
    assert.deepEqual(Object.keys(manifest.packages).sort(), [
      'darwin-amd64', 'darwin-arm64', 'linux-amd64', 'windows-amd64',
    ]);
    assert.match(manifest.installer.sha256, /^[a-f0-9]{64}$/);

    const packageListing = run('tar', ['-tzf', path.join(output, 'awiki-cli.tgz')]);
    assert.equal(packageListing.status, 0, packageListing.stderr);
    for (const required of [
      'package/package.json', 'package/awiki-release.json',
      'package/scripts/install.js', 'package/scripts/run.js',
    ]) assert.match(packageListing.stdout, new RegExp(`^${required}$`, 'm'));
    assert.doesNotMatch(packageListing.stdout, /publish-server|scripts\/release/);

    const releaseMetadata = run('tar', ['-xOzf', path.join(output, 'awiki-cli.tgz'), 'package/awiki-release.json']);
    assert.equal(releaseMetadata.status, 0, releaseMetadata.stderr);
    const metadata = JSON.parse(releaseMetadata.stdout);
    assert.equal(metadata.default_tenant.backend_base_url, 'https://tenant.example.com');
    assert.equal(metadata.default_tenant.did_host, 'tenant.example.com');

    const onboarding = fs.readFileSync(path.join(output, 'onboarding.md'), 'utf8');
    assert.match(onboarding, /https:\/\/downloads\.example\.com\/cli\/beta\/awiki-cli\.tgz/);
    assert.doesNotMatch(onboarding, /\{\{[A-Z0-9_]+\}\}/);
    const skillListing = run('tar', ['-tzf', path.join(output, 'awiki-cli-skill.tar.gz')]);
    assert.equal(skillListing.status, 0, skillListing.stderr);
    assert.match(skillListing.stdout, /^SKILL\.md$/m);
    assert.match(skillListing.stdout, /^references\/00-installation\.md$/m);
    assert.doesNotMatch(skillListing.stdout, /(^|\/)\._/m);
  } finally {
    fs.rmSync(temp, { recursive: true, force: true });
  }
});

test('rejects missing and unexpected workflow artifacts', () => {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'awiki-stage-release-invalid-'));
  try {
    const artifacts = path.join(temp, 'artifacts');
    const output = path.join(temp, 'output');
    const serverConfig = path.join(temp, 'server.toml');
    fs.mkdirSync(artifacts);
    writeServerConfig(serverConfig, temp);
    writeArtifacts(artifacts, '1.0.19-beta.2');
    fs.rmSync(path.join(artifacts, 'awiki-cli-1.0.19-beta.2-linux-amd64.tar.gz'));

    const baseArgs = [
      path.join(scriptDir, 'stage-release.js'), '--channel', 'beta',
      '--release-config', releaseConfig, '--server-config', serverConfig,
      '--artifacts', artifacts, '--output', output,
      '--source-tag', 'cli-v1.0.19-beta.2', '--source-commit', 'a'.repeat(40),
    ];
    let result = run(process.execPath, baseArgs, { cwd: rootDir });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /missing artifact/);

    fs.writeFileSync(path.join(artifacts, 'unexpected.txt'), 'unexpected');
    result = run(process.execPath, baseArgs, { cwd: rootDir });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /unexpected release artifacts/);
  } finally {
    fs.rmSync(temp, { recursive: true, force: true });
  }
});
