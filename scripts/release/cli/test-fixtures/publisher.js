'use strict';
const fs=require('node:fs');
const path=require('node:path');
const {spawnSync}=require('node:child_process');
function publisher(root, server, artifacts, version, origin, localOrigin) {
  const bin=path.join(root,'bin');fs.mkdirSync(bin);
  const gh=`#!/usr/bin/env node
const fs=require('node:fs'),path=require('node:path');const a=process.argv.slice(2);
if(a[0]==='api') {
 if(a[1].includes('/git/ref/')) console.log('a'.repeat(40));
 else if(a[1].includes('/git/tags/')) process.exit(1);
 else if(a[1].includes('/actions/runs/')) console.log(JSON.stringify({head_sha:'a'.repeat(40),event:'workflow_dispatch',path:'.github/workflows/build-cli-release.yml',display_title:${JSON.stringify(`CLI stable cli-v${version}`)}}));
 else process.exit(2);
} else if(a[0]==='run' && a[1]==='watch') {}
else if(a[0]==='run' && a[1]==='download') fs.cpSync(${JSON.stringify(artifacts)},path.join(a[a.indexOf('--dir')+1],'fixture'),{recursive:true});
else { console.error('unexpected gh command',a);process.exit(2); }
`;
  fs.writeFileSync(path.join(bin,'gh'),gh,{mode:0o755});
  fs.writeFileSync(path.join(bin,'sudo'),'#!/bin/sh\nexec "$@"\n',{mode:0o755});
  // The publisher is Linux-owned; macOS has no flock. No concurrent test publishes.
  if(spawnSync('sh',['-c','command -v flock']).status!==0) fs.writeFileSync(path.join(bin,'flock'),'#!/bin/sh\nexit 0\n',{mode:0o755});
  const bridge=path.join(root,'fetch-bridge.cjs');
  fs.writeFileSync(bridge,`const original=global.fetch;global.fetch=(url,options)=>{const u=new URL(url);if(u.origin!==${JSON.stringify(origin)})throw Error('external request forbidden');return original(${JSON.stringify(localOrigin)}+u.pathname+u.search,options);};`);
  fs.chmodSync(server,0o600);
  return args=>spawnSync('bash',[path.join(__dirname,'../publish-cli-release.sh'),...args],{
    encoding:'utf8',timeout:30000,
    env:{...process.env,PATH:bin+path.delimiter+process.env.PATH,NODE_OPTIONS:`--require ${bridge}`},
  });
}
module.exports={publisher};
