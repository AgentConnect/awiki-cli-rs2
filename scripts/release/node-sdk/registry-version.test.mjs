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

test('only explicit source candidates can record an exact prerelease', () => {
  assert.throws(() => exactDependencyVersion('=1.0.4-rc.1'), /exact stable SDK version/)
  assert.equal(exactDependencyVersion('=1.0.4-rc.1', { allowPrerelease: true }), '1.0.4-rc.1')
  for (const value of ['^1.0.4-rc.1', '1.0.4-rc', '1.0.4-dev.1']) {
    assert.throws(() => exactDependencyVersion(value, { allowPrerelease: true }), /exact stable SDK version/)
  }
})
