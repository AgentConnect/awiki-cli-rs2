'use strict';

const assert = require('node:assert/strict');
const path = require('node:path');
const fs = require('node:fs');
const os = require('node:os');
const { spawnSync } = require('node:child_process');
const test = require('node:test');

test('optional shared stable artifact root preserves the production alias and validates paths', t => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'cli-shared-render-'));
  t.after(() => fs.rmSync(dir, {recursive:true,force:true}));
  const file = path.join(dir, 'server.toml');
  const base = fs.readFileSync(path.join(__dirname, 'publish-server.example.toml'), 'utf8')
    .replaceAll('example.com', 'anpclaw.com')
    .replace('/etc/awiki/cli-document-tenants.json', path.join(__dirname, 'document-tenants.singapore.json'));
  fs.writeFileSync(file, base + '\nstable_artifact_root = "/var/lib/awiki-cli/artifacts"\n');
  const render = () => spawnSync(process.execPath, [path.join(__dirname, 'render-nginx-snippet.js'), file], {encoding:'utf8'});
  const good = render();
  assert.equal(good.status, 0, good.stderr);
  assert.match(good.stdout, /location \^~ \/cli\/stable\/artifacts\/ \{\n    alias \/var\/lib\/awiki-cli\/artifacts\//);
  assert.match(good.stdout, /location \^~ \/cli\/beta\/artifacts\/ \{\n    alias \/var\/www\/awiki-web\/cli\/beta\/artifacts\//);
  for (const invalid of ['', 'relative', '/var/lib/../unsafe', '/tmp/bad;directive']) {
    fs.writeFileSync(file, base + `\nstable_artifact_root = "${invalid}"\n`);
    assert.notEqual(render().status, 0, invalid);
  }
});

test('renders all public CLI routes inside the configured namespace', t => {
  const script = path.resolve(__dirname, 'render-nginx-snippet.js');
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'cli-render-'));
  t.after(() => fs.rmSync(dir, {recursive:true,force:true}));
  const config = path.join(dir, 'server.toml');
  fs.writeFileSync(config, fs.readFileSync(path.join(__dirname, 'publish-server.example.toml'), 'utf8').replaceAll('example.com','anpclaw.com').replace('/etc/awiki/cli-document-tenants.json', path.join(__dirname,'document-tenants.singapore.json')));
  const result = spawnSync(process.execPath, [script, config], { encoding: 'utf8' });

  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /location = \/cli\/onboarding\.md \{/);
  assert.match(result.stdout, /location = \/cli\/skill\.md \{/);
  assert.equal((result.stdout.match(/limit_except GET HEAD \{ deny all; \}/g) || []).length, 2);
  assert.match(result.stdout, /alias \/var\/www\/awiki-web\/cli\/docs\/anpclaw\.com\/onboarding\.md;/);
  assert.match(result.stdout, /location = \/cli\/stable\/ \{\n    rewrite \^ \/cli\/stable\/manifest\.json last;/);
  assert.match(result.stdout, /location = \/cli\/beta\/ \{\n    rewrite \^ \/cli\/beta\/manifest\.json last;/);
  for (const channel of ['stable', 'beta']) {
    const block = new RegExp(
      `location \\^~ \\/cli\\/${channel}\\/artifacts\\/ \\{[\\s\\S]*?`
      + `alias \\/var\\/www\\/awiki-web\\/cli\\/${channel}\\/artifacts\\/;[\\s\\S]*?`
      + 'limit_conn cli_download_per_ip 2;[\\s\\S]*?'
      + 'limit_conn cli_download_total 4;[\\s\\S]*?'
      + 'limit_conn_status 429;[\\s\\S]*?'
      + 'limit_rate_after 1m;[\\s\\S]*?'
      + 'limit_rate 512k;[\\s\\S]*?'
      + 'Cache-Control "public, max-age=31536000, immutable" always;',
    );
    assert.match(result.stdout, block);
  }
  assert.match(result.stdout, /location \^~ \/cli\/ \{/);
  assert.match(result.stdout, /autoindex off;/);
  assert.equal((result.stdout.match(/max-age=31536000, immutable/g) || []).length, 3);
  assert.equal((result.stdout.match(/no-cache, no-store, must-revalidate/g) || []).length, 1);
  assert.doesNotMatch(result.stdout, /location = \/onboarding\.md/);
  assert.doesNotMatch(result.stdout, /location = \/skill\.md/);
});


test('rejects an existing versioned route pointing at a different installer directory',t=>{
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'cli-nginx-conflict-'));
 t.after(()=>fs.rmSync(root,{recursive:true,force:true}));
 const site=path.join(root,'site.conf'),server=path.join(root,'server.toml');
 fs.writeFileSync(site,'location ^~ /cli/stable/releases/ { alias /wrong-directory/; }');
 fs.writeFileSync(server,fs.readFileSync(path.join(__dirname,'publish-server.example.toml'),'utf8')
  .replaceAll('example.com','anpclaw.com')
  .replace('/etc/nginx/conf.d/anpclaw.com.conf',site)
  .replace('/etc/awiki/cli-document-tenants.json',path.join(__dirname,'document-tenants.singapore.json')));
 const result=spawnSync(process.execPath,[path.join(__dirname,'render-nginx-snippet.js'),server],{encoding:'utf8'});
 assert.notEqual(result.status,0);assert.match(result.stderr,/does not match installer_root/);
});

test('generated document method restrictions are accepted by real Nginx', t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'cli-nginx-syntax-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const server = path.join(root, 'server.toml');
  fs.writeFileSync(server, fs.readFileSync(path.join(__dirname, 'publish-server.example.toml'), 'utf8')
    .replaceAll('example.com', 'anpclaw.com')
    .replace('/etc/awiki/cli-document-tenants.json', path.join(__dirname, 'document-tenants.singapore.json')));
  const render = spawnSync(process.execPath, [path.join(__dirname, 'render-nginx-snippet.js'), server], {encoding:'utf8'});
  assert.equal(render.status, 0, render.stderr);
  const config = path.join(root, 'nginx.conf');
  fs.writeFileSync(config, `error_log stderr;\npid ${root}/nginx.pid;\nevents {}\nhttp {\n`
    + 'limit_conn_zone $binary_remote_addr zone=cli_download_per_ip:1m;\n'
    + 'limit_conn_zone $server_name zone=cli_download_total:1m;\n'
    + `server { listen 127.0.0.1:18081;\n${render.stdout}\n}\n}\n`);
  const checked = spawnSync(process.env.NGINX_BIN || 'nginx', ['-t', '-p', root+'/', '-c', config], {encoding:'utf8'});
  assert.ifError(checked.error);
  assert.equal(checked.status, 0, checked.stderr);
});
