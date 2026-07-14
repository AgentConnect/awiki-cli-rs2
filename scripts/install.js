#!/usr/bin/env node
'use strict';

const crypto = require('crypto');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawn } = require('child_process');

const SUPPORTED_TARGETS = new Set([
  'darwin-amd64',
  'darwin-arm64',
  'linux-amd64',
  'windows-amd64',
]);

function mapTarget(platform = process.platform, arch = process.arch) {
  const osName = platform === 'darwin' ? 'darwin' : platform === 'linux' ? 'linux' : platform === 'win32' ? 'windows' : '';
  const archName = arch === 'x64' ? 'amd64' : arch === 'arm64' ? 'arm64' : '';
  const target = osName && archName ? `${osName}-${archName}` : `${platform}-${arch}`;
  if (!SUPPORTED_TARGETS.has(target)) {
    throw new Error(`Unsupported platform: ${platform}/${arch}. Supported targets: ${[...SUPPORTED_TARGETS].join(', ')}`);
  }
  return { osName, archName, target };
}

function readReleaseMetadata(rootDir) {
  const metadataPath = path.join(rootDir, 'awiki-release.json');
  let metadata;
  try {
    metadata = JSON.parse(fs.readFileSync(metadataPath, 'utf8'));
  } catch (err) {
    throw new Error(`Cannot read ${metadataPath}: ${err.message}`);
  }
  if (metadata.schema_version !== 1 || !metadata.version || !metadata.packages || typeof metadata.packages !== 'object') {
    throw new Error('awiki-release.json is missing schema_version=1, version, or packages');
  }
  return metadata;
}

function sha256File(filePath) {
  const hash = crypto.createHash('sha256');
  hash.update(fs.readFileSync(filePath));
  return hash.digest('hex');
}

function runCommand(command, args, options = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { stdio: 'inherit', ...options });
    child.on('error', reject);
    child.on('exit', code => code === 0 ? resolve() : reject(new Error(`${command} exited with code ${code}`)));
  });
}

async function download(url, destination) {
  if (!/^https:\/\//i.test(url) && !/^http:\/\/(127\.0\.0\.1|localhost)(:\d+)?\//i.test(url)) {
    throw new Error(`Release artifact URL must use HTTPS: ${url}`);
  }
  const curl = process.env.AWIKI_CLI_CURL || 'curl';
  const args = ['--fail', '--location', '--silent', '--show-error', '--connect-timeout', '10', '--max-time', '180'];
  if (process.platform === 'win32') args.push('--ssl-revoke-best-effort');
  args.push('--output', destination, url);
  await runCommand(curl, args);
}

async function extract(archivePath, destination, osName) {
  if (osName === 'windows') {
    const escapedArchive = archivePath.replaceAll("'", "''");
    const escapedDestination = destination.replaceAll("'", "''");
    await runCommand('powershell', [
      '-NoProfile',
      '-NonInteractive',
      '-Command',
      `Expand-Archive -LiteralPath '${escapedArchive}' -DestinationPath '${escapedDestination}' -Force`,
    ]);
    return;
  }
  await runCommand('tar', ['-xzf', archivePath, '-C', destination]);
}

function installLocalBinary(source, destination, osName) {
  const stat = fs.statSync(source);
  if (!stat.isFile()) throw new Error(`AWIKI_CLI_LOCAL_BINARY must point to a file: ${source}`);
  fs.copyFileSync(source, destination);
  if (osName !== 'windows') fs.chmodSync(destination, 0o755);
}

async function main() {
  const rootDir = path.resolve(__dirname, '..');
  const { osName, target } = mapTarget();
  const binDir = path.join(rootDir, 'bin');
  const binaryPath = path.join(binDir, osName === 'windows' ? 'awiki-cli.exe' : 'awiki-cli');
  fs.mkdirSync(binDir, { recursive: true });

  const localBinary = (process.env.AWIKI_CLI_LOCAL_BINARY || '').trim();
  if (localBinary) {
    installLocalBinary(path.resolve(localBinary), binaryPath, osName);
    console.log(`awiki-cli binary is installed at ${binaryPath}`);
    return;
  }

  const metadata = readReleaseMetadata(rootDir);
  const artifact = metadata.packages[target];
  if (!artifact || !artifact.url || !/^[a-f0-9]{64}$/i.test(artifact.sha256 || '')) {
    throw new Error(`awiki-release.json has no valid package entry for ${target}`);
  }

  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'awiki-cli-'));
  const archivePath = path.join(tempDir, path.basename(new URL(artifact.url).pathname) || 'awiki-cli.archive');
  try {
    console.log(`Downloading awiki-cli ${metadata.version} for ${target} from ${artifact.url} ...`);
    await download(artifact.url, archivePath);
    const actualDigest = sha256File(archivePath);
    if (actualDigest.toLowerCase() !== artifact.sha256.toLowerCase()) {
      throw new Error(`SHA-256 mismatch for ${target}: expected ${artifact.sha256}, got ${actualDigest}`);
    }
    await extract(archivePath, binDir, osName);
    if (!fs.existsSync(binaryPath)) throw new Error(`Archive did not contain ${path.basename(binaryPath)}`);
    if (osName !== 'windows') fs.chmodSync(binaryPath, 0o755);
    await runCommand(binaryPath, ['version'], { env: { ...process.env, AWIKI_CLI_UPDATE_CACHE_ONLY: '1' } });
    console.log(`awiki-cli binary is installed at ${binaryPath}`);
  } finally {
    fs.rmSync(tempDir, { recursive: true, force: true });
  }
}

if (require.main === module) {
  main().catch(err => {
    console.error(`[awiki-cli] Failed to install binary: ${err.message}`);
    process.exit(1);
  });
}

module.exports = { _internal: { SUPPORTED_TARGETS, mapTarget, readReleaseMetadata, sha256File } };
