import assert from 'node:assert/strict'
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import { spawnSync } from 'node:child_process'
import test from 'node:test'

import { ImCoreNodeError, openImCoreNodeClient } from '../dist/index.js'
import { nativePlatformPackages, resolveNativeTarget } from '../dist/targets.js'

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

test('clears SDK-owned local data and keeps the client usable', async t => {
  const root = await mkdtemp(join(tmpdir(), 'awiki-im-core-node-clear-'))
  t.after(() => rm(root, { recursive: true, force: true }))
  const client = await openImCoreNodeClient(options(root))
  t.after(() => client.close())
  await writeFile(join(root, 'cache', 'owned.bin'), 'private', { mode: 0o600 })

  assert.deepEqual(await client.clearLocalData(), { cleared: true })
  await assert.rejects(readFile(join(root, 'cache', 'owned.bin')), { code: 'ENOENT' })
  assert.equal(await client.getDefaultIdentity(), null)
  assert.deepEqual(await client.clearLocalData(), { cleared: true })
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

test('resolves libc explicitly and has no musl or TypeScript fallback', () => {
  assert.equal(resolveNativeTarget('linux', 'x64', '2.34'), 'linux-x64-gnu')
  assert.equal(resolveNativeTarget('linux', 'arm64', undefined), 'linux-arm64-musl')
  assert.equal(resolveNativeTarget('darwin', 'arm64', undefined), 'darwin-arm64')
  assert.equal(nativePlatformPackages['linux-x64-gnu'], '@awiki/im-core-node-linux-x64-gnu')
  assert.equal(nativePlatformPackages['linux-arm64-musl'], undefined)
})

test('Tier 1 platform manifests match the root optional dependency contract', async () => {
  const packageRoot = join(import.meta.dirname, '..')
  const rootManifest = JSON.parse(await readFile(join(packageRoot, 'package.json'), 'utf8'))
  assert.deepEqual(
    Object.keys(rootManifest.optionalDependencies).sort(),
    Object.values(nativePlatformPackages).sort(),
  )
  const tier1 = {
    'linux-x64-gnu': ['linux-x64-gnu', '@awiki/im-core-node-linux-x64-gnu'],
    'linux-arm64-gnu': ['linux-arm64-gnu', '@awiki/im-core-node-linux-arm64-gnu'],
    'darwin-x64': ['darwin-x64', '@awiki/im-core-node-darwin-x64'],
    'darwin-arm64': ['darwin-arm64', '@awiki/im-core-node-darwin-arm64'],
    'win32-x64-msvc': ['win32-x64-msvc', '@awiki/im-core-node-win32-x64-msvc'],
  }
  for (const [directory, [target, packageName]] of Object.entries(tier1)) {
    const manifest = JSON.parse(await readFile(
      join(packageRoot, '../awiki-im-core-node-platforms', directory, 'package.json'),
      'utf8',
    ))
    assert.equal(manifest.name, packageName)
    assert.equal(manifest.version, rootManifest.version)
    assert.equal(rootManifest.optionalDependencies[packageName], `workspace:${rootManifest.version}`)
    assert.equal(manifest.license, 'AGPL-3.0-only')
    assert.equal(manifest.type, 'commonjs')
    assert.equal(manifest.main, `./awiki-im-core-node.${target}.node`)
    assert.equal(manifest.scripts, undefined)
  }
})
