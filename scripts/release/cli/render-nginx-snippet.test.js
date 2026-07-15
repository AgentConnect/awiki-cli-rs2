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
  assert.match(result.stdout, /location = \/cli\/stable\/ \{\n    alias \/var\/www\/awiki-web\/cli\/stable\/manifest\.json;/);
  assert.match(result.stdout, /location = \/cli\/beta\/ \{\n    alias \/var\/www\/awiki-web\/cli\/beta\/manifest\.json;/);
  assert.match(result.stdout, /location \^~ \/cli\/ \{/);
  assert.match(result.stdout, /autoindex off;/);
  assert.doesNotMatch(result.stdout, /location = \/onboarding\.md/);
  assert.doesNotMatch(result.stdout, /location = \/skill\.md/);
});
