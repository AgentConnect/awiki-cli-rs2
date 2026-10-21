'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { spawn, spawnSync } = require('node:child_process');
const { createServer } = require('node:net');
const { once } = require('node:events');
const { readServerConfig } = require('../config.js');
function writable(directory) {
  fs.chmodSync(directory,0o755);
  for(const item of fs.readdirSync(directory,{withFileTypes:true})) if(item.isDirectory())writable(path.join(directory,item.name));
}
async function serve(t, root, configFile, manifest, productionConfig, staticManifest = false) {
  const config=readServerConfig(configFile);
  const infoFile=path.join(root,'policy.json'), manifestFile=path.join(root,'public-manifest.json');
  const info={client_versions:{policy_origin:config.public_origin,products:{cli:{enabled:true,recommended_version:manifest.latest,installer:manifest.installer}}}};
  fs.writeFileSync(infoFile,JSON.stringify(info));fs.writeFileSync(manifestFile,JSON.stringify(manifest));
  const renderer=name=>{
    const result=spawnSync(process.execPath,[path.join(__dirname,'..',name),configFile],{encoding:'utf8'});
    assert.equal(result.status,0,result.stderr);return result.stdout;
  };
  // Keep the actual production exact-manifest / longer Stable-prefix precedence.
  const site=fs.readFileSync(productionConfig,'utf8');
  const manifestRoute=site.match(/location = \/cli\/stable\/manifest.json \{[^{}]*\}/)[0]
    .replace(/proxy_pass [^;]+;/,`alias ${staticManifest ? path.join(config.web_root,'stable/manifest.json') : manifestFile};`);
  const stableRoute=site.match(/location \^~ \/cli\/stable\/ \{[^{}]*\}/)[0]
    .replace(/root [^;]+;/,`root ${path.resolve(config.installer_root,'../../..')};`);
  const existingRoute=config.public_origin==='https://awiki.ai'
    ? `location ^~ /cli/stable/releases/ { alias ${config.installer_root}/; }` : '';
  fs.writeFileSync(config.nginx_config,site+'\n'+existingRoute);
  const snippet=renderer('render-nginx-snippet.js');
  if(existingRoute) assert(!snippet.includes('location ^~ /cli/stable/releases/'));
  const probe=createServer().listen(0,'127.0.0.1');await once(probe,'listening');
  const port=probe.address().port;await new Promise(r=>probe.close(r));
  const file=path.join(root,'nginx.conf');
  fs.writeFileSync(file,`daemon off; pid ${root}/nginx.pid; error_log stderr; events {} http {
    access_log off; ${renderer('render-nginx-download-zones.js')}
    server { listen 127.0.0.1:${port};
      location = /user-service/v1/server-info { alias ${infoFile}; }
      ${manifestRoute} ${stableRoute} ${existingRoute} ${snippet}
    }
  }`);
  const checked=spawnSync('nginx',['-t','-p',root,'-c',file],{encoding:'utf8'});assert.equal(checked.status,0,checked.stderr);
  const child=spawn('nginx',['-p',root,'-c',file]);child.stderr.resume();
  t.after(async()=>{child.kill('SIGQUIT');await once(child,'exit');});
  const realFetch=global.fetch;
  for(let i=0;i<100;i++){try{await realFetch(`http://127.0.0.1:${port}/`);break;}catch{await new Promise(r=>setTimeout(r,20));}}
  global.fetch=async(url,options)=>{
    const parsed=new URL(url);assert.equal(parsed.origin,config.public_origin,'no external requests in this fixture');
    return realFetch(`http://127.0.0.1:${port}${parsed.pathname}${parsed.search}`,options);
  };
  t.after(()=>{global.fetch=realFetch;});
  return {config,infoFile,manifestFile,info,localOrigin:`http://127.0.0.1:${port}`};
}
module.exports={serve,writable};
