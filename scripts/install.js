#!/usr/bin/env node
'use strict';

const crypto = require('crypto');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawn } = require('child_process');

const HOST_ARTIFACT_CANDIDATES = Object.freeze({
  'darwin-amd64': ['darwin-amd64'],
  'darwin-arm64': ['darwin-arm64'],
  'linux-amd64': ['linux-amd64'],
  'windows-amd64': ['windows-amd64'],
  'windows-arm64': ['windows-arm64', 'windows-amd64'],
});

function normalizeArchitecture(machine) {
  const normalizedMachine = String(machine).trim().toLowerCase();
  return ['x64', 'x86_64', 'amd64'].includes(normalizedMachine)
    ? 'amd64'
    : ['arm64', 'aarch64'].includes(normalizedMachine) ? 'arm64' : '';
}

function detectHostArchitecture(
  machineReader = typeof os.machine === 'function' ? os.machine : null,
  fallbackArchitecture = process.arch,
) {
  let machine = '';
  if (typeof machineReader === 'function') {
    try {
      machine = machineReader();
    } catch {
      machine = '';
    }
  }

  // Some Windows ARM64 Node/libuv versions report `unknown` from
  // os.machine(). process.arch still identifies an executable architecture.
  return normalizeArchitecture(machine) || normalizeArchitecture(fallbackArchitecture);
}

function mapHost(
  platform = process.platform,
  architecture = detectHostArchitecture(),
) {
  const osName = platform === 'darwin' ? 'darwin' : platform === 'linux' ? 'linux' : platform === 'win32' ? 'windows' : '';
  const archName = normalizeArchitecture(architecture);
  const target = osName && archName ? `${osName}-${archName}` : '';
  if (!target || !Object.hasOwn(HOST_ARTIFACT_CANDIDATES, target)) {
    throw new Error(`Unsupported platform: ${platform}/${architecture || 'unknown'}`);
  }
  return { osName, archName, hostTarget: target };
}

function artifactCandidates(hostTarget) {
  const candidates = HOST_ARTIFACT_CANDIDATES[hostTarget];
  if (!candidates) {
    throw new Error(`Unsupported host target: ${hostTarget}`);
  }
  return [...candidates];
}

function validArtifact(artifact) {
  return artifact
    && typeof artifact === 'object'
    && typeof artifact.url === 'string'
    && artifact.url.trim().length > 0
    && /^[a-f0-9]{64}$/i.test(artifact.sha256 || '');
}

function selectArtifactForHost(host, packages) {
  const candidates = artifactCandidates(host.hostTarget);
  for (const artifactTarget of candidates) {
    if (!Object.hasOwn(packages, artifactTarget)) {
      continue;
    }
    const artifact = packages[artifactTarget];
    if (!validArtifact(artifact)) {
      throw new Error(`awiki-release.json has an invalid package entry for ${artifactTarget}`);
    }
    return {
      artifact,
      artifactTarget,
      compatibilityFallback: artifactTarget !== host.hostTarget,
    };
  }
  throw new Error(
    `awiki-release.json has no valid package entry for ${host.hostTarget} (tried: ${candidates.join(', ')})`,
  );
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

function buildCurlArgs(url, destination, platform = process.platform) {
  if (!/^https:\/\//i.test(url) && !/^http:\/\/(127\.0\.0\.1|localhost)(:\d+)?\//i.test(url)) {
    throw new Error(`Release artifact URL must use HTTPS: ${url}`);
  }
  const args = ['--fail', '--location', '--silent', '--show-error', '--connect-timeout', '10', '--max-time', '180'];

  // Schannel revocation lookups can bypass curl's proxy and block before the
  // HTTP request. CA, hostname, and validity checks remain enabled, and the
  // downloaded archive is checked against the digest shipped in this package.
  if (platform === 'win32') args.push('--ssl-no-revoke');
  args.push('--output', destination, url);
  return args;
}

async function download(url, destination) {
  const curl = process.env.AWIKI_CLI_CURL || 'curl';
  const args = buildCurlArgs(url, destination);
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

function binaryProbeEnvironment(metadata, tempDir) {
  const probeHome = path.join(tempDir, 'probe-home');
  const probeWorkspace = path.join(probeHome, '.awiki-cli');
  const defaults = metadata.default_tenant || {};
  fs.mkdirSync(probeHome, { recursive: true });
  return {
    ...process.env,
    HOME: probeHome,
    USERPROFILE: probeHome,
    AWIKI_CLI_WORKSPACE_HOME_DIR: probeWorkspace,
    AWIKI_CLI_UPDATE_CACHE_ONLY: '1',
    AWIKI_CLI_UPDATE_BASE_URL: metadata.update_base_url || '',
    AWIKI_CLI_DEFAULT_BACKEND_BASE_URL: defaults.backend_base_url || '',
    AWIKI_CLI_DEFAULT_DID_HOST: defaults.did_host || '',
  };
}

async function main() {
  const rootDir = path.resolve(__dirname, '..');
  const host = mapHost();
  const { osName } = host;
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
  const { artifact, artifactTarget, compatibilityFallback } =
    selectArtifactForHost(host, metadata.packages);
  if (compatibilityFallback) {
    console.log(
      `[awiki-cli] ${host.hostTarget} host detected; using the ${artifactTarget} compatibility package. `
      + 'Windows 11 x64 app emulation is required.',
    );
  }

  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'awiki-cli-'));
  const archivePath = path.join(tempDir, path.basename(new URL(artifact.url).pathname) || 'awiki-cli.archive');
  try {
    console.log(`Downloading awiki-cli ${metadata.version} for ${artifactTarget} from ${artifact.url} ...`);
    await download(artifact.url, archivePath);
    const actualDigest = sha256File(archivePath);
    if (actualDigest.toLowerCase() !== artifact.sha256.toLowerCase()) {
      throw new Error(`SHA-256 mismatch for ${artifactTarget}: expected ${artifact.sha256}, got ${actualDigest}`);
    }
    await extract(archivePath, binDir, osName);
    if (!fs.existsSync(binaryPath)) throw new Error(`Archive did not contain ${path.basename(binaryPath)}`);
    if (osName !== 'windows') fs.chmodSync(binaryPath, 0o755);
    // `version` resolves workspace configuration. Probe inside the installer
    // temp directory so postinstall cannot initialize or alter the user's
    // real workspace before the package wrapper supplies release defaults.
    try {
      await runCommand(binaryPath, ['version'], { env: binaryProbeEnvironment(metadata, tempDir) });
    } catch (err) {
      if (compatibilityFallback) {
        throw new Error(
          `${artifactTarget} compatibility binary could not run on ${host.hostTarget}. `
          + `Windows 11 x64 app emulation is required: ${err.message}`,
        );
      }
      throw err;
    }
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

module.exports = {
  _internal: {
    artifactCandidates,
    buildCurlArgs,
    detectHostArchitecture,
    mapHost,
    normalizeArchitecture,
    readReleaseMetadata,
    selectArtifactForHost,
    sha256File,
    binaryProbeEnvironment,
  },
};
