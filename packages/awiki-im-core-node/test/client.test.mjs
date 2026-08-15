import assert from 'node:assert/strict'
import { mkdtemp, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import { spawnSync } from 'node:child_process'
import test from 'node:test'

import { ImCoreNodeError, openImCoreNodeClient } from '../dist/index.js'

function options(stateRoot) {
  return {
    stateRoot,
    serviceBaseUrl: 'https://example.test',
    didDomain: 'example.test',
    operationTimeoutMs: 1000,
    syncTimeoutMs: 100,
  }
}

test('opens an empty Rust state, closes idempotently, and rejects later work', async t => {
  const root = await mkdtemp(join(tmpdir(), 'awiki-im-core-node-'))
  t.after(() => rm(root, { recursive: true, force: true }))
  const client = await openImCoreNodeClient(options(root))
  assert.equal(await client.getDefaultIdentity(), null)
  await client.close()
  await client.close()
  await assert.rejects(
    client.getDefaultIdentity(),
    error => error instanceof ImCoreNodeError && error.code === 'client_closed' && error.message === error.safeMessage,
  )
})

test('fails loudly when another process owns the same state root', async t => {
  const root = await mkdtemp(join(tmpdir(), 'awiki-im-core-node-lock-'))
  t.after(() => rm(root, { recursive: true, force: true }))
  const client = await openImCoreNodeClient(options(root))
  t.after(() => client.close())
  const moduleUrl = pathToFileURL(join(import.meta.dirname, '../dist/index.js')).href
  const child = spawnSync(process.execPath, ['--input-type=module', '-e', `
    import { openImCoreNodeClient } from ${JSON.stringify(moduleUrl)}
    try {
      await openImCoreNodeClient(${JSON.stringify(options(root))})
      console.log(JSON.stringify({ code: 'unexpected_success' }))
      process.exitCode = 2
    }
    catch (error) {
      console.log(JSON.stringify({ code: error.code, message: error.safeMessage }))
    }
  `], { encoding: 'utf8', timeout: 10000 })
  assert.equal(child.status, 0, child.stderr)
  assert.deepEqual(JSON.parse(child.stdout.trim()), {
    code: 'state_in_use',
    message: 'The IM state root is already open in another client.',
  })
})

test('a normal open and close lets Node exit without process.exit()', async t => {
  const root = await mkdtemp(join(tmpdir(), 'awiki-im-core-node-exit-'))
  t.after(() => rm(root, { recursive: true, force: true }))
  const moduleUrl = pathToFileURL(join(import.meta.dirname, '../dist/index.js')).href
  const child = spawnSync(process.execPath, ['--input-type=module', '-e', `
    import { openImCoreNodeClient } from ${JSON.stringify(moduleUrl)}
    const client = await openImCoreNodeClient(${JSON.stringify(options(root))})
    await client.close()
    console.log('closed')
  `], { encoding: 'utf8', timeout: 10000 })
  assert.equal(child.status, 0, child.stderr)
  assert.equal(child.stdout.trim(), 'closed')
})

test('does not import a legacy TypeScript SDK identity.json', async t => {
  const root = await mkdtemp(join(tmpdir(), 'awiki-im-core-node-legacy-'))
  t.after(() => rm(root, { recursive: true, force: true }))
  await writeFile(
    join(root, 'identity.json'),
    JSON.stringify({ did: 'did:example:must-not-import', token: 'legacy-secret' }),
    { mode: 0o600 },
  )
  const client = await openImCoreNodeClient(options(root))
  t.after(() => client.close())
  assert.equal(await client.getDefaultIdentity(), null)
})

test('returns a stable redacted error for an invalid state root', async () => {
  await assert.rejects(
    openImCoreNodeClient(options('relative/private/token-secret')),
    error => error instanceof ImCoreNodeError
      && error.code === 'invalid_state_root'
      && error.safeMessage === 'The IM state root must be an absolute path.'
      && error.message === error.safeMessage
      && !error.message.includes('token-secret'),
  )
})
