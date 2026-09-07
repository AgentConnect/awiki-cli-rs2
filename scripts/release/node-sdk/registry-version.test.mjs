import assert from 'node:assert/strict'
import test from 'node:test'
import { exactDependencyVersion } from './registry-version.mjs'

test('Cargo exact requirements become plain artifact versions', () => {
  assert.equal(exactDependencyVersion('=1.0.1'), '1.0.1')
  assert.equal(exactDependencyVersion('1.0.1'), '1.0.1')
})

test('floating or malformed requirements cannot become provenance versions', () => {
  for (const value of ['^1.0.1', '>=1.0.1', '*', '1.0', 'latest']) {
    assert.throws(() => exactDependencyVersion(value), /exact stable SDK version/)
  }
})
