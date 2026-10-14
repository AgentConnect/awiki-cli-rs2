'use strict';
const assert = require('node:assert/strict');
const test = require('node:test');
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const d = require('./documents.js');
const crypto = require('node:crypto');
const installerBytes = Buffer.from('test installer artifact');
const installerDigest = crypto.createHash('sha256').update(installerBytes).digest('hex');
const template = 'Install {{AWIKI_CLI_CHANNEL_BASE_URL}}/awiki-cli.tgz\n<!-- AWIKI_TENANT_SETUP -->';
const manifest = {channel:'stable',latest:'1.0.55',source:{commit:'a'.repeat(40)},installer:{url:'https://anpclaw.com/cli/stable/releases/1.0.55/awiki-cli.tgz',sha256:installerDigest}};
const config = JSON.parse(fs.readFileSync(path.join(__dirname,'document-tenants.singapore.json')));
function fixture(t, options = {}) {
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'cli-docs-'));t.after(()=>fs.rmSync(root,{recursive:true,force:true}));
 const stage=path.join(root,'stage');d.prepare({template,config,manifest,output:stage,documentSource:'c'.repeat(40),...options});return {root,stage};
}
test('all reviewed tenants are explicitly rendered and both mirrors match',t=>{
 for(const env of ['shanghai','silicon-valley','singapore']) {
  const c=JSON.parse(fs.readFileSync(path.join(__dirname,`document-tenants.${env}.json`)));
  for(const row of d.tenants(c)) { const text=d.render(template,row,'stable');
   assert(!text.includes('AWIKI_TENANT_SETUP'));assert(!text.includes('{{'));
   if(row.mode==='default') assert(!text.includes('tenant setup'));else assert(text.includes(`--did-host ${row.did_host}`));
  }
 }
 const {stage}=fixture(t);assert.equal(Object.keys(d.validate(stage).files).length,8);
});
test('missing marker, unknown token, invalid tenant and changed digest fail closed',t=>{
 assert.throws(()=>d.render('missing',config.tenants[0],'stable'),/marker/);
 assert.throws(()=>d.render(template+'{{UNKNOWN}}',config.tenants[0],'stable'),/unresolved/);
 assert.throws(()=>d.tenants({...config,tenants:[{...config.tenants[0],did_host:'elsewhere'}]}),/endpoints/);
 assert.throws(()=>d.tenants({...config,tenants:[config.tenants[0],config.tenants[0]]}),/duplicate/);
 const {stage}=fixture(t);fs.appendFileSync(path.join(stage,'anpclaw.com/skill.md'),'bad');assert.throws(()=>d.validate(stage),/checksum/);
});
test('failed activation restores old pointer and records recovery; Beta cannot replace Stable',async t=>{
 const {root,stage}=fixture(t),pointer=path.join(root,'docs');fs.symlinkSync('/previous',pointer);
 await assert.rejects(d.activate(stage,pointer,async()=>{throw Error('policy version mismatch');}),/policy/);
 assert.equal(fs.readlinkSync(pointer),'/previous');assert.equal(JSON.parse(fs.readFileSync(pointer+'.transaction.json')).status,'restored');
 const record=JSON.parse(fs.readFileSync(path.join(stage,'release.json')));record.channel='beta';fs.writeFileSync(path.join(stage,'release.json'),JSON.stringify(record));
 await assert.rejects(d.activate(stage,pointer,async()=>{}),/Beta/);assert.equal(fs.readlinkSync(pointer),'/previous');
});
test('effective policy version or installer mismatch prevents completion',async t=>{
 const {stage}=fixture(t),original=global.fetch;t.after(()=>global.fetch=original);
 global.fetch=async url=>({ok:true,json:async()=>url.includes('server-info')?{client_versions:{policy_origin:'https://anpclaw.com',products:{cli:{enabled:true,recommended_version:'1.0.56',installer:manifest.installer}}}}:{latest:manifest.latest,installer:manifest.installer}});
 await assert.rejects(d.verify(stage,false),/policy/);
 global.fetch=async()=>({ok:true,json:async()=>({latest:'9.9.9',installer:manifest.installer})});
 await assert.rejects(d.verify(stage,false),/binding/);
});

test('document installs the policy-bound versioned artifact while Skill archive URL stays stable',()=>{
 const url='https://anpclaw.com/cli/stable/releases/1.0.58/awiki-cli.tgz';
 const text=d.render(template+'\nSkill {{AWIKI_CLI_CHANNEL_BASE_URL}}',config.tenants[0],'stable',url);
 assert(text.includes(`Install ${url}`));assert(text.includes('Skill https://anpclaw.com/cli/stable'));
});

function publicFixture(t, stage, {manifestInstaller, policyInstaller, installerStatus = 200, bytes = installerBytes, publicBasePath = '/cli'} = {}) {
 const record = d.validate(stage), requests = [], original = global.fetch;
 t.after(() => { global.fetch = original; });
 global.fetch = async url => {
  requests.push(url);
  for (const tenant of record.tenants.tenants) {
   if (url === `${tenant.download_origin}/user-service/v1/server-info?client_platform=cli`)
    return Response.json({client_versions:{policy_origin:tenant.download_origin,products:{cli:{enabled:true,recommended_version:record.cli_version,installer:policyInstaller || record.installer}}}});
   if (url === `${tenant.download_origin}${publicBasePath}/${record.channel}/manifest.json`)
    return Response.json({latest:record.cli_version,installer:manifestInstaller || record.installer});
   for (const name of ['onboarding.md','skill.md']) {
    if (url === `${tenant.origin}${publicBasePath}/${name}`)
     return new Response(fs.readFileSync(path.join(stage,new URL(tenant.origin).hostname,name)),{headers:{'content-type':'text/markdown; charset=utf-8','cache-control':'no-store'}});
   }
  }
  if (url === record.installer.url) return new Response(bytes,{status:installerStatus});
  if (url === manifestInstaller?.url) return new Response(installerBytes);
  return new Response('not found',{status:404});
 };
 return requests;
}
for (const mismatch of ['manifest URL','policy URL','unavailable document URL','installer content']) {
 test(`${mismatch} prevents activation and restores the previous document pointer`,async t => {
  const {root,stage}=fixture(t), pointer=path.join(root,'docs');
  fs.symlinkSync('/previous',pointer);
  const otherInstaller={...manifest.installer,url:'https://anpclaw.com/cli/stable/current-installer.tgz'};
  const options = mismatch === 'manifest URL' ? {manifestInstaller:otherInstaller,installerStatus:404}
   : mismatch === 'policy URL' ? {policyInstaller:otherInstaller}
   : mismatch === 'unavailable document URL' ? {installerStatus:404} : {bytes:Buffer.from('wrong artifact')};
  publicFixture(t,stage,options);
  await assert.rejects(d.activate(stage,pointer),/binding mismatch|HTTP 404|checksum mismatch/);
  assert.equal(fs.readlinkSync(pointer),'/previous');
  assert.equal(JSON.parse(fs.readFileSync(pointer+'.transaction.json')).status,'restored');
 });
}
for (const channel of ['stable','beta']) {
 test(`${channel} renders and verifies the configured public base path`,async t => {
  const publicBasePath='/downloads/awiki-cli';
  const {root,stage}=fixture(t,{publicBasePath,template:template+'\nSkill {{AWIKI_CLI_CHANNEL_BASE_URL}}/awiki-cli-skill.tar.gz',manifest:{...manifest,channel}});
  assert.equal(d.validate(stage).public_base_path,publicBasePath);
  for (const tenant of config.tenants) {
   const text=fs.readFileSync(path.join(stage,new URL(tenant.origin).hostname,'skill.md'),'utf8');
   assert(text.includes(`${tenant.download_origin}${publicBasePath}/${channel}/awiki-cli-skill.tar.gz`));
   assert(text.includes(manifest.installer.url));
  }
  const requests=publicFixture(t,stage,{publicBasePath});
  const stablePointer=path.join(root,'docs');fs.symlinkSync('/previous-stable',stablePointer);
  const pointer=channel==='stable'?stablePointer:path.join(root,'beta-docs');
  await d.activate(stage,pointer);
  assert.equal(fs.readlinkSync(pointer),stage);
  assert(requests.includes(manifest.installer.url));
  assert(requests.some(url=>url.endsWith(`${publicBasePath}/${channel}/manifest.json`)));
  if(channel==='beta') assert.equal(fs.readlinkSync(stablePointer),'/previous-stable');
 });
}
test('legacy records default to /cli and still verify the actual installer',async t=>{
 const {stage}=fixture(t), file=path.join(stage,'release.json');
 const record=JSON.parse(fs.readFileSync(file));delete record.public_base_path;fs.writeFileSync(file,JSON.stringify(record));
 const requests=publicFixture(t,stage);await d.verify(stage);
 assert(requests.includes(manifest.installer.url));
});
test('invalid public base paths and absent installer URLs are rejected',t=>{
 for(const publicBasePath of ['/cli/','/cli/../other','https://example.com','/cli?x','']) {
  assert.throws(()=>fixture(t,{publicBasePath}),/public base path/);
 }
 assert.throws(()=>fixture(t,{manifest:{...manifest,installer:{sha256:installerDigest}}}),/installer URL/);
});
