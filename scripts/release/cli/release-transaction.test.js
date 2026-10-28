'use strict';
const assert = require('node:assert/strict');
const test = require('node:test');
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const crypto = require('node:crypto');
const { spawnSync } = require('node:child_process');
const transaction = require('./release-transaction.js');
const documents = require('./documents.js');
const hash = value => crypto.createHash('sha256').update(value).digest('hex');
function cleanup(root) {
  for (const item of fs.readdirSync(root, {withFileTypes:true})) {
    if (item.isDirectory()) cleanup(path.join(root,item.name));
  }
  fs.chmodSync(root,0o755);
}
function fixture(t, channel = 'stable', existing = true, sharedArtifacts = false) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'cli-transaction-'));
  t.after(() => { cleanup(root); fs.rmSync(root, { recursive: true, force: true }); });
  const config = { public_origin:'https://awiki.me', public_base_path:'/cli',
    archive_root:path.join(root,'archive'), web_root:path.join(root,'web'), installer_root:path.join(root,'policy-downloads/cli') };
  fs.mkdirSync(config.web_root);
  if (existing) fs.symlinkSync('/previous-channel',path.join(config.web_root,channel));
  fs.symlinkSync('/previous-docs',path.join(config.web_root,'docs'));
  const stage = path.join(root,'stage');fs.mkdirSync(stage);
  const bytes = Buffer.from('installer'), skill = Buffer.from('skill');
  fs.writeFileSync(path.join(stage,'awiki-cli.tgz'),bytes);fs.writeFileSync(path.join(stage,'awiki-cli-skill.tar.gz'),skill);
  const version = channel === 'stable' ? '1.2.3' : '1.2.3-beta.1';
  const manifest = {channel,latest:version,source:{commit:'a'.repeat(40)},
    installer:{url:`https://awiki.me/cli/${channel}/${channel==='stable'?`releases/${version}/`:''}awiki-cli.tgz`,sha256:hash(bytes)},
    packages:{},skill:{digest:`sha256:${hash(skill)}`}};
  if (sharedArtifacts) {
    config.stable_artifact_root = path.join(root, 'shared-artifacts');
    fs.mkdirSync(config.stable_artifact_root);
    fs.writeFileSync(path.join(config.stable_artifact_root, 'old-version.tar.gz'), 'old-version');
    const name = `awiki-cli-${version}-linux-amd64.tar.gz`, binary = Buffer.from('new-binary');
    fs.mkdirSync(path.join(stage, 'artifacts'));
    fs.writeFileSync(path.join(stage, 'artifacts', name), binary);
    manifest.packages['linux-amd64'] = {url:`https://awiki.me/cli/${channel}/artifacts/${name}`,sha256:hash(binary)};
  }
  fs.writeFileSync(path.join(stage,'manifest.json'),JSON.stringify(manifest));
  documents.prepare({template:'{{AWIKI_CLI_CHANNEL_BASE_URL}}/awiki-cli.tgz\n<!-- AWIKI_TENANT_SETUP -->',
    config:{schema_version:1,tenants:[{origin:'https://awiki.me',download_origin:'https://awiki.me',backend_origin:'https://awiki.me',did_host:'awiki.me',name:'china',mode:'default'}]},
    manifest,output:path.join(stage,'documents'),documentSource:'b'.repeat(40)});
  const release = transaction.install(stage,config);
  return {root,config,stage,release,manifest,channel,pointer:path.join(config.web_root,channel),docs:path.join(config.web_root,'docs'),journal:transaction.journalPath(config,channel)};
}
test('private operator umask leaves new release parents traversable and journals private', async t => {
  const previous = process.umask(0o077);
  let f;
  try { f = fixture(t, 'stable', false); }
  finally { process.umask(previous); }
  for (const dir of [f.config.archive_root, path.join(f.config.archive_root,'releases'),
    path.dirname(f.release), path.join(f.config.archive_root,'document-releases'),
    f.config.installer_root, path.dirname(transaction.installerFile(f.config,f.manifest))]) {
    assert.equal(fs.statSync(dir).mode & 0o777, 0o755, dir);
  }
  assert.equal(fs.statSync(f.root).mode & 0o777, 0o700, 'existing private parent stays private');
  await transaction.activate(f.release,f.config,async()=>{});
  assert.equal(fs.statSync(f.journal).mode & 0o777, 0o600);
});
test('shared stable platform store preserves old archives across activation and rollback', async t => {
  const f = fixture(t, 'stable', true, true);
  const name = path.basename(new URL(f.manifest.packages['linux-amd64'].url).pathname);
  assert.equal(fs.readFileSync(path.join(f.config.stable_artifact_root, name), 'utf8'), 'new-binary');
  await assert.rejects(transaction.activate(f.release, f.config, async () => { throw Error('failed public gate'); }), /failed public gate/);
  assert.equal(fs.readFileSync(path.join(f.config.stable_artifact_root, 'old-version.tar.gz'), 'utf8'), 'old-version');
  assert.equal(fs.readlinkSync(f.pointer), '/previous-channel');
  await transaction.activate(f.release, f.config, async () => {});
  assert.equal(fs.readFileSync(path.join(f.config.stable_artifact_root, name), 'utf8'), 'new-binary');
});
for (const kind of ['different-file', 'symlink']) test(`shared stable platform store rejects ${kind} without activation`, async t => {
  const f = fixture(t, 'stable', true, true);
  const name = path.basename(new URL(f.manifest.packages['linux-amd64'].url).pathname);
  const target = path.join(f.config.stable_artifact_root, name);
  fs.unlinkSync(target);
  if (kind === 'symlink') fs.symlinkSync(path.join(f.release, 'artifacts', name), target);
  else fs.writeFileSync(target, 'conflicting');
  await assert.rejects(transaction.activate(f.release, f.config, async () => {}), /immutable platform artifact/);
  assert.equal(fs.readlinkSync(f.pointer), '/previous-channel');
});
test('shared stable platform store is not used for beta artifacts', t => {
  const f = fixture(t, 'beta', true, true);
  assert.deepEqual(fs.readdirSync(f.config.stable_artifact_root), ['old-version.tar.gz']);
});
function worker(f, hook, recovery = false) {
  const code = `const tx=require(${JSON.stringify(require.resolve('./release-transaction.js'))});
    const durable=require(${JSON.stringify(require.resolve('./durable-files.js'))});
    const config=${JSON.stringify(f.config)};
    ${hook}
    ${recovery ? `tx.recover(${JSON.stringify(f.journal)});` : `tx.activate(${JSON.stringify(f.release)},config,async()=>{}).catch(e=>{console.error(e);process.exitCode=1;});`}`;
  return spawnSync(process.execPath,['-e',code],{encoding:'utf8',timeout:10000});
}
function restoreFresh(f) {
  // This documented entrypoint must also recover a whole normal release.
  const pointer = f.channel === 'stable' ? f.docs : f.pointer;
  const result = spawnSync(process.execPath,[require.resolve('./documents.js'),'restore',pointer],{encoding:'utf8'});
  assert.equal(result.status,0,result.stderr);
}
for (const channel of ['stable','beta']) {
  for (const boundary of ['journal','channel',...(channel==='stable'?['documents']:[])]) {
    test(`${channel}: SIGKILL after ${boundary} recovers all old pointers in a new process`, t=>{
      const f=fixture(t,channel);
      const hook=boundary==='journal'
        ? `const original=durable.json;durable.json=(file,value)=>{original(file,value);if(value.status==='pending')process.kill(process.pid,'SIGKILL');};`
        : `const original=durable.link;durable.link=(target,pointer)=>{original(target,pointer);if(pointer.endsWith('/${boundary==='channel'?channel:'docs'}'))process.kill(process.pid,'SIGKILL');};`;
      const killed=worker(f,hook);assert.equal(killed.signal,'SIGKILL',killed.stderr);
      assert.equal(JSON.parse(fs.readFileSync(f.journal)).status,'pending');
      assert.throws(()=>transaction.ready(f.config,channel),/unfinished/);
      restoreFresh(f);restoreFresh(f);
      assert.equal(fs.readlinkSync(f.pointer),'/previous-channel');
      assert.equal(fs.readlinkSync(f.docs),'/previous-docs');
      assert.equal(JSON.parse(fs.readFileSync(f.journal)).status,'restored');
    });
  }
}
test('recovery itself can be killed between pointers and safely repeated',t=>{
  const f=fixture(t);
  assert.equal(worker(f,`const original=durable.link;durable.link=(target,pointer)=>{original(target,pointer);if(pointer.endsWith('/docs'))process.kill(process.pid,'SIGKILL');};`).signal,'SIGKILL');
  const killed=worker(f,`const original=durable.restore;durable.restore=(pointer,old)=>{original(pointer,old);process.kill(process.pid,'SIGKILL');};`,true);
  assert.equal(killed.signal,'SIGKILL');assert.equal(JSON.parse(fs.readFileSync(f.journal)).status,'restoring');
  restoreFresh(f);assert.equal(fs.readlinkSync(f.pointer),'/previous-channel');assert.equal(fs.readlinkSync(f.docs),'/previous-docs');
});
for(const channel of ['stable','beta']) {
 test(`${channel}: first publication recovery restores absent pointers`,t=>{
  const f=fixture(t,channel,false);
  if(channel==='stable')fs.unlinkSync(f.docs);
  assert.equal(worker(f,`const original=durable.link;durable.link=(...args)=>{original(...args);process.kill(process.pid,'SIGKILL');};`).signal,'SIGKILL');
  restoreFresh(f);assert.equal(fs.existsSync(f.pointer),false);
  if(channel==='stable') assert.equal(fs.existsSync(f.docs),false);
  else assert.equal(fs.readlinkSync(f.docs),'/previous-docs');
 });
}
test('public validation failure rolls back both pointers; installed artifacts support retry',async t=>{
  const f=fixture(t);
  await assert.rejects(transaction.activate(f.release,f.config,async(dir,publicDocs)=>{if(publicDocs)throw Error('public checksum mismatch');}),/checksum/);
  assert.equal(fs.readlinkSync(f.pointer),'/previous-channel');assert.equal(fs.readlinkSync(f.docs),'/previous-docs');
  await transaction.activate(f.release,f.config,async()=>{});
  assert.equal(fs.readlinkSync(f.pointer),f.release);assert.notEqual(fs.readlinkSync(f.docs),'/previous-docs');
  assert.equal(JSON.parse(fs.readFileSync(f.journal)).status,'complete');
});
test('policy mismatch restores both pointers and installs no new policy',async t=>{
  const f=fixture(t);
  await assert.rejects(transaction.activate(f.release,f.config,async()=>{throw Error('effective policy/document binding mismatch');}),/policy/);
  assert.equal(fs.readlinkSync(f.pointer),'/previous-channel');assert.equal(fs.readlinkSync(f.docs),'/previous-docs');
  assert.equal(JSON.parse(fs.readFileSync(f.journal)).status,'restored');
});
test('installation preserves pointers and refuses a different artifact at an immutable URL',async t=>{
  const f=fixture(t);
  const file=transaction.installerFile(f.config,f.manifest);
  assert.equal(hash(fs.readFileSync(file)),f.manifest.installer.sha256);
  assert.equal(fs.readlinkSync(f.pointer),'/previous-channel');assert.equal(fs.readlinkSync(f.docs),'/previous-docs');
  fs.chmodSync(file,0o644);fs.writeFileSync(file,'conflicting installer');
  await assert.rejects(transaction.activate(f.release,f.config,async()=>{}),/immutable installer/);
  assert.equal(fs.readFileSync(file,'utf8'),'conflicting installer');
});
test('recovery rejects unrelated pointer drift before restoring either entry',t=>{
  const f=fixture(t);
  assert.equal(worker(f,`const original=durable.link;durable.link=(target,pointer)=>{original(target,pointer);if(pointer.endsWith('/docs'))process.kill(process.pid,'SIGKILL');};`).signal,'SIGKILL');
  fs.unlinkSync(f.docs);fs.symlinkSync('/manual-change',f.docs);
  assert.throws(()=>transaction.recover(f.journal),/drift/);
  assert.equal(fs.readlinkSync(f.pointer),f.release);assert.equal(fs.readlinkSync(f.docs),'/manual-change');
});


test('activation refuses a mutable staging directory even when its checksums match',async t=>{
 const f=fixture(t);
 await assert.rejects(transaction.activate(f.stage,f.config,async()=>{}),/installed immutable/);
 assert.equal(fs.readlinkSync(f.pointer),'/previous-channel');
 assert.equal(fs.readlinkSync(f.docs),'/previous-docs');
 assert.equal(fs.existsSync(f.journal),false);
});
