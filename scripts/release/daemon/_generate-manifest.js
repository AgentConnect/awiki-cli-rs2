#!/usr/bin/env node
const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

function usage() {
  console.error(`Usage:
  node scripts/release/daemon/_generate-manifest.js --version VERSION [--min-supported VERSION] [--dist DIR] [--output FILE] [--download-base-urls FILE] [--download-base-url URL] [--allow-partial]`);
}

function die(message) {
  console.error(`Error: ${message}`);
  process.exit(1);
}

function validateVersionSegment(value, fieldName) {
  if (!value || value.startsWith('.') || value.includes('..') || !/^[A-Za-z0-9._-]+$/.test(value)) {
    die(`${fieldName} contains unsupported characters`);
  }
}

let version = '';
let minSupported = '';
let distDir = path.join(process.cwd(), 'dist', 'daemon');
let output = '';
let allowPartial = false;
const downloadBaseUrls = [];

function addDownloadBaseUrls(raw) {
  if (!raw) {
    return;
  }
  for (const part of raw.replace(/,/g, '\n').split(/\r?\n/)) {
    const value = part.trim().replace(/\/+$/, '');
    if (!value || downloadBaseUrls.includes(value)) {
      continue;
    }
    downloadBaseUrls.push(value);
  }
}

for (let i = 2; i < process.argv.length; i += 1) {
  const arg = process.argv[i];
  const next = () => {
    i += 1;
    if (i >= process.argv.length || !process.argv[i]) {
      die(`${arg} requires a value`);
    }
    return process.argv[i];
  };
  switch (arg) {
    case '--version':
      version = next().replace(/^v/, '');
      break;
    case '--min-supported':
      minSupported = next().replace(/^v/, '');
      break;
    case '--dist':
      distDir = next();
      break;
    case '--output':
      output = next();
      break;
    case '--download-base-url':
      addDownloadBaseUrls(next());
      break;
    case '--download-base-urls':
      addDownloadBaseUrls(fs.readFileSync(next(), 'utf8'));
      break;
    case '--allow-partial':
      allowPartial = true;
      break;
    case '-h':
    case '--help':
      usage();
      process.exit(0);
      break;
    default:
      die(`unknown argument: ${arg}`);
  }
}

if (!version) {
  die('--version is required');
}
validateVersionSegment(version, 'version');
if (!minSupported) {
  minSupported = version;
}
validateVersionSegment(minSupported, 'min-supported');
if (!output) {
  output = path.join(distDir, 'manifest.json');
}

const targets = [
  ['darwin', 'amd64'],
  ['darwin', 'arm64'],
  ['linux', 'amd64'],
];

function expectedPackageName(os, arch) {
  return `awiki-deamon-${os}-${arch}.tar.gz`;
}

function discoverTargetsFromExistingPackages() {
  if (!fs.existsSync(distDir)) {
    die(`dist directory does not exist: ${distDir}`);
  }
  const supportedKeys = new Set(targets.map(([os, arch]) => `${os}/${arch}`));
  const discovered = new Set();
  for (const entry of fs.readdirSync(distDir)) {
    if (!entry.startsWith('awiki-deamon-') || !entry.endsWith('.tar.gz')) {
      continue;
    }
    const match = entry.match(/^awiki-deamon-([A-Za-z0-9_-]+)-([A-Za-z0-9_-]+)\.tar\.gz$/);
    if (!match) {
      die(`unsupported daemon package name: ${entry}`);
    }
    const key = `${match[1]}/${match[2]}`;
    if (!supportedKeys.has(key)) {
      die(`unsupported daemon package target: ${entry}`);
    }
    discovered.add(key);
  }
  const selected = targets.filter(([os, arch]) => discovered.has(`${os}/${arch}`));
  if (selected.length === 0) {
    die(`no daemon packages found in ${distDir}`);
  }
  return selected;
}

const selectedTargets = allowPartial ? discoverTargetsFromExistingPackages() : targets;

const packages = selectedTargets.map(([os, arch]) => {
  const fileName = expectedPackageName(os, arch);
  const filePath = path.join(distDir, fileName);
  if (!fs.existsSync(filePath)) {
    die(`missing daemon package: ${filePath}`);
  }
  const bytes = fs.readFileSync(filePath);
  const sha256 = crypto.createHash('sha256').update(bytes).digest('hex');
  return {
    version,
    os,
    arch,
    path: `releases/${version}/${fileName}`,
    sha256,
  };
});

const manifest = {
  latest: version,
  min_supported: minSupported,
  download_base_urls: downloadBaseUrls,
  packages,
};

fs.mkdirSync(path.dirname(output), { recursive: true });
fs.writeFileSync(output, `${JSON.stringify(manifest, null, 2)}\n`);
console.log(`daemon manifest created: ${output}`);
