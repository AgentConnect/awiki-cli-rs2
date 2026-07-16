'use strict';

const assert = require('node:assert/strict');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const test = require('node:test');

test('renders isolated HTTP-scope zones for CLI package downloads', () => {
  const script = path.resolve(__dirname, 'render-nginx-download-zones.js');
  const config = path.resolve(__dirname, 'publish-server.example.toml');
  const result = spawnSync(process.execPath, [script, config], { encoding: 'utf8' });

  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /limit_conn_zone \$binary_remote_addr zone=cli_download_per_ip:10m;/);
  assert.match(result.stdout, /limit_conn_zone \$server_name zone=cli_download_total:10m;/);
  assert.doesNotMatch(result.stdout, /limit_conn cli_download/);
  assert.doesNotMatch(result.stdout, /\nserver\s*\{/);
});
