#!/usr/bin/env node
'use strict';
// One CLI release transaction, shared by normal publication and explicit recovery.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const documents = require('./documents.js');
const durable = require('./durable-files.js');
const { readServerConfig } = require('./config.js');
const read = file => JSON.parse(fs.readFileSync(file, 'utf8'));
const hash = file => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
function check(ok, message) { if (!ok) throw new Error(message); }
function journalPath(config, channel) {
  check(['stable', 'beta'].includes(channel), 'invalid release channel');
  return path.join(config.web_root, channel === 'stable' ? 'docs.transaction.json' : 'beta.transaction.json');
}
function ready(config, channel) {
  const journal = journalPath(config, channel);
  if (fs.existsSync(journal)) check(!['pending', 'restoring'].includes(read(journal).status), 'unfinished release; restore before publishing');
}
function validate(directory, config) {
  const m = read(path.join(directory, 'manifest.json'));
  check(['stable', 'beta'].includes(m.channel) && /^\d+\.\d+\.\d+(?:-[\w.-]+)?$/.test(m.latest), 'invalid release version/channel');
  const base = `${config.public_origin}${config.public_base_path}/${m.channel}`;
  const expected = m.channel === 'stable' ? `${base}/releases/${m.latest}/awiki-cli.tgz` : `${base}/awiki-cli.tgz`;
  check(m.installer.url === expected, 'installer URL does not match configured release layout');
  check(hash(path.join(directory, 'awiki-cli.tgz')) === m.installer.sha256, 'installer checksum mismatch');
  const record = documents.validate(path.join(directory, 'documents'));
  check(record.channel === m.channel && record.cli_version === m.latest
    && record.cli_source.commit === m.source.commit && record.installer.url === m.installer.url
    && record.installer.sha256 === m.installer.sha256
    && (record.public_base_path || '/cli') === config.public_base_path, 'release/document binding mismatch');
  for (const p of Object.values(m.packages)) {
    const name = path.basename(new URL(p.url).pathname);
    check(hash(path.join(directory, 'artifacts', name)) === p.sha256, `platform checksum mismatch: ${name}`);
  }
  check(`sha256:${hash(path.join(directory, 'awiki-cli-skill.tar.gz'))}` === m.skill.digest, 'Skill archive checksum mismatch');
  return m;
}
function seal(directory) {
  for (const name of fs.readdirSync(directory)) {
    const file = path.join(directory, name), stat = fs.lstatSync(file);
    if (stat.isDirectory()) seal(file);
    else { check(stat.isFile(), 'release contains non-regular file'); fs.chmodSync(file, 0o444); }
  }
  fs.chmodSync(directory, 0o555);
}
function installerFile(config, m) {
  return path.join(config.installer_root, m.latest, 'awiki-cli.tgz');
}
function install(stage, config) {
  const m = validate(stage, config);
  ready(config, m.channel);
  const dest = path.join(config.archive_root, 'releases', m.latest, m.channel);
  check(!fs.existsSync(dest), `release already installed; reuse with --release activate CONFIG ${dest}`);
  const tmp = `${dest}.staging-${process.pid}`;
  durable.mkdir(path.dirname(dest));
  fs.cpSync(stage, tmp, { recursive: true });
  validate(tmp, config); durable.tree(tmp);
  fs.renameSync(tmp, dest); seal(dest); durable.tree(dest); durable.sync(path.dirname(dest));
  installAssets(dest, config);
  return dest;
}
function installAssets(directory, config) {
  const m = validate(directory, config);
  if (m.channel === 'stable') {
    const file = installerFile(config, m);
    if (fs.existsSync(file)) check(hash(file) === m.installer.sha256, 'immutable installer already exists with different content');
    else durable.write(file, fs.readFileSync(path.join(directory, 'awiki-cli.tgz')), 0o444);
    // Existing production installers refer to this shared, versioned-name store.
    // A channel pointer change must not make their platform archives disappear.
    if (config.stable_artifact_root) {
      for (const artifact of Object.values(m.packages)) {
        const name = path.basename(new URL(artifact.url).pathname);
        const target = path.join(config.stable_artifact_root, name);
        const existing = fs.lstatSync(target, { throwIfNoEntry: false });
        if (existing) check(existing.isFile() && hash(target) === artifact.sha256,
          `immutable platform artifact already exists with different content: ${name}`);
        else durable.write(target, fs.readFileSync(path.join(directory, 'artifacts', name)), 0o444);
      }
    }
  }
  durable.mkdir(path.join(config.archive_root, 'document-releases'));
  const docRelease = documents.install(path.join(directory, 'documents'), config.archive_root);
  return { m, docRelease };
}
function recover(journal) {
  const entry = read(journal);
  check(entry.kind === 'cli-release' && entry.schema_version === 1 && Array.isArray(entry.pointers), 'not a CLI release transaction');
  check(['pending', 'restoring', 'complete', 'restored'].includes(entry.status), 'invalid release status');
  if (entry.status === 'restored') return;
  // Refuse a stale journal after an unrelated manual change; do not partly restore.
  for (const p of entry.pointers) {
    const actual = documents.snapshot(p.pointer);
    check(actual === p.old || actual === p.next, `pointer drift: ${p.pointer}`);
  }
  entry.status = 'restoring'; durable.json(journal, entry);
  for (const p of entry.pointers) durable.restore(p.pointer, p.old);
  entry.status = 'restored'; durable.json(journal, entry);
}
async function activate(directory, config, verify = documents.verify) {
  const m = validate(directory, config);
  check(path.resolve(directory) === path.resolve(config.archive_root, 'releases', m.latest, m.channel),
    'activate requires the installed immutable release directory');
  ready(config, m.channel);
  const { docRelease } = installAssets(directory, config);
  // Verify after switching: some environments serve manifest from the channel,
  // others project it from User Service. Both must match before completion.
  const pointers = [{ pointer: path.join(config.web_root, m.channel), next: path.resolve(directory) }];
  if (m.channel === 'stable') pointers.push({ pointer: path.join(config.web_root, 'docs'), next: docRelease });
  for (const p of pointers) p.old = documents.snapshot(p.pointer);
  const entry = { schema_version: 1, kind: 'cli-release', channel: m.channel, version: m.latest, status: 'pending', pointers };
  const journal = journalPath(config, m.channel);
  // Persist the entire old/new pair before the FIRST public pointer is changed.
  durable.json(journal, entry);
  try {
    for (const p of pointers) durable.link(p.next, p.pointer);
    await verify(docRelease, m.channel === 'stable');
    entry.status = 'complete'; durable.json(journal, entry);
  } catch (error) { recover(journal); throw error; }
}
async function main() {
  const [action, configFile, value] = process.argv.slice(2), config = readServerConfig(configFile);
  if (action === 'check') ready(config, value);
  else if (action === 'install') console.log(install(path.resolve(value), config));
  else if (action === 'activate') await activate(path.resolve(value), config);
  else if (action === 'restore') recover(journalPath(config, value));
  else throw new Error('usage: release-transaction.js check|restore CONFIG CHANNEL | install|activate CONFIG RELEASE');
}
if (require.main === module) main().catch(e => { console.error(e.message); process.exitCode = 1; });
module.exports = { ready, validate, install, activate, recover, journalPath, installerFile };
