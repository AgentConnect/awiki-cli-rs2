#!/usr/bin/env node
'use strict';
// CLI-owned document artifacts. No request Host inference or separate version policy.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const durable = require('./durable-files.js');
const digest = value => crypto.createHash('sha256').update(value).digest('hex');
const read = file => JSON.parse(fs.readFileSync(file, 'utf8'));
const hash = file => digest(fs.readFileSync(file));
const marker = '<!-- AWIKI_TENANT_SETUP -->';
function check(ok, message) { if (!ok) throw new Error(message); }
function origin(value) {
  const u = new URL(value);
  check(u.protocol === 'https:' && u.origin === value && !u.username && !u.password, 'invalid tenant origin');
  check(/^[a-z0-9.-]+$/.test(u.hostname) && !u.port, 'invalid tenant host');
  return u.hostname;
}
function tenants(config) {
  check(config.schema_version === 1 && Array.isArray(config.tenants) && config.tenants.length, 'invalid tenants config');
  const seen = new Set();
  for (const t of config.tenants) {
    const host = origin(t.origin); origin(t.download_origin);
    check(!seen.has(host), 'duplicate tenant'); seen.add(host);
    check(origin(t.backend_origin) === t.did_host && /^[a-z][a-z0-9-]{0,63}$/.test(t.name), 'invalid tenant endpoints or name');
    check(['default', 'setup'].includes(t.mode), 'invalid tenant mode');
    check(t.mode !== 'default' || t.origin === 'https://awiki.me' && t.backend_origin === t.origin, 'only China may use default tenant');
  }
  return config.tenants;
}
function publicPath(value = '/cli') {
  check(typeof value === 'string' && /^\/(?:[a-z0-9._-]+(?:\/[a-z0-9._-]+)*)$/i.test(value)
    && !value.split('/').some(part => part === '.' || part === '..'), 'invalid public base path');
  return value;
}
function checkInstallerUrl(value) {
  check(typeof value === 'string' && value.length, 'installer URL required');
  const url = new URL(value);
  check(url.protocol === 'https:' && !url.username && !url.password, 'invalid installer URL');
}
function render(template, tenant, channel, installerUrl, publicBasePath = '/cli') {
  const base = publicPath(publicBasePath);
  check(template.split(marker).length === 2, 'template must contain exactly one tenant marker');
  const block = tenant.mode === 'default' ? '' : `## Select This Tenant\n\nConfigure and activate this tenant before initializing its isolated workspace.\n\n\`\`\`bash\nawiki-cli tenant setup ${tenant.name} \\\n  --backend-base-url ${tenant.backend_origin} \\\n  --did-host ${tenant.did_host}\n\`\`\`\n`;
  const installer = installerUrl || `${tenant.download_origin}${base}/${channel}/awiki-cli.tgz`;
  checkInstallerUrl(installer);
  const result = template.replace(marker, block).replaceAll('{{AWIKI_CLI_CHANNEL_BASE_URL}}/awiki-cli.tgz', installer).replaceAll('{{AWIKI_CLI_CHANNEL_BASE_URL}}', `${tenant.download_origin}${base}/${channel}`);
  check(!/\{\{[A-Z0-9_]+\}\}/.test(result), 'unresolved document template');
  return result;
}
function prepare({ template, config, manifest, output, documentSource, publicBasePath = '/cli' }) {
  const rows = tenants(config);
  const base = publicPath(publicBasePath);
  check(['stable', 'beta'].includes(manifest.channel) && /^\d+\.\d+\.\d+(?:-[\w.-]+)?$/.test(manifest.latest), 'invalid manifest version/channel');
  check(/^[a-f0-9]{40}$/.test(documentSource) && /^[a-f0-9]{40}$/.test(manifest.source?.commit), 'full document and CLI source commits required');
  check(/^[a-f0-9]{64}$/.test(manifest.installer?.sha256), 'installer digest required');
  checkInstallerUrl(manifest.installer.url);
  check(!fs.existsSync(output), 'document stage already exists');
  fs.mkdirSync(output, { recursive: true });
  const files = {};
  for (const t of rows) {
    const host = origin(t.origin), content = render(template, t, manifest.channel, manifest.installer.url, base);
    fs.mkdirSync(path.join(output, host));
    for (const name of ['onboarding.md', 'skill.md']) {
      const relative = `${host}/${name}`;
      fs.writeFileSync(path.join(output, relative), content); files[relative] = digest(content);
    }
  }
  const record = { schema_version: 1, public_base_path: base, channel: manifest.channel, cli_version: manifest.latest,
    cli_source: manifest.source, installer: manifest.installer, document_source: documentSource,
    template_sha256: digest(template), tenant_config_sha256: digest(JSON.stringify(config)), tenants: config, files };
  fs.writeFileSync(path.join(output, 'release.json'), JSON.stringify(record, null, 2) + '\n');
  return record;
}
function validate(directory) {
  const record = read(path.join(directory, 'release.json'));
  publicPath(record.public_base_path); // Existing schema v1 releases use /cli.
  checkInstallerUrl(record.installer?.url);
  const rows = tenants(record.tenants);
  check(record.tenant_config_sha256 === digest(JSON.stringify(record.tenants)), 'tenant config digest mismatch');
  check(['stable', 'beta'].includes(record.channel), 'invalid document channel');
  check(/^[a-f0-9]{40}$/.test(record.document_source) && /^[a-f0-9]{40}$/.test(record.cli_source?.commit), 'invalid source commits');
  const expected = rows.flatMap(t => ['onboarding.md','skill.md'].map(n => `${origin(t.origin)}/${n}`)).sort();
  check(JSON.stringify(Object.keys(record.files).sort()) === JSON.stringify(expected), 'document file set mismatch');
  for (const file of expected) check(hash(path.join(directory, file)) === record.files[file], `document checksum mismatch: ${file}`);
  for (const t of rows) check(record.files[`${origin(t.origin)}/onboarding.md`] === record.files[`${origin(t.origin)}/skill.md`], 'Skill mirror mismatch');
  return record;
}
async function get(url) {
  const response = await fetch(url, { signal: AbortSignal.timeout(20000), redirect: 'error' });
  check(response.ok, `HTTP ${response.status}: ${url}`); return response;
}
async function verify(directory, publicDocuments = true) {
  const r = validate(directory);
  const base = publicPath(r.public_base_path);
  for (const t of tenants(r.tenants)) {
    const info = await (await get(`${t.download_origin}/user-service/v1/server-info?client_platform=cli`)).json();
    const m = await (await get(`${t.download_origin}${base}/${r.channel}/manifest.json`)).json();
    check(m.latest === r.cli_version && m.installer.sha256 === r.installer.sha256 && m.installer.url === r.installer.url, 'manifest/document binding mismatch');
    if (r.channel === 'stable') {
      const policy = info.client_versions;
      check(policy.policy_origin === t.download_origin && policy.products.cli.enabled && policy.products.cli.recommended_version === r.cli_version && policy.products.cli.installer.sha256 === r.installer.sha256 && policy.products.cli.installer.url === r.installer.url, 'effective policy/document binding mismatch');
    }
    check(digest(Buffer.from(await (await get(r.installer.url)).arrayBuffer())) === r.installer.sha256, 'public installer checksum mismatch');
    if (publicDocuments) for (const name of ['onboarding.md', 'skill.md']) {
      const response = await get(`${t.origin}${base}/${name}`);
      check(/^text\/markdown(?:;|$)/.test(response.headers.get('content-type') || ''), 'Markdown Content-Type mismatch');
      check(/no-store/.test(response.headers.get('cache-control') || ''), 'document cache policy mismatch');
      check(digest(Buffer.from(await response.arrayBuffer())) === r.files[`${origin(t.origin)}/${name}`], 'public document checksum mismatch');
    }
  }
  return r;
}
const link = durable.link;
function snapshot(pointer) { try { check(fs.lstatSync(pointer).isSymbolicLink(), 'existing pointer is not symlink'); return fs.readlinkSync(pointer); } catch(e) { if(e.code === 'ENOENT') return null; throw e; } }
const restore = durable.restore;
function install(stage, archive) {
  validate(stage);
  const id = hash(path.join(stage, 'release.json'));
  const dest = path.resolve(archive, 'document-releases', id);
  if (!fs.existsSync(dest)) {
    durable.mkdir(path.dirname(dest));
    const tmp = `${dest}.tmp-${process.pid}`; fs.cpSync(stage, tmp, { recursive: true }); validate(tmp);
    for (const host of fs.readdirSync(tmp)) {
      const p = path.join(tmp, host);
      if(fs.statSync(p).isDirectory()) { for(const f of fs.readdirSync(p)) fs.chmodSync(path.join(p,f), 0o444); fs.chmodSync(p,0o555); }
      else fs.chmodSync(p,0o444);
    }
    fs.renameSync(tmp,dest); fs.chmodSync(dest,0o555);
  }
  validate(dest); durable.tree(dest); durable.sync(path.dirname(dest)); return dest;
}
async function activate(directory, pointer, verifyFn = verify) {
  const record = validate(directory);
  check(record.channel === 'stable' || path.basename(pointer) !== 'docs', 'Beta cannot change Stable documents');
  const old = snapshot(pointer);
  const journal = `${pointer}.transaction.json`;
  check(!fs.existsSync(journal) || !['pending', 'restoring'].includes(read(journal).status), 'unfinished document activation; restore first');
  const entry = { pointer, old, next: path.resolve(directory), status: 'pending' };
  durable.json(journal, entry);
  try { link(entry.next, pointer); await verifyFn(directory); entry.status = 'complete'; }
  catch (e) { restore(pointer,old); entry.status = 'restored'; throw e; }
  finally { durable.json(journal, entry); }
}
async function main() {
  const [action, ...args] = process.argv.slice(2);
  if(action === 'prepare') {
    const [template, config, manifest, output, documentSource, publicBasePath] = args;
    prepare({ template:fs.readFileSync(template,'utf8'),config:read(config),manifest:read(manifest),output,documentSource,publicBasePath });
  } else if(action === 'install') console.log(install(...args));
  else if(action === 'activate') await activate(...args);
  else if(action === 'verify') await verify(args[0], args[1] !== '--before-activation');
  else if(action === 'restore') {
    const journal = `${args[0]}.transaction.json`, j = read(journal);
    if (j.kind === 'cli-release') require('./release-transaction.js').recover(journal);
    else { restore(j.pointer,j.old); j.status='restored'; durable.json(journal,j); }
  }
  else throw new Error('usage: documents.js prepare TEMPLATE TENANTS MANIFEST OUTPUT SOURCE_SHA [PUBLIC_BASE_PATH] | install STAGE ARCHIVE | activate RELEASE POINTER | verify RELEASE [--before-activation] | restore POINTER');
}
module.exports = { prepare, validate, render, tenants, verify, install, activate, snapshot, restore, link };
if(require.main === module) main().catch(e => { console.error(e.message); process.exitCode=1; });
