'use strict';

const assert = require('node:assert/strict');
const test = require('node:test');
const { _internal } = require('./run.js');

test('enables real user-systemd service management for Linux npm installs', () => {
  assert.deepEqual(_internal.serviceManagerEnvironment('linux', {}), {
    AWIKI_CLI_ENABLE_SYSTEMD_LISTENER_SERVICE: '1',
    AWIKI_CLI_ENABLE_SYSTEMD_HERMES_BRIDGE_SERVICE: '1',
  });
});

test('preserves explicit Linux service-management overrides', () => {
  assert.deepEqual(
    _internal.serviceManagerEnvironment('linux', {
      AWIKI_CLI_ENABLE_SYSTEMD_LISTENER_SERVICE: '0',
      AWIKI_CLI_ENABLE_SYSTEMD_HERMES_BRIDGE_SERVICE: 'false',
    }),
    {
      AWIKI_CLI_ENABLE_SYSTEMD_LISTENER_SERVICE: '0',
      AWIKI_CLI_ENABLE_SYSTEMD_HERMES_BRIDGE_SERVICE: 'false',
    },
  );
});

test('does not enable Linux service backends on other platforms', () => {
  assert.deepEqual(_internal.serviceManagerEnvironment('darwin', {}), {});
  assert.deepEqual(_internal.serviceManagerEnvironment('win32', {}), {});
});
