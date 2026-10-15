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

function writeServerConfig(filePath, root, publicBasePath = '/cli') {
  fs.writeFileSync(path.join(root, 'tenants.json'), JSON.stringify({schema_version:1,tenants:[{origin:'https://awiki.info',backend_origin:'https://awiki.info',did_host:'awiki.info',download_origin:'https://awiki.info',name:'test-tenant',mode:'setup'}]}));
  const values = {
    public_origin: 'https://awiki.info',
    public_base_path: publicBasePath,
    default_backend_base_url: 'https://awiki.info',
    default_did_host: 'awiki.info',
    web_root: `${root}/web`,
    archive_root: `${root}/archive`,
    installer_root: `${root}/installers`,
    nginx_config: `${root}/nginx.conf`,
    nginx_http_snippet: `${root}/nginx-http-snippet.conf`,
    nginx_snippet: `${root}/nginx-snippet.conf`,
    nginx_backup_root: `${root}/backups`,
    document_tenants: path.join(root, 'tenants.json'),
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
  const tenantConfig = fs.readFileSync(
    path.join(rootDir, 'config', 'builtin-tenants.default.json'),
  );
  for (const target of ['darwin-amd64', 'darwin-arm64', 'linux-amd64', 'windows-amd64']) {
    const extension = target.startsWith('windows-') ? 'zip' : 'tar.gz';
    const stage = fs.mkdtempSync(path.join(os.tmpdir(), 'awiki-artifact-'));
    const configPath = path.join(stage, 'BUILTIN-TENANTS.json');
    fs.writeFileSync(configPath, tenantConfig);
    const archive = path.join(directory, `awiki-cli-${version}-${target}.${extension}`);
    const result = target.startsWith('windows-')
      ? run('zip', ['-q', '-j', archive, configPath])
      : run('tar', ['-C', stage, '-czf', archive, 'BUILTIN-TENANTS.json']);
    fs.rmSync(stage, { recursive: true, force: true });
    assert.equal(result.status, 0, result.stderr);
  }
}

test('stages a complete self-hosted package, manifest, Skill, and onboarding snapshot', () => {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'awiki-stage-release-'));
  try {
    const expectedRelease = JSON.parse(fs.readFileSync(releaseConfig, 'utf8'));
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
      'package/LICENSE', 'package/LICENSES/Apache-2.0.txt',
      'package/COMMERCIAL-LICENSING.md', 'package/SOURCE.md',
    ]) assert.match(packageListing.stdout, new RegExp(`^${required}$`, 'm'));
    assert.doesNotMatch(packageListing.stdout, /publish-server|scripts\/release/);

    const releaseMetadata = run('tar', ['-xOzf', path.join(output, 'awiki-cli.tgz'), 'package/awiki-release.json']);
    assert.equal(releaseMetadata.status, 0, releaseMetadata.stderr);
    const metadata = JSON.parse(releaseMetadata.stdout);
    assert.equal(metadata.builtin_tenants.default_slot, 'primary');
    assert.equal(metadata.builtin_tenants.tenants.primary.backend_origin, 'https://awiki.me');
    assert.match(metadata.builtin_tenants_sha256, /^[a-f0-9]{64}$/);
    assert.equal(manifest.builtin_tenants_sha256, metadata.builtin_tenants_sha256);

    const sourceDocument = run('tar', ['-xOzf', path.join(output, 'awiki-cli.tgz'), 'package/SOURCE.md']);
    assert.equal(sourceDocument.status, 0, sourceDocument.stderr);
    assert.match(sourceDocument.stdout, /Version: 1\.0\.20-beta\.1/);
    assert.match(sourceDocument.stdout, new RegExp(`Commit: ${'a'.repeat(40)}`));
    assert.match(sourceDocument.stdout, new RegExp(`tree/${'a'.repeat(40)}`));
    assert.match(
      sourceDocument.stdout,
      new RegExp(`ANP dependency commit: ${expectedRelease.anp_commit}`),
    );

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

test('rejects platform artifacts built with different tenant catalogs', () => {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'awiki-stage-release-tenants-'));
  try {
    const artifacts = path.join(temp, 'artifacts');
    const output = path.join(temp, 'output');
    const serverConfig = path.join(temp, 'server.toml');
    fs.mkdirSync(artifacts);
    writeServerConfig(serverConfig, temp);
    writeArtifacts(artifacts, '1.0.20-beta.1');

    const changed = JSON.parse(fs.readFileSync(
      path.join(rootDir, 'config', 'builtin-tenants.default.json'),
      'utf8',
    ));
    changed.tenants.secondary.backend_origin = 'https://other.example';
    changed.tenants.secondary.did_host = 'other.example';
    const stage = fs.mkdtempSync(path.join(os.tmpdir(), 'awiki-artifact-mismatch-'));
    fs.writeFileSync(path.join(stage, 'BUILTIN-TENANTS.json'), `${JSON.stringify(changed)}\n`);
    const archive = path.join(artifacts, 'awiki-cli-1.0.20-beta.1-linux-amd64.tar.gz');
    const packed = run('tar', ['-C', stage, '-czf', archive, 'BUILTIN-TENANTS.json']);
    fs.rmSync(stage, { recursive: true, force: true });
    assert.equal(packed.status, 0, packed.stderr);

    const result = run(process.execPath, [
      path.join(scriptDir, 'stage-release.js'), '--channel', 'beta',
      '--release-config', releaseConfig, '--server-config', serverConfig,
      '--artifacts', artifacts, '--output', output,
      '--source-tag', 'cli-v1.0.20-beta.1', '--source-commit', 'a'.repeat(40),
    ], { cwd: rootDir });

    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /built-in tenant config differs across release artifacts/);
  } finally {
    fs.rmSync(temp, { recursive: true, force: true });
  }
});

for (const channel of ['stable', 'beta']) {
  test(`stages ${channel} documents and Nginx routes under the configured public path`, t => {
    const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'awiki-stage-custom-path-'));
    t.after(() => fs.rmSync(temp, { recursive: true, force: true }));
    const publicBasePath = '/downloads/awiki-cli';
    const artifacts = path.join(temp, 'artifacts'), output = path.join(temp, 'output');
    const serverConfig = path.join(temp, 'server.toml');
    const version = JSON.parse(fs.readFileSync(releaseConfig)).channels[channel].version;
    fs.mkdirSync(artifacts);
    writeServerConfig(serverConfig, temp, publicBasePath);
    writeArtifacts(artifacts, version);
    const result = run(process.execPath, [path.join(scriptDir, 'stage-release.js'),
      '--channel', channel, '--release-config', releaseConfig, '--server-config', serverConfig,
      '--artifacts', artifacts, '--output', output, '--source-tag', `cli-v${version}`,
      '--source-commit', 'a'.repeat(40)], { cwd: rootDir });
    assert.equal(result.status, 0, result.stderr || result.stdout);
    const record = require('./documents.js').validate(path.join(output, 'documents'));
    assert.equal(record.public_base_path, publicBasePath);
    const document = fs.readFileSync(path.join(output, 'documents/awiki.info/skill.md'), 'utf8');
    const baseUrl = `https://awiki.info${publicBasePath}/${channel}`;
    const installerUrl = channel === 'stable' ? `${baseUrl}/releases/${version}/awiki-cli.tgz` : `${baseUrl}/awiki-cli.tgz`;
    assert.equal(record.installer.url, installerUrl);
    assert(document.includes(installerUrl));
    const metadata = run('tar', ['-xOzf', path.join(output,'awiki-cli.tgz'), 'package/awiki-release.json']);
    assert.equal(JSON.parse(metadata.stdout).installer_url, installerUrl);
    assert(document.includes(`npx skills add ${baseUrl} --agent`));
    const index = JSON.parse(fs.readFileSync(path.join(output, '.well-known/agent-skills/index.json')));
    assert.equal(index.skills[0].url, `${baseUrl}/awiki-cli-skill.tar.gz`);
    assert(!document.includes('https://awiki.info/cli/'));
    const nginx = run(process.execPath, [path.join(scriptDir, 'render-nginx-snippet.js'), serverConfig]);
    assert.equal(nginx.status, 0, nginx.stderr);
    for (const name of ['onboarding.md', 'skill.md']) assert(nginx.stdout.includes(`location = ${publicBasePath}/${name}`));
  });
}

for (const host of ['awiki.me', 'awiki.ai', 'anpclaw.com']) {
  test(`${host} versioned policy publishes staged CLI and documents through real Nginx`, async t => {
    const {serve,writable}=require('./test-fixtures/nginx-policy.js');
    const transaction=require('./release-transaction.js');
    const temp=fs.mkdtempSync(path.join(os.tmpdir(),'cli-production-layout-'));
    const artifacts=path.join(temp,'artifacts'),output=path.join(temp,'stage'),server=path.join(temp,'server.toml');
    fs.mkdirSync(artifacts);writeServerConfig(server,temp);
    fs.writeFileSync(server,fs.readFileSync(server,'utf8').replaceAll('awiki.info',host).replace(`${temp}/installers`,`${temp}/policy/cli/stable/releases`));
    const tenantsFile=path.join(temp,'tenants.json');fs.writeFileSync(tenantsFile,fs.readFileSync(tenantsFile,'utf8').replaceAll('awiki.info',host));
    const version=JSON.parse(fs.readFileSync(releaseConfig)).channels.stable.version;
    writeArtifacts(artifacts,version);
    const staged=run(process.execPath,[path.join(scriptDir,'stage-release.js'),'--channel','stable','--release-config',releaseConfig,'--server-config',server,'--artifacts',artifacts,'--output',output,'--source-tag',`cli-v${version}`,'--source-commit','a'.repeat(40)]);
    assert.equal(staged.status,0,staged.stderr);
    const manifest=JSON.parse(fs.readFileSync(path.join(output,'manifest.json')));
    assert.equal(manifest.installer.url,`https://${host}/cli/stable/releases/${version}/awiki-cli.tgz`);
    const fixture=await serve(t,temp,server,manifest,path.resolve(rootDir,`../awiki-web/deploy/nginx/${host==='awiki.me'?'awiki.me':'awiki.ai'}.conf`),host==='anpclaw.com');
    t.after(()=>{writable(temp);fs.rmSync(temp,{recursive:true,force:true});});
    fs.mkdirSync(fixture.config.web_root,{recursive:true});
    fs.symlinkSync('/previous-cli',path.join(fixture.config.web_root,'stable'));
    fs.symlinkSync('/previous-docs',path.join(fixture.config.web_root,'docs'));
    const publish=require('./test-fixtures/publisher.js').publisher(temp,server,artifacts,version,fixture.config.public_origin,fixture.localOrigin);
    const result=publish(['--config',server,'--run-id','1',...(host==='awiki.me'?['--stage-only']:[]),'stable']);
    assert.equal(result.status,0,result.stderr || result.stdout);
    const release=path.join(fixture.config.archive_root,'releases',version,'stable');
    if(host==='awiki.me') {
      assert.equal(fs.readlinkSync(path.join(fixture.config.web_root,'stable')),'/previous-cli');
      const activated=publish(['--release','activate',server,release]);
      assert.equal(activated.status,0,activated.stderr);
    }
    const downloaded=await fetch(manifest.installer.url);
    assert.equal(downloaded.status,200);
    assert.deepEqual(Buffer.from(await downloaded.arrayBuffer()),fs.readFileSync(path.join(output,'awiki-cli.tgz')));
    assert.equal(fs.readlinkSync(path.join(fixture.config.web_root,'stable')),release);
    assert.equal((await fetch(`https://${host}/cli/skill.md`)).status,200);
    const index=await (await fetch(`https://${host}/cli/stable/.well-known/agent-skills/index.json`)).json();
    assert.equal(index.skills[0].url,`https://${host}/cli/stable/awiki-cli-skill.tar.gz`);
    assert.equal((await fetch(index.skills[0].url)).status,200);
    assert.equal((await fetch(`https://${host}/cli/stable/awiki-cli.tgz`)).status,200);
    // Validation through actual HTTP rolls back a policy mismatch.
    transaction.recover(transaction.journalPath(fixture.config,'stable'));
    fixture.info.client_versions.products.cli.installer={...manifest.installer,url:`https://${host}/cli/stable/awiki-cli.tgz`};
    fs.writeFileSync(fixture.infoFile,JSON.stringify(fixture.info));
    await assert.rejects(transaction.activate(release,fixture.config),/binding mismatch/);
    assert.equal(fs.readlinkSync(path.join(fixture.config.web_root,'stable')),'/previous-cli');
    fixture.info.client_versions.products.cli.installer=manifest.installer;
    fs.writeFileSync(fixture.infoFile,JSON.stringify(fixture.info));
    // Reuse the exact immutable release after failure; no repacking or new policy.
    await transaction.activate(release,fixture.config);
    assert.equal(JSON.parse(fs.readFileSync(transaction.journalPath(fixture.config,'stable'))).status,'complete');
  });
}
