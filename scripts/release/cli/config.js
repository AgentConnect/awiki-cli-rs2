#!/usr/bin/env node
'use strict';

const fs = require('fs');
const path = require('path');

const SERVER_KEYS = new Set([
  'public_origin', 'public_base_path', 'default_backend_base_url', 'default_did_host',
  'web_root', 'archive_root', 'nginx_config', 'nginx_http_snippet', 'nginx_snippet',
  'nginx_backup_root', 'protocol_gateway_checkout', 'protocol_gateway_origin',
  'protocol_gateway_service', 'github_repo', 'github_workflow', 'github_token',
  'cli_download_max_per_ip', 'cli_download_max_total', 'cli_download_rate_after',
  'cli_download_rate',
]);
const RELEASE_KEYS = new Set([
  'schema_version', 'channels', 'anp_repository', 'anp_commit',
  'anp_identity_commit', 'archive_keep_versions', 'targets',
]);
const CHANNEL_KEYS = new Set(['version', 'min_supported_version']);
const SUPPORTED_TARGETS = [
  'darwin-amd64', 'darwin-arm64', 'linux-amd64', 'windows-amd64',
];

function rejectUnknownKeys(value, allowed, label) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error(`${label} must be an object`);
  }
  const unknown = Object.keys(value).filter(key => !allowed.has(key));
  if (unknown.length) throw new Error(`unknown ${label} keys: ${unknown.join(', ')}`);
}

function parseFlatToml(text) {
  const result = {};
  for (const [index, rawLine] of text.split(/\r?\n/).entries()) {
    const line = rawLine.trim();
    if (!line || line.startsWith('#')) continue;
    const match = line.match(/^([a-z][a-z0-9_]*)\s*=\s*"((?:[^"\\]|\\.)*)"\s*(?:#.*)?$/);
    if (!match) throw new Error(`unsupported TOML syntax on line ${index + 1}`);
    const [, key, rawValue] = match;
    if (Object.hasOwn(result, key)) throw new Error(`duplicate TOML key ${key}`);
    result[key] = JSON.parse(`"${rawValue}"`);
  }
  return result;
}

function readServerConfig(filePath) {
  const resolved = path.resolve(filePath);
  const config = parseFlatToml(fs.readFileSync(resolved, 'utf8'));
  const unknown = Object.keys(config).filter(key => !SERVER_KEYS.has(key));
  const missing = [...SERVER_KEYS].filter(key => !config[key]);
  if (unknown.length) throw new Error(`unknown publish-server keys: ${unknown.join(', ')}`);
  if (missing.length) throw new Error(`missing publish-server keys: ${missing.join(', ')}`);
  for (const [key, value] of Object.entries(config)) {
    if (/[\u0000-\u001f\u007f]/.test(value)) throw new Error(`${key} must not contain control characters`);
  }
  for (const key of ['public_origin', 'default_backend_base_url']) {
    const url = new URL(config[key]);
    if (url.protocol !== 'https:' || url.pathname !== '/' || url.search || url.hash) {
      throw new Error(`${key} must be an HTTPS origin without a path`);
    }
    config[key] = url.origin;
  }
  const gatewayOrigin = new URL(config.protocol_gateway_origin);
  if (!['http:', 'https:'].includes(gatewayOrigin.protocol)
      || gatewayOrigin.username || gatewayOrigin.password
      || gatewayOrigin.pathname !== '/' || gatewayOrigin.search || gatewayOrigin.hash) {
    throw new Error('protocol_gateway_origin must be an HTTP(S) origin without credentials or a path');
  }
  config.protocol_gateway_origin = gatewayOrigin.origin;
  if (!/^\/(?:[a-z0-9._-]+(?:\/[a-z0-9._-]+)*)$/i.test(config.public_base_path)
      || config.public_base_path.split('/').some(part => part === '.' || part === '..')) {
    throw new Error('public_base_path must be an absolute URL path without a trailing slash');
  }
  for (const key of [
    'web_root', 'archive_root', 'nginx_config', 'nginx_http_snippet', 'nginx_snippet',
    'nginx_backup_root', 'protocol_gateway_checkout',
  ]) {
    if (!path.isAbsolute(config[key])) throw new Error(`${key} must be an absolute filesystem path`);
    if (!/^\/[a-z0-9._/-]+$/i.test(config[key]) || config[key].split('/').includes('..')) {
      throw new Error(`${key} contains unsafe filesystem path characters`);
    }
  }
  if (!/^[a-z0-9.-]+$/i.test(config.default_did_host) || config.default_did_host.includes('..')) {
    throw new Error('default_did_host must be a bare DNS host');
  }
  if (!/^[a-z0-9_.@-]+$/i.test(config.protocol_gateway_service)) {
    throw new Error('protocol_gateway_service must be a systemd service name');
  }
  if (!/^[a-z0-9_.-]+\/[a-z0-9_.-]+$/i.test(config.github_repo)) {
    throw new Error('github_repo must use owner/repository syntax');
  }
  if (!/^[a-z0-9_.-]+\.ya?ml$/i.test(config.github_workflow)) {
    throw new Error('github_workflow must be a workflow YAML filename');
  }
  for (const key of ['cli_download_max_per_ip', 'cli_download_max_total']) {
    if (!/^[1-9]\d*$/.test(config[key])) throw new Error(`${key} must be a positive integer`);
    const value = Number(config[key]);
    if (!Number.isSafeInteger(value) || value > 10000) {
      throw new Error(`${key} must be an integer from 1 to 10000`);
    }
    config[key] = value;
  }
  if (config.cli_download_max_total < config.cli_download_max_per_ip) {
    throw new Error('cli_download_max_total must be greater than or equal to cli_download_max_per_ip');
  }
  for (const key of ['cli_download_rate_after', 'cli_download_rate']) {
    if (!/^[1-9]\d*(?:[kKmMgG])?$/.test(config[key])) {
      throw new Error(`${key} must be a positive Nginx size such as 512k or 1m`);
    }
    config[key] = config[key].toLowerCase();
  }
  return config;
}

function readAnpCandidateLock(filePath) {
  const lock = JSON.parse(fs.readFileSync(path.resolve(filePath), 'utf8'));
  rejectUnknownKeys(
    lock,
    new Set(['schemaVersion', 'candidateVersion', 'sourceDateEpoch', 'anp', 'identity']),
    'ANP candidate lock',
  );
  if (lock.schemaVersion !== 1 || lock.candidateVersion !== '1.0.0'
      || !Number.isInteger(lock.sourceDateEpoch) || lock.sourceDateEpoch < 1) {
    throw new Error('ANP candidate lock version is invalid');
  }
  rejectUnknownKeys(
    lock.anp,
    new Set(['repository', 'commit', 'rustTreeSha256', 'pythonWheel']),
    'ANP candidate SDK',
  );
  rejectUnknownKeys(
    lock.identity,
    new Set(['repository', 'commit', 'version', 'rustTreeSha256']),
    'ANP Identity candidate',
  );
  rejectUnknownKeys(lock.anp.pythonWheel, new Set(['filename', 'sha256']), 'ANP wheel');
  if (lock.anp.repository !== 'https://github.com/agent-network-protocol/anp.git'
      || !/^[a-f0-9]{40}$/.test(lock.anp.commit || '')
      || !/^[a-f0-9]{64}$/.test(lock.anp.rustTreeSha256 || '')
      || lock.anp.pythonWheel.filename !== 'anp-1.0.0-py3-none-any.whl'
      || !/^[a-f0-9]{64}$/.test(lock.anp.pythonWheel.sha256 || '')) {
    throw new Error('ANP candidate SDK provenance is invalid');
  }
  if (lock.identity.repository !== 'https://github.com/agent-network-protocol/anp-identity.git'
      || !/^[a-f0-9]{40}$/.test(lock.identity.commit || '')
      || lock.identity.version !== '0.2.0'
      || !/^[a-f0-9]{64}$/.test(lock.identity.rustTreeSha256 || '')) {
    throw new Error('ANP Identity candidate provenance is invalid');
  }
  return lock;
}

function readReleaseConfig(filePath, candidateLockPath = null) {
  const config = JSON.parse(fs.readFileSync(path.resolve(filePath), 'utf8'));
  rejectUnknownKeys(config, RELEASE_KEYS, 'release-config');
  if (config.schema_version !== 1 || !config.channels || !Array.isArray(config.targets)) {
    throw new Error('release-config.json must use schema_version=1 and define channels and targets');
  }
  rejectUnknownKeys(config.channels, new Set(['beta', 'stable']), 'release channels');
  if (Object.keys(config.channels).length !== 2) throw new Error('release channels must define beta and stable');
  for (const channel of ['beta', 'stable']) {
    const entry = config.channels[channel] || {};
    rejectUnknownKeys(entry, CHANNEL_KEYS, `${channel} channel`);
    if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(entry.version || '')) {
      throw new Error(`invalid ${channel} version`);
    }
    if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(entry.min_supported_version || '')) {
      throw new Error(`invalid ${channel} min_supported_version`);
    }
  }
  if (!config.channels.beta.version.includes('-')) throw new Error('beta version must be a prerelease');
  if (config.channels.stable.version.includes('-')) throw new Error('stable version must not be a prerelease');
  if (config.anp_repository !== 'agent-network-protocol/anp') throw new Error('anp_repository is invalid');
  if (!/^[a-f0-9]{40}$/i.test(config.anp_commit || '')) throw new Error('anp_commit must be a full commit SHA');
  if (!/^[a-f0-9]{40}$/i.test(config.anp_identity_commit || '')) throw new Error('anp_identity_commit must be a full commit SHA');
  if (!Number.isInteger(config.archive_keep_versions) || config.archive_keep_versions < 1 || config.archive_keep_versions > 100) {
    throw new Error('archive_keep_versions must be an integer from 1 to 100');
  }
  const uniqueTargets = [...new Set(config.targets)];
  if (uniqueTargets.length !== config.targets.length
      || uniqueTargets.length !== SUPPORTED_TARGETS.length
      || SUPPORTED_TARGETS.some(target => !uniqueTargets.includes(target))) {
    throw new Error(`targets must contain exactly: ${SUPPORTED_TARGETS.join(', ')}`);
  }
  if (candidateLockPath) {
    const candidate = readAnpCandidateLock(candidateLockPath);
    if (config.anp_commit !== candidate.anp.commit
        || config.anp_identity_commit !== candidate.identity.commit) {
      throw new Error('release config does not match ANP candidate lock');
    }
  }
  return config;
}

if (require.main === module) {
  const [kind, filePath, key] = process.argv.slice(2);
  const candidateLock = kind === 'release'
    ? path.resolve(path.dirname(path.resolve(filePath)), '../../../anp-release.lock.json')
    : null;
  const config = kind === 'server'
    ? readServerConfig(filePath)
    : kind === 'release'
      ? readReleaseConfig(filePath, candidateLock)
      : null;
  if (!config) throw new Error('usage: config.js server|release FILE [KEY]');
  if (!key) process.stdout.write(`${JSON.stringify(config)}\n`);
  else {
    const value = key.split('.').reduce((cursor, part) => cursor && cursor[part], config);
    if (value === undefined) throw new Error(`unknown config key ${key}`);
    process.stdout.write(typeof value === 'object' ? JSON.stringify(value) : String(value));
  }
}

module.exports = {
  parseFlatToml, readAnpCandidateLock, readServerConfig, readReleaseConfig,
};
