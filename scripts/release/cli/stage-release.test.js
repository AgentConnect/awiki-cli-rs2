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
    public_origin: 'https://awiki.info',
    public_base_path: '/cli',
    default_backend_base_url: 'https://awiki.info',
    default_did_host: 'awiki.info',
    web_root: `${root}/web`,
    archive_root: `${root}/archive`,
    nginx_config: `${root}/nginx.conf`,
    nginx_http_snippet: `${root}/nginx-http-snippet.conf`,
    nginx_snippet: `${root}/nginx-snippet.conf`,
    nginx_backup_root: `${root}/backups`,
    protocol_gateway_checkout: `${root}/gateway`,
    protocol_gateway_origin: 'http://127.0.0.1:9896',
    protocol_gateway_service: 'protocol-gateway',
    github_repo: 'AgentConnect/awiki-cli-rs2',
    github_workflow: 'build-cli-release.yml',
    github_token: 'test-token',
    cli_download_max_per_ip: '2',
    cli_download_max_total: '4',
    cli_download_rate_after: '1m',
    cli_download_rate: '512k',
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
    writeArtifacts(artifacts, '1.0.20-beta.1');

    const result = run(process.execPath, [
      path.join(scriptDir, 'stage-release.js'),
      '--channel', 'beta',
      '--release-config', releaseConfig,
      '--server-config', serverConfig,
      '--artifacts', artifacts,
      '--output', output,
      '--source-tag', 'cli-v1.0.20-beta.1',
      '--source-commit', 'a'.repeat(40),
    ], { cwd: rootDir });
    assert.equal(result.status, 0, result.stderr || result.stdout);

    const manifest = JSON.parse(fs.readFileSync(path.join(output, 'manifest.json'), 'utf8'));
    assert.equal(manifest.latest, '1.0.20-beta.1');
    assert.equal(manifest.installer.url, 'https://awiki.info/cli/beta/awiki-cli.tgz');
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
    assert.equal(metadata.default_tenant.backend_base_url, 'https://awiki.info');
    assert.equal(metadata.default_tenant.did_host, 'awiki.info');

    const onboarding = fs.readFileSync(path.join(output, 'onboarding.md'), 'utf8');
    assert.match(onboarding, /https:\/\/awiki\.info\/cli\/beta\/awiki-cli\.tgz/);
    assert.match(onboarding, /AWIKI_SKILL_ONBOARDING_V1/);
    assert.match(onboarding, /awiki-cli onboarding claim/);
    assert.match(onboarding, /--expected-agent-handle/);
    assert.match(onboarding, /--token-stdin/);
    assert.doesNotMatch(onboarding, /awiki\.ai/);
    assert.doesNotMatch(onboarding, /\{\{[A-Z0-9_]+\}\}/);
    const skillListing = run('tar', ['-tzf', path.join(output, 'awiki-cli-skill.tar.gz')]);
    assert.equal(skillListing.status, 0, skillListing.stderr);
    assert.match(skillListing.stdout, /^SKILL\.md$/m);
    assert.match(skillListing.stdout, /^references\/00-installation\.md$/m);
    assert.match(skillListing.stdout, /^references\/01-onboarding\.md$/m);
    assert.match(skillListing.stdout, /^references\/12-mail\.md$/m);
    assert.match(skillListing.stdout, /^references\/13-tenants\.md$/m);
    assert.doesNotMatch(skillListing.stdout, /(^|\/)\._/m);

    const skillEntry = run('tar', ['-xOzf', path.join(output, 'awiki-cli-skill.tar.gz'), 'SKILL.md']);
    assert.equal(skillEntry.status, 0, skillEntry.stderr);
    assert.match(skillEntry.stdout, /AWIKI_SKILL_ONBOARDING_V1/);
    assert.doesNotMatch(skillEntry.stdout, /awiki\.ai/);
    const skillOnboarding = run('tar', [
      '-xOzf', path.join(output, 'awiki-cli-skill.tar.gz'), 'references/01-onboarding.md',
    ]);
    assert.equal(skillOnboarding.status, 0, skillOnboarding.stderr);
    assert.match(skillOnboarding.stdout, /awiki-cli onboarding claim/);
    assert.match(skillOnboarding.stdout, /--token-stdin/);
    assert.doesNotMatch(skillOnboarding.stdout, /awiki\.ai/);
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
    writeArtifacts(artifacts, '1.0.20-beta.1');
    fs.rmSync(path.join(artifacts, 'awiki-cli-1.0.20-beta.1-linux-amd64.tar.gz'));

    const baseArgs = [
      path.join(scriptDir, 'stage-release.js'), '--channel', 'beta',
      '--release-config', releaseConfig, '--server-config', serverConfig,
      '--artifacts', artifacts, '--output', output,
      '--source-tag', 'cli-v1.0.20-beta.1', '--source-commit', 'a'.repeat(40),
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
