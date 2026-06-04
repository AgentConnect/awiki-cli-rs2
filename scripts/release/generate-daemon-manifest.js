#!/usr/bin/env node
const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

function usage() {
  console.error(`Usage:
  node scripts/release/generate-daemon-manifest.js --version VERSION [--min-supported VERSION] [--dist DIR] [--base-url URL] [--output FILE]`);
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
let baseUrl = 'https://awiki.ai/daemon/releases';
let output = '';

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
    case '--base-url':
      baseUrl = next().replace(/\/+$/, '');
      break;
    case '--output':
      output = next();
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
  ['linux', 'arm64'],
];

const packages = targets.map(([os, arch]) => {
  const fileName = `awiki-deamon-${os}-${arch}.tar.gz`;
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
    url: `${baseUrl}/${version}/${fileName}`,
    sha256,
  };
});

const manifest = {
  latest: version,
  min_supported: minSupported,
  packages,
};

fs.mkdirSync(path.dirname(output), { recursive: true });
fs.writeFileSync(output, `${JSON.stringify(manifest, null, 2)}\n`);
console.log(`daemon manifest created: ${output}`);
