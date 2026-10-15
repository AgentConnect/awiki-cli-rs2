import test from 'node:test'
import assert from 'node:assert/strict'
import { candidateDependencies } from './candidate-dependencies.mjs'
const versions = { anp: '1.0.6', 'anp-identity': '0.2.5' }
const packages = Object.entries(versions).map(([name, version]) => ({ name, version, source: 'registry+https://github.com/rust-lang/crates.io-index' }))
test('Core candidate preserves fixed registry dependency provenance', () => {
  const result = candidateDependencies({ packages }, versions, () => ({ commit: 'a'.repeat(40), dirty: false }))
  assert.equal(result[0].source, packages[0].source)
  assert.equal(result[1].version, '0.2.5')
})
test('candidate rejects changed versions and alternate registries', () => {
  for (const patch of [{ version: '1.0.7' }, { source: 'registry+https://untrusted.example' }]) {
    assert.throws(() => candidateDependencies({ packages: [{ ...packages[0], ...patch }, packages[1]] }, versions, () => ({})))
  }
})
