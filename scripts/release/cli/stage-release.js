#!/usr/bin/env node
'use strict';

const crypto = require('crypto');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawnSync } = require('child_process');
const { readReleaseConfig, readServerConfig } = require('./config.js');

function die(message) { throw new Error(message); }
function sha256(file) { return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex'); }
function run(command, args, options = {}) {
  const result = spawnSync(command, args, { stdio: 'inherit', ...options });
  if (result.status !== 0) die(`${command} failed with exit code ${result.status}`);
}
function readArchiveFile(archivePath, target, entry) {
  const command = target.startsWith('windows-') ? 'unzip' : 'tar';
  const args = target.startsWith('windows-')
    ? ['-p', archivePath, entry]
    : ['-xOzf', archivePath, entry];
  const result = spawnSync(command, args, { encoding: null, maxBuffer: 1024 * 1024 });
  if (result.status !== 0) {
    die(`${path.basename(archivePath)} is missing ${entry}`);
  }
  return result.stdout;
}
function validateTenantConfig(raw, label) {
  let config;
  try { config = JSON.parse(raw.toString('utf8')); } catch (err) {
    die(`${label} contains invalid JSON: ${err.message}`);
  }
  if (config.schema_version !== 1
      || !['primary', 'secondary'].includes(config.default_slot)
      || !config.tenants || typeof config.tenants !== 'object'
      || Object.keys(config.tenants).sort().join(',') !== 'primary,secondary') {
    die(`${label} is not a two-slot built-in tenant config`);
  }
  for (const slot of ['primary', 'secondary']) {
    const tenant = config.tenants[slot];
    if (!tenant || typeof tenant.display_name !== 'object'
        || !tenant.display_name['zh-CN']?.trim() || !tenant.display_name.en?.trim()) {
      die(`${label} has an invalid ${slot} display name`);
    }
    let origin;
    try { origin = new URL(tenant.backend_origin); } catch {
      die(`${label} has an invalid ${slot} backend origin`);
    }
    const loopback = origin.protocol === 'http:'
      && ['localhost', '127.0.0.1', '[::1]'].includes(origin.hostname);
    const didHost = String(tenant.did_host || '').trim().toLowerCase().replace(/\.$/u, '');
    if ((origin.protocol !== 'https:' && !loopback) || origin.username || origin.password
        || origin.pathname !== '/' || origin.search || origin.hash
        || (!loopback && origin.port) || origin.hostname !== didHost) {
      die(`${label} has inconsistent ${slot} endpoints`);
    }
  }
  if (config.tenants.primary.backend_origin === config.tenants.secondary.backend_origin
      || config.tenants.primary.did_host === config.tenants.secondary.did_host) {
    die(`${label} must contain distinct tenant endpoints`);
  }
  return config;
}
function renderTemplate(text, values, label) {
  let rendered = text;
  for (const [token, value] of Object.entries(values)) rendered = rendered.replaceAll(`{{${token}}}`, value);
  const unresolved = rendered.match(/\{\{[A-Z0-9_]+\}\}/g);
  if (unresolved) die(`${label} contains unresolved release tokens: ${[...new Set(unresolved)].join(', ')}`);
  return rendered;
}
function renderTree(root, values) {
  for (const entry of fs.readdirSync(root, { withFileTypes: true })) {
    const fullPath = path.join(root, entry.name);
    if (entry.isDirectory()) renderTree(fullPath, values);
    else if (entry.isFile() && entry.name.endsWith('.md')) {
      fs.writeFileSync(fullPath, renderTemplate(fs.readFileSync(fullPath, 'utf8'), values, fullPath));
    }
  }
}
function arg(name) {
  const index = process.argv.indexOf(name);
  if (index < 0 || !process.argv[index + 1]) die(`${name} is required`);
  return process.argv[index + 1];
}

function main() {
  const root = path.resolve(__dirname, '../../..');
  const channel = arg('--channel');
  if (!['beta', 'stable'].includes(channel)) die('channel must be beta or stable');
  const releaseConfig = readReleaseConfig(arg('--release-config'));
  const serverConfig = readServerConfig(arg('--server-config'));
  const artifactsDir = path.resolve(arg('--artifacts'));
  const outputDir = path.resolve(arg('--output'));
  const sourceTag = arg('--source-tag');
  const sourceCommit = arg('--source-commit');
  if (!/^[a-f0-9]{40}$/i.test(sourceCommit)) die('source commit must be a full commit SHA');
  const entry = releaseConfig.channels[channel];
  const version = entry.version;
  const channelBaseUrl = `${serverConfig.public_origin}${serverConfig.public_base_path}/${channel}`;
  const templateValues = { AWIKI_CLI_CHANNEL_BASE_URL: channelBaseUrl };
  if (sourceTag !== `cli-v${version}`) die(`source tag ${sourceTag} does not match cli-v${version}`);
  fs.rmSync(outputDir, { recursive: true, force: true });
  fs.mkdirSync(path.join(outputDir, 'artifacts'), { recursive: true });

  const packages = {};
  let tenantConfigRaw = null;
  let tenantConfig = null;
  const expectedArtifacts = new Set(releaseConfig.targets.map(target => {
    const extension = target.startsWith('windows-') ? 'zip' : 'tar.gz';
    return `awiki-cli-${version}-${target}.${extension}`;
  }));
  const unexpectedArtifacts = fs.readdirSync(artifactsDir).filter(name => !expectedArtifacts.has(name));
  if (unexpectedArtifacts.length) die(`unexpected release artifacts: ${unexpectedArtifacts.join(', ')}`);
  for (const target of releaseConfig.targets) {
    const extension = target.startsWith('windows-') ? 'zip' : 'tar.gz';
    const fileName = `awiki-cli-${version}-${target}.${extension}`;
    const source = path.join(artifactsDir, fileName);
    if (!fs.statSync(source, { throwIfNoEntry: false })?.isFile()) die(`missing artifact ${source}`);
    const currentTenantConfigRaw = readArchiveFile(source, target, 'BUILTIN-TENANTS.json');
    validateTenantConfig(currentTenantConfigRaw, `${fileName}/BUILTIN-TENANTS.json`);
    if (tenantConfigRaw === null) {
      tenantConfigRaw = currentTenantConfigRaw;
      tenantConfig = JSON.parse(currentTenantConfigRaw.toString('utf8'));
    } else if (!tenantConfigRaw.equals(currentTenantConfigRaw)) {
      die(`built-in tenant config differs across release artifacts (at ${fileName})`);
    }
    const destination = path.join(outputDir, 'artifacts', fileName);
    fs.copyFileSync(source, destination);
    packages[target] = {
      url: `${serverConfig.public_origin}${serverConfig.public_base_path}/${channel}/artifacts/${fileName}`,
      sha256: sha256(destination),
      size: fs.statSync(destination).size,
    };
  }

  const packageStage = fs.mkdtempSync(path.join(os.tmpdir(), 'awiki-cli-package-'));
  try {
    for (const name of ['package.json', 'LICENSE', 'README.md', 'COMMERCIAL-LICENSING.md']) {
      fs.copyFileSync(path.join(root, name), path.join(packageStage, name));
    }
    fs.cpSync(path.join(root, 'LICENSES'), path.join(packageStage, 'LICENSES'), { recursive: true });
    const tenantConfigSha256 = crypto.createHash('sha256').update(tenantConfigRaw).digest('hex');
    fs.writeFileSync(path.join(packageStage, 'SOURCE.md'), `# AWiki CLI S2 Corresponding Source

Version: ${version}
Tag: ${sourceTag}
Commit: ${sourceCommit}
Source: https://github.com/${serverConfig.github_repo}/tree/${sourceCommit}
Source archive: https://github.com/${serverConfig.github_repo}/archive/${sourceCommit}.tar.gz
Build instructions: https://github.com/${serverConfig.github_repo}/blob/${sourceCommit}/docs/development.md

ANP dependency commit: ${releaseConfig.anp_commit}
Built-in tenant config SHA-256: ${tenantConfigSha256}
ANP source: https://github.com/agent-network-protocol/anp/tree/${releaseConfig.anp_commit}

The source location above identifies the exact revision used to build this
release. The Corresponding Source is provided under GNU AGPLv3 as described in
the accompanying LICENSE file.
`);
    fs.cpSync(path.join(root, 'scripts'), path.join(packageStage, 'scripts'), { recursive: true });
    const packageJsonPath = path.join(packageStage, 'package.json');
    const packageJson = JSON.parse(fs.readFileSync(packageJsonPath, 'utf8'));
    packageJson.version = version;
    packageJson.awikiCli.minSupportedVersion = entry.min_supported_version;
    delete packageJson.scripts['test:installer'];
    fs.writeFileSync(packageJsonPath, `${JSON.stringify(packageJson, null, 2)}\n`);
    const updateBaseUrl = channelBaseUrl;
    fs.writeFileSync(path.join(packageStage, 'awiki-release.json'), `${JSON.stringify({
      schema_version: 1,
      version,
      channel,
      installer_url: `${updateBaseUrl}/awiki-cli.tgz`,
      builtin_tenants: tenantConfig,
      builtin_tenants_sha256: tenantConfigSha256,
      packages,
    }, null, 2)}\n`);
    run('npm', ['pack', '--ignore-scripts', '--pack-destination', outputDir], { cwd: packageStage });
    const generated = path.join(outputDir, `awiki-cli-${version}.tgz`);
    fs.renameSync(generated, path.join(outputDir, 'awiki-cli.tgz'));
  } finally {
    fs.rmSync(packageStage, { recursive: true, force: true });
  }

  const skillStage = fs.mkdtempSync(path.join(os.tmpdir(), 'awiki-cli-skill-'));
  const skillArchive = path.join(outputDir, 'awiki-cli-skill.tar.gz');
  try {
    fs.cpSync(path.join(root, 'skills'), skillStage, { recursive: true });
    renderTree(skillStage, templateValues);
    run('tar', ['-C', skillStage, '-czf', skillArchive, 'SKILL.md', 'references'], {
      env: { ...process.env, COPYFILE_DISABLE: '1' },
    });
  } finally {
    fs.rmSync(skillStage, { recursive: true, force: true });
  }
  const skillDigest = sha256(skillArchive);
  const discoveryDir = path.join(outputDir, '.well-known', 'agent-skills');
  fs.mkdirSync(discoveryDir, { recursive: true });
  fs.writeFileSync(path.join(discoveryDir, 'index.json'), `${JSON.stringify({
    $schema: 'https://schemas.agentskills.io/discovery/0.2.0/schema.json',
    skills: [{
      name: 'awiki-cli',
      type: 'archive',
      description: 'Operate AWiki identities, messages, groups, pages, tenants, and runtime through awiki-cli.',
      url: `${serverConfig.public_origin}${serverConfig.public_base_path}/${channel}/awiki-cli-skill.tar.gz`,
      digest: `sha256:${skillDigest}`,
    }],
  }, null, 2)}\n`);

  fs.writeFileSync(
    path.join(outputDir, 'onboarding.md'),
    renderTemplate(fs.readFileSync(path.join(root, 'onboarding.md'), 'utf8'), templateValues, 'onboarding.md'),
  );
  const manifest = {
    schema_version: 1,
    channel,
    latest: version,
    min_supported_version: entry.min_supported_version,
    published_at: new Date().toISOString(),
    source: { tag: sourceTag, commit: sourceCommit },
    builtin_tenants_sha256: crypto.createHash('sha256').update(tenantConfigRaw).digest('hex'),
    installer: {
      url: `${serverConfig.public_origin}${serverConfig.public_base_path}/${channel}/awiki-cli.tgz`,
      sha256: sha256(path.join(outputDir, 'awiki-cli.tgz')),
      size: fs.statSync(path.join(outputDir, 'awiki-cli.tgz')).size,
    },
    packages,
    skill: {
      version,
      index_url: `${serverConfig.public_origin}${serverConfig.public_base_path}/${channel}/.well-known/agent-skills/index.json`,
      digest: `sha256:${skillDigest}`,
    },
  };
  fs.writeFileSync(path.join(outputDir, 'manifest.json'), `${JSON.stringify(manifest, null, 2)}\n`);
  console.log(JSON.stringify({ version, channel, output: outputDir, manifest }, null, 2));
}

if (require.main === module) {
  try { main(); } catch (err) { console.error(`Error: ${err.message}`); process.exit(1); }
}
