'use strict';

const assert = require('node:assert/strict');
const test = require('node:test');
const { _internal } = require('./run.js');

test('enables the opt-in Hermes user service for Linux npm installs', () => {
  assert.deepEqual(_internal.hermesServiceEnvironment('linux', {}), {
    AWIKI_CLI_ENABLE_SYSTEMD_HERMES_BRIDGE_SERVICE: '1',
  });
});

test('preserves an explicit Hermes user-service override', () => {
  assert.deepEqual(
    _internal.hermesServiceEnvironment('linux', {
      AWIKI_CLI_ENABLE_SYSTEMD_HERMES_BRIDGE_SERVICE: 'false',
    }),
    {
      AWIKI_CLI_ENABLE_SYSTEMD_HERMES_BRIDGE_SERVICE: 'false',
    },
  );
});

test('does not enable the Hermes user service on other platforms', () => {
  assert.deepEqual(_internal.hermesServiceEnvironment('darwin', {}), {});
  assert.deepEqual(_internal.hermesServiceEnvironment('win32', {}), {});
});

test('prints an exact package-local binary installer command on POSIX', () => {
  assert.equal(
    _internal.installBinaryCommand('/tmp/Awiki CLI', 'darwin'),
    "node '/tmp/Awiki CLI/scripts/install.js'",
  );
});

test('quotes the package-local binary installer command on Windows', () => {
  assert.equal(
    _internal.installBinaryCommand('C:\\Awiki CLI', 'win32'),
    'node "C:\\Awiki CLI\\scripts\\install.js"',
  );
});
