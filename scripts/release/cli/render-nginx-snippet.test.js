'use strict';

const assert = require('node:assert/strict');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const test = require('node:test');

test('renders all public CLI routes inside the configured namespace', () => {
  const script = path.resolve(__dirname, 'render-nginx-snippet.js');
  const config = path.resolve(__dirname, 'publish-server.example.toml');
  const result = spawnSync(process.execPath, [script, config], { encoding: 'utf8' });

  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /location = \/cli\/onboarding\.md \{/);
  assert.match(result.stdout, /location = \/cli\/skill\.md \{/);
  assert.match(result.stdout, /proxy_pass http:\/\/127\.0\.0\.1:9896\/cli\/onboarding\.md;/);
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
  assert.equal((result.stdout.match(/max-age=31536000, immutable/g) || []).length, 2);
  assert.equal((result.stdout.match(/no-cache, no-store, must-revalidate/g) || []).length, 1);
  assert.doesNotMatch(result.stdout, /location = \/onboarding\.md/);
  assert.doesNotMatch(result.stdout, /location = \/skill\.md/);
});
