#!/usr/bin/env node
'use strict';

const fs = require('fs');
const path = require('path');
const { spawn } = require('child_process');

function findBinary() {
  const rootDir = path.resolve(__dirname, '..');
  const binDir = path.join(rootDir, 'bin');
  const exeName = process.platform === 'win32' ? 'awiki-cli.exe' : 'awiki-cli';
  return path.join(binDir, exeName);
}

function fileExists(p) {
  try {
    fs.accessSync(p, fs.constants.F_OK);
    return true;
  } catch {
    return false;
  }
}

function getPackageVersion() {
  try {
    const pkg = require(path.resolve(__dirname, '..', 'package.json'));
    return typeof pkg.version === 'string' ? pkg.version : 'unknown';
  } catch {
    return 'unknown';
  }
}

function releaseEnvironment() {
  try {
    const metadata = require(path.resolve(__dirname, '..', 'awiki-release.json'));
    const defaults = metadata.default_tenant || {};
    return {
      AWIKI_CLI_UPDATE_BASE_URL: process.env.AWIKI_CLI_UPDATE_BASE_URL || metadata.update_base_url || '',
      AWIKI_CLI_DEFAULT_BACKEND_BASE_URL: process.env.AWIKI_CLI_DEFAULT_BACKEND_BASE_URL || defaults.backend_base_url || '',
      AWIKI_CLI_DEFAULT_DID_HOST: process.env.AWIKI_CLI_DEFAULT_DID_HOST || defaults.did_host || '',
    };
  } catch {
    return {};
  }
}

function serviceManagerEnvironment(
  platform = process.platform,
  environment = process.env,
) {
  if (platform !== 'linux') {
    return {};
  }

  // The Rust binary keeps real user-systemd writes opt-in so isolated tests can
  // exercise runtime policy without touching the host. The npm entrypoint is
  // the production boundary, so Linux installs enable those backends by default.
  return {
    AWIKI_CLI_ENABLE_SYSTEMD_LISTENER_SERVICE:
      environment.AWIKI_CLI_ENABLE_SYSTEMD_LISTENER_SERVICE || '1',
    AWIKI_CLI_ENABLE_SYSTEMD_HERMES_BRIDGE_SERVICE:
      environment.AWIKI_CLI_ENABLE_SYSTEMD_HERMES_BRIDGE_SERVICE || '1',
  };
}

function run() {
  const binPath = findBinary();

  if (!fileExists(binPath)) {
    const version = getPackageVersion();
    console.error(`[awiki-cli] Binary not found at ${binPath}.`);
    console.error('[awiki-cli] Please download it first, for example:');
    console.error('  npm run install-binary');
    console.error('');
    console.error('If you installed this package globally, you may need to run the command with the same package manager (npm/pnpm/yarn).');
    console.error(`Current package version: ${version}`);
    process.exit(1);
  }

  const args = process.argv.slice(2);
  const child = spawn(binPath, args, {
    stdio: 'inherit',
    env: {
      ...process.env,
      ...releaseEnvironment(),
      ...serviceManagerEnvironment(),
    },
  });

  child.on('exit', code => {
    process.exit(code ?? 1);
  });

  child.on('error', err => {
    console.error(`[awiki-cli] Failed to start binary: ${err.message}`);
    process.exit(1);
  });
}

if (require.main === module) {
  run();
}

module.exports = {
  _internal: {
    findBinary,
    fileExists,
    releaseEnvironment,
    serviceManagerEnvironment,
  },
};
