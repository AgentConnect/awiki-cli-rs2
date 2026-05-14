#!/usr/bin/env node
'use strict';

const fs = require('fs');
const path = require('path');
const os = require('os');
const { spawn, execSync } = require('child_process');

function mapPlatform() {
  const p = process.platform;
  if (p === 'darwin') return 'darwin';
  if (p === 'linux') return 'linux';
  if (p === 'win32') return 'windows';
  throw new Error(`Unsupported platform: ${p}`);
}

function mapArch() {
  const a = process.arch;
  if (a === 'x64') return 'amd64';
  if (a === 'arm64') return 'arm64';
  throw new Error(`Unsupported architecture: ${a}`);
}

function getVersion(pkg) {
  const v = pkg && typeof pkg.version === 'string' ? pkg.version.trim() : '';
  if (!v) {
    throw new Error('version is missing in package.json');
  }
  return v;
}

function getDownloadUrl(version, osName, arch) {
  const archiveBaseName = `awiki-cli-${version}-${osName}-${arch}`;
  const ext = osName === 'windows' ? 'zip' : 'tar.gz';
  const fileName = `${archiveBaseName}.${ext}`;

  const mirror = (process.env.AWIKI_CLI_DOWNLOAD_MIRROR || '').trim();
  const mirrorBase = mirror ? mirror.replace(/\/+$/, '') : '';
  const githubBase = 'https://github.com/AgentConnect/awiki-cli/releases/download'.replace(/\/+$/, '');
  const giteeBase = 'https://gitee.com/agentconnect/awiki-cli/releases/download'.replace(/\/+$/, '');
  const tag = `v${version}`;

  const urls = [];
  const seenUrls = new Set();
  const addUrl = base => {
    if (!base) {
      return;
    }
    const url = `${base}/${tag}/${fileName}`;
    if (seenUrls.has(url)) {
      return;
    }
    seenUrls.add(url);
    urls.push(url);
  };

  // If a mirror is configured, try it first.
  addUrl(mirrorBase);
  // Prefer GitHub, then fall back to Gitee for networks where GitHub release
  // downloads are slow or blocked.
  addUrl(githubBase);
  addUrl(giteeBase);

  return {
    urls,
    fileName,
  };
}

function readPositiveIntEnv(name, fallback) {
  const raw = (process.env[name] || '').trim();
  if (!raw) {
    return fallback;
  }
  const value = Number.parseInt(raw, 10);
  if (!Number.isFinite(value) || value <= 0) {
    return fallback;
  }
  return value;
}

function readNonNegativeIntEnv(name, fallback) {
  const raw = (process.env[name] || '').trim();
  if (!raw) {
    return fallback;
  }
  const value = Number.parseInt(raw, 10);
  if (!Number.isFinite(value) || value < 0) {
    return fallback;
  }
  return value;
}

function isGitHubReleaseUrl(url) {
  try {
    const parsed = new URL(url);
    return parsed.hostname === 'github.com' &&
      parsed.pathname.startsWith('/AgentConnect/awiki-cli/releases/download/');
  } catch {
    return false;
  }
}

function getDownloadOptions(url, sourceIndex, sourceCount) {
  const defaultConnectTimeoutSeconds = readPositiveIntEnv('AWIKI_CLI_DOWNLOAD_CONNECT_TIMEOUT_SECONDS', 10);
  const defaultMaxTimeSeconds = readPositiveIntEnv('AWIKI_CLI_DOWNLOAD_MAX_TIME_SECONDS', 60);
  const options = {
    connectTimeoutSeconds: defaultConnectTimeoutSeconds,
    maxTimeSeconds: defaultMaxTimeSeconds,
    speedLimitBytesPerSecond: 0,
    speedTimeSeconds: 0,
    fastFallback: false,
  };

  if (sourceIndex < sourceCount - 1 && isGitHubReleaseUrl(url)) {
    const githubConnectTimeoutSeconds = readPositiveIntEnv('AWIKI_CLI_GITHUB_FAST_CONNECT_TIMEOUT_SECONDS', 5);
    const githubMaxTimeSeconds = readPositiveIntEnv('AWIKI_CLI_GITHUB_FAST_MAX_TIME_SECONDS', 20);
    options.connectTimeoutSeconds = Math.min(defaultConnectTimeoutSeconds, githubConnectTimeoutSeconds);
    options.maxTimeSeconds = Math.min(defaultMaxTimeSeconds, githubMaxTimeSeconds);
    options.speedLimitBytesPerSecond = readNonNegativeIntEnv('AWIKI_CLI_GITHUB_FAST_SPEED_LIMIT_BYTES', 64 * 1024);
    options.speedTimeSeconds = readPositiveIntEnv('AWIKI_CLI_GITHUB_FAST_SPEED_TIME_SECONDS', 8);
    options.fastFallback = true;
  }

  return options;
}

function formatDownloadOptions(options) {
  if (!options.fastFallback) {
    return '';
  }

  const parts = [`max ${options.maxTimeSeconds}s`];
  if (options.speedLimitBytesPerSecond > 0 && options.speedTimeSeconds > 0) {
    parts.push(`low-speed ${options.speedLimitBytesPerSecond} B/s for ${options.speedTimeSeconds}s`);
  }
  return ` (fast fallback: ${parts.join(', ')})`;
}

function download(url, destPath, options = {}) {
  return new Promise((resolve, reject) => {
    const curlCmd = process.env.AWIKI_CLI_CURL || 'curl';
    const isWindows = process.platform === 'win32';
    const args = [];
    const connectTimeoutSeconds = options.connectTimeoutSeconds || 10;
    const maxTimeSeconds = options.maxTimeSeconds || 60;

    if (isWindows) {
      // On Windows, avoid CRYPT_E_REVOCATION_OFFLINE errors when the
      // certificate revocation list server is unreachable.
      args.push('--ssl-revoke-best-effort');
    }

    args.push(
      '--fail',
      '--location',
      '--silent',
      '--show-error',
      '--connect-timeout',
      String(connectTimeoutSeconds),
      '--max-time',
      String(maxTimeSeconds)
    );

    if (options.speedLimitBytesPerSecond > 0 && options.speedTimeSeconds > 0) {
      args.push(
        '--speed-limit',
        String(options.speedLimitBytesPerSecond),
        '--speed-time',
        String(options.speedTimeSeconds)
      );
    }

    args.push('--output', destPath, url);

    const child = spawn(curlCmd, args, { stdio: ['ignore', 'ignore', 'pipe'] });
    let stderr = '';

    child.stderr.on('data', chunk => {
      stderr += chunk.toString();
    });

    child.on('error', err => {
      if (err && err.code === 'ENOENT') {
        reject(new Error('curl not found. Please install curl or set AWIKI_CLI_CURL to a valid curl binary.'));
      } else {
        reject(err);
      }
    });

    child.on('exit', code => {
      if (code === 0) {
        resolve();
      } else {
        const msg = stderr.trim() || `curl exited with code ${code}`;
        reject(new Error(msg));
      }
    });
  });
}

function ensureDir(dir) {
  fs.mkdirSync(dir, { recursive: true });
}

function isTruthyEnv(name) {
  const value = (process.env[name] || '').trim().toLowerCase();
  return value === '1' || value === 'true' || value === 'yes' || value === 'on';
}

function resolveLocalBinaryPath() {
  const raw = (process.env.AWIKI_CLI_LOCAL_BINARY || '').trim();
  if (!raw) {
    return '';
  }

  if (path.isAbsolute(raw)) {
    return raw;
  }

  return path.resolve(process.cwd(), raw);
}

function fileExists(p) {
  try {
    fs.accessSync(p, fs.constants.F_OK);
    return true;
  } catch {
    return false;
  }
}

function installLocalBinary(sourcePath, destPath, osName) {
  if (!fileExists(sourcePath)) {
    throw new Error(`AWIKI_CLI_LOCAL_BINARY does not exist: ${sourcePath}`);
  }

  const stat = fs.statSync(sourcePath);
  if (!stat.isFile()) {
    throw new Error(`AWIKI_CLI_LOCAL_BINARY must point to a file: ${sourcePath}`);
  }

  fs.copyFileSync(sourcePath, destPath);

  if (osName !== 'windows') {
    try {
      fs.chmodSync(destPath, 0o755);
    } catch {
      // best effort
    }
  }
}

function extractArchive(archivePath, destDir, osName) {
  return new Promise((resolve, reject) => {
    let cmd;
    let args;

    if (osName === 'windows') {
      // Use PowerShell Expand-Archive; requires PowerShell 5+ (Windows 10/11)
      cmd = 'powershell';
      args = [
        '-NoProfile',
        '-NonInteractive',
        '-Command',
        `Expand-Archive -LiteralPath '${archivePath}' -DestinationPath '${destDir}' -Force`,
      ];
    } else {
      cmd = 'tar';
      args = ['-xzf', archivePath, '-C', destDir];
    }

    const child = spawn(cmd, args, { stdio: 'inherit' });
    child.on('error', reject);
    child.on('exit', code => {
      if (code === 0) resolve();
      else reject(new Error(`Extraction command ${cmd} exited with code ${code}`));
    });
  });
}

function applySystemProxyFromMacOS() {
  // If the user已经显式配置了代理，就不要覆盖。
  if (process.env.HTTPS_PROXY || process.env.HTTP_PROXY) {
    return;
  }
  if (process.platform !== 'darwin') {
    return;
  }

  try {
    const output = execSync('scutil --proxy', { encoding: 'utf8' });
    const lines = output.split('\n').map(line => line.trim()).filter(Boolean);
    const values = {};

    for (const line of lines) {
      // 形如: "HTTPSProxy : 127.0.0.1"
      const parts = line.split(':');
      if (parts.length < 2) continue;
      const key = parts[0].trim();
      const value = parts.slice(1).join(':').trim();
      values[key] = value;
    }

    const httpsEnabled = values.HTTPSEnable === '1';
    const httpsHost = values.HTTPSProxy || '';
    const httpsPort = values.HTTPSPort || '';

    let proxyURL = '';
    if (httpsEnabled && httpsHost && httpsPort) {
      proxyURL = `http://${httpsHost}:${httpsPort}`;
    } else {
      const httpEnabled = values.HTTPEnable === '1';
      const httpHost = values.HTTPProxy || '';
      const httpPort = values.HTTPPort || '';
      if (httpEnabled && httpHost && httpPort) {
        proxyURL = `http://${httpHost}:${httpPort}`;
      }
    }

    if (proxyURL) {
      // 只在尚未设置时填充，避免覆盖用户显式配置。
      if (!process.env.HTTPS_PROXY) {
        process.env.HTTPS_PROXY = proxyURL;
      }
      if (!process.env.HTTP_PROXY) {
        process.env.HTTP_PROXY = proxyURL;
      }
    }
  } catch {
    // best-effort：探测失败时忽略，走默认直连行为。
  }
}

async function main() {
  // 在读取 package.json 和执行任何网络请求前，尝试从系统代理设置同步到 env。
  applySystemProxyFromMacOS();

  const rootDir = path.resolve(__dirname, '..');
  const pkgPath = path.join(rootDir, 'package.json');
  const pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf8'));

  const version = getVersion(pkg);
  const osName = mapPlatform();
  const arch = mapArch();
  const { urls, fileName } = getDownloadUrl(version, osName, arch);

  const binDir = path.join(rootDir, 'bin');
  ensureDir(binDir);
  const exeName = osName === 'windows' ? 'awiki-cli.exe' : 'awiki-cli';
  const exePath = path.join(binDir, exeName);
  const localBinaryPath = resolveLocalBinaryPath();

  if (localBinaryPath) {
    console.log(`Installing awiki-cli ${version} from local binary ${localBinaryPath} ...`);
    installLocalBinary(localBinaryPath, exePath, osName);
    console.log(`awiki-cli binary is installed at ${exePath}`);
    return;
  }

  if (isTruthyEnv('AWIKI_CLI_SKIP_DOWNLOAD')) {
    if (fileExists(exePath)) {
      console.log(`Skipping download and reusing existing binary at ${exePath}`);
      return;
    }
    throw new Error(
      `AWIKI_CLI_SKIP_DOWNLOAD is set, but no binary exists at ${exePath}. ` +
      'Set AWIKI_CLI_LOCAL_BINARY=/absolute/path/to/awiki-cli or run without AWIKI_CLI_SKIP_DOWNLOAD.'
    );
  }

  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'awiki-cli-'));
  const archivePath = path.join(tmpDir, fileName);

  let lastError;
  for (let i = 0; i < urls.length; i += 1) {
    const url = urls[i];
    const downloadOptions = getDownloadOptions(url, i, urls.length);
    console.log(`Downloading awiki-cli ${version} for ${osName}/${arch} from ${url}${formatDownloadOptions(downloadOptions)} ...`);
    try {
      await download(url, archivePath, downloadOptions);
      lastError = undefined;
      break;
    } catch (err) {
      lastError = err;
      try {
        fs.rmSync(archivePath, { force: true });
      } catch {
        // best effort cleanup before trying the next source
      }
      console.error(`[awiki-cli] Download failed from ${url}: ${err.message}`);
      if (i < urls.length - 1) {
        console.error('[awiki-cli] Retrying with the next download source ...');
      }
    }
  }

  if (lastError) {
    throw lastError;
  }

  console.log(`Extracting to ${binDir} ...`);
  try {
    await extractArchive(archivePath, binDir, osName);
  } finally {
    try {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    } catch (e) {
      // best effort cleanup
    }
  }

  if (osName !== 'windows') {
    try {
      fs.chmodSync(exePath, 0o755);
    } catch (e) {
      // best effort
    }
  }

  console.log(`awiki-cli binary is installed at ${exePath}`);
}

if (require.main === module) {
  main().catch(err => {
    console.error(`[awiki-cli] Failed to install binary: ${err.message}`);
     console.error(
       '\nIf you are behind a firewall or using a restricted network, you can:\n' +
       '  - The installer automatically tries GitHub releases and then Gitee releases,\n' +
       '  - Set AWIKI_CLI_DOWNLOAD_MIRROR to a reachable HTTPS base URL to override the default order,\n' +
       '  - Or set AWIKI_CLI_LOCAL_BINARY to a local awiki-cli binary for local package testing,\n' +
       '  - Or manually download the archive and extract it into the awiki-cli bin directory.\n'
     );
    process.exit(1);
  });
}

module.exports = {
  _internal: {
    mapPlatform,
    mapArch,
    getVersion,
    getDownloadUrl,
  },
};
