'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const test = require('node:test');

const script = path.resolve(__dirname, 'deploy-nginx-config.sh');

function executable(filePath, content) {
  fs.writeFileSync(filePath, content, { mode: 0o755 });
}

function prepare(root) {
  const fakeBin = path.join(root, 'bin');
  const nginxConfig = path.join(root, 'nginx', 'site.conf');
  const httpSnippet = path.join(root, 'nginx', '00-awiki-cli-download-zones.conf');
  const serverSnippet = path.join(root, 'nginx', 'awiki-cli-release.conf');
  const backupRoot = path.join(root, 'backups');
  const serverConfig = path.join(root, 'publish-server.toml');
  const systemctlLog = path.join(root, 'systemctl.log');
  fs.mkdirSync(fakeBin, { recursive: true });
  fs.mkdirSync(path.dirname(nginxConfig), { recursive: true });
  fs.writeFileSync(nginxConfig, `server {\n    include ${serverSnippet};\n}\n`);
  fs.writeFileSync(systemctlLog, '');

  const values = {
    public_origin: 'https://downloads.example.com',
    public_base_path: '/cli',
    default_backend_base_url: 'https://tenant.example.com',
    default_did_host: 'tenant.example.com',
    web_root: path.join(root, 'web'),
    archive_root: path.join(root, 'archive'),
    nginx_config: nginxConfig,
    nginx_http_snippet: httpSnippet,
    nginx_snippet: serverSnippet,
    nginx_backup_root: backupRoot,
    protocol_gateway_checkout: path.join(root, 'gateway'),
    protocol_gateway_origin: 'http://127.0.0.1:9896',
    protocol_gateway_service: 'protocol-gateway',
    github_repo: 'AgentConnect/awiki-cli-rs2',
    github_workflow: 'build-cli-release.yml',
    github_token: 'test-token',
    cli_download_max_per_ip: '2',
    cli_download_max_total: '4',
    cli_download_rate_after: '1m',
    cli_download_rate: '512k',
  };
  fs.writeFileSync(
    serverConfig,
    `${Object.entries(values).map(([key, value]) => `${key} = ${JSON.stringify(value)}`).join('\n')}\n`,
    { mode: 0o600 },
  );

  executable(path.join(fakeBin, 'sudo'), `#!/usr/bin/env bash
set -euo pipefail
command="$1"
shift
if [[ "$command" == "install" ]]; then
  args=()
  while [[ $# -gt 0 ]]; do
    case "$1" in
      -o|-g) shift 2 ;;
      *) args+=("$1"); shift ;;
    esac
  done
  exec install "\${args[@]}"
fi
[[ "$command" == "chown" ]] && exit 0
exec "$command" "$@"
`);
  executable(path.join(fakeBin, 'nginx'), `#!/usr/bin/env bash
set -euo pipefail
if [[ "\${1:-}" == "-t" && -n "\${FAKE_NGINX_FAIL_ONCE_FILE:-}" && ! -e "$FAKE_NGINX_FAIL_ONCE_FILE" ]]; then
  : >"$FAKE_NGINX_FAIL_ONCE_FILE"
  exit 1
fi
exit 0
`);
  executable(path.join(fakeBin, 'systemctl'), `#!/usr/bin/env bash
set -euo pipefail
echo "$*" >>"$FAKE_SYSTEMCTL_LOG"
exit 0
`);
  executable(path.join(fakeBin, 'sha256sum'), `#!/usr/bin/env bash
set -euo pipefail
for file in "$@"; do
  digest="$(shasum -a 256 "$file" | awk '{print $1}')"
  printf '%s  %s\n' "$digest" "$file"
done
`);

  return {
    backupRoot,
    env: {
      ...process.env,
      PATH: `${fakeBin}:${process.env.PATH}`,
      FAKE_SYSTEMCTL_LOG: systemctlLog,
    },
    httpSnippet,
    serverConfig,
    serverSnippet,
    systemctlLog,
  };
}

function deploy(fixture, extraEnv = {}) {
  return spawnSync('bash', [script, '--config', fixture.serverConfig], {
    encoding: 'utf8',
    env: { ...fixture.env, ...extraEnv },
  });
}

test('deploys generated Nginx files once and skips reload when unchanged', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'awiki-cli-nginx-deploy-'));
  try {
    const fixture = prepare(root);
    let result = deploy(fixture);
    assert.equal(result.status, 0, result.stderr || result.stdout);
    assert.match(result.stdout, /config_changed=true/);
    assert.match(result.stdout, /reloaded=true/);
    assert.match(fs.readFileSync(fixture.httpSnippet, 'utf8'), /zone=cli_download_per_ip:10m/);
    const server = fs.readFileSync(fixture.serverSnippet, 'utf8');
    assert.match(server, /location \^~ \/cli\/stable\/artifacts\//);
    assert.match(server, /Cache-Control "public, max-age=31536000, immutable"/);
    assert.deepEqual(fs.readFileSync(fixture.systemctlLog, 'utf8').trim().split('\n'), [
      'reload nginx',
      'is-active --quiet nginx',
    ]);
    assert.equal(fs.readdirSync(fixture.backupRoot).length, 1);

    fs.writeFileSync(fixture.systemctlLog, '');
    result = deploy(fixture);
    assert.equal(result.status, 0, result.stderr || result.stdout);
    assert.match(result.stdout, /config_changed=false/);
    assert.match(result.stdout, /reloaded=false/);
    assert.equal(fs.readFileSync(fixture.systemctlLog, 'utf8'), '');
    assert.equal(fs.readdirSync(fixture.backupRoot).length, 1);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('restores previous state when the candidate fails nginx validation', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'awiki-cli-nginx-rollback-'));
  try {
    const fixture = prepare(root);
    fs.writeFileSync(fixture.serverSnippet, 'existing server snippet\n');
    const failOnce = path.join(root, 'fail-nginx-once');
    const result = deploy(fixture, { FAKE_NGINX_FAIL_ONCE_FILE: failOnce });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /restoring/);
    assert.equal(fs.existsSync(fixture.httpSnippet), false);
    assert.equal(fs.readFileSync(fixture.serverSnippet, 'utf8'), 'existing server snippet\n');
    assert.equal(fs.readFileSync(fixture.systemctlLog, 'utf8'), '');
    assert.equal(fs.readdirSync(fixture.backupRoot).length, 1);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});
