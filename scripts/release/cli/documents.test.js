'use strict';
const assert = require('node:assert/strict');
const test = require('node:test');
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const d = require('./documents.js');
const template = 'Install {{AWIKI_CLI_CHANNEL_BASE_URL}}/awiki-cli.tgz\n<!-- AWIKI_TENANT_SETUP -->';
const manifest = {channel:'stable',latest:'1.0.55',source:{commit:'a'.repeat(40)},installer:{sha256:'b'.repeat(64)}};
const config = JSON.parse(fs.readFileSync(path.join(__dirname,'document-tenants.singapore.json')));
function fixture(t) {
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'cli-docs-'));t.after(()=>fs.rmSync(root,{recursive:true,force:true}));
 const stage=path.join(root,'stage');d.prepare({template,config,manifest,output:stage,documentSource:'c'.repeat(40)});return {root,stage};
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
