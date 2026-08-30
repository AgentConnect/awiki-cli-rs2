import assert from 'node:assert/strict'
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import { spawnSync } from 'node:child_process'
import { createServer } from 'node:http'
import test from 'node:test'

import { ImCoreNodeError, openImCoreNodeClient } from '../dist/index.js'
import { nativePlatformPackages, resolveNativeTarget } from '../dist/targets.js'
import { createIdentityProviderFixture } from './identity-provider-fixture.mjs'

function options(stateRoot) {
  return {
    stateRoot,
    serviceBaseUrl: 'https://example.test',
    didDomain: 'example.test',
    operationTimeoutMs: 1000,
    syncTimeoutMs: 100,
  }
}

async function startRecoveryService(t) {
  const requests = []
  const server = createServer((request, response) => {
    const chunks = []
    request.on('data', chunk => chunks.push(chunk))
    request.on('end', () => {
      const body = JSON.parse(Buffer.concat(chunks).toString('utf8'))
      requests.push({ path: request.url, body })
      let result
      if (request.url === '/user-service/v1/handle/rpc') {
        result = {
          jsonrpc: '2.0',
          id: body.id,
          result: { ok: true, retry_after_seconds: 60, retry_at: '2099-08-20T12:00:00Z' },
        }
      }
      else if (request.url === '/user-service/v1/auth/handle-recovery/v4/exchange') {
        result = {
          contract_version: 'awiki.handle-recovery.v1.contract.4.20260807',
          recovery_grant: 'local-shape-test-grant',
          purpose: 'awiki.identity.handle-recovery.v1',
          expires_at: '2099-08-20T12:05:00Z',
          current_binding: {
            account_user_id: 'local-shape-test-user',
            full_handle: 'alice.awiki.test',
            current_did: 'did:wba:awiki.test:users:alice-existing',
            binding_generation: '7',
          },
        }
      }
      else {
        response.writeHead(404).end()
        return
      }
      response.writeHead(200, { 'content-type': 'application/json' })
      response.end(JSON.stringify(result))
    })
  })
  await new Promise((resolve, reject) => {
    server.once('error', reject)
    server.listen(0, '127.0.0.1', resolve)
  })
  t.after(() => new Promise((resolve, reject) => {
    server.close(error => error === undefined ? resolve() : reject(error))
  }))
  const address = server.address()
  assert.notEqual(address, null)
  assert.equal(typeof address, 'object')
  return { baseUrl: `http://127.0.0.1:${address.port}`, requests }
}

test('recovery progress exposes only the current stable impact fields through the real native binding', async t => {
  const root = await mkdtemp(join(tmpdir(), 'awiki-im-core-node-recovery-'))
  t.after(() => rm(root, { recursive: true, force: true }))
  const service = await startRecoveryService(t)
  const identityProvider = await createIdentityProviderFixture(root)
  t.after(() => identityProvider.dispose())
  const client = await openImCoreNodeClient({
    ...options(root),
    serviceBaseUrl: service.baseUrl,
    didDomain: 'awiki.test',
    multiDeviceHandleRecoveryEnabled: true,
    multiDeviceAudience: 'awiki-user-service',
    identityProvider,
  })
  t.after(() => client.close())

  const challenge = await client.requestHandleRecoveryOtp({
    fullHandle: 'alice.awiki.test',
    phone: '+8613800000000',
  })
  const progress = await client.prepareHandleRecovery({
    operationId: challenge.operationId,
    phone: '+8613800000000',
    otp: '123456',
  })

  assert.equal(progress.phase, 'ready_to_commit')
  assert.deepEqual(progress.impact, {
    localOrdinaryDataWillMigrate: false,
    otherDevicesMustRejoin: true,
  })
  assert.deepEqual(JSON.parse(JSON.stringify(progress)), progress)
  assert.deepEqual(service.requests.map(request => request.path), [
    '/user-service/v1/handle/rpc',
    '/user-service/v1/auth/handle-recovery/v4/exchange',
  ])
})

test('opens an empty Rust state, closes idempotently, and rejects later work', async t => {
  const root = await mkdtemp(join(tmpdir(), 'awiki-im-core-node-'))
  t.after(() => rm(root, { recursive: true, force: true }))
  const client = await openImCoreNodeClient(options(root))
  assert.equal(await client.getDefaultIdentity(), null)
  await assert.rejects(
    client.prepareExternalHttpRequest({
      url: 'https://api.example.test/orders',
      method: 'POST',
      headers: [{ name: 'content-type', value: 'application/json' }],
      body: new Uint8Array(),
    }),
    error => error instanceof ImCoreNodeError && error.code === 'identity_required',
  )
  await assert.rejects(
    client.prepareExternalHttpRequest({
      url: 'https://api.example.test/orders',
      method: 'POST',
      headers: [],
      body: new Uint8Array(4 * 1024 * 1024 + 1),
    }),
    error => error instanceof ImCoreNodeError && error.code === 'invalid_input',
  )
  await assert.rejects(
    client.getLocalConversationTimeline({ conversationId: 'dm:did:example:bob' }),
    error => error instanceof ImCoreNodeError && error.code === 'identity_required',
  )
  await assert.rejects(
    client.completeRegistrationWithOutcome({
      handle: 'alice',
      phone: '+8613800000000',
      otp: 'not-a-code',
    }),
    error => error instanceof ImCoreNodeError
      && error.code === 'invalid_otp'
      && error.safeMessage === 'The registration OTP is invalid.',
  )
  await assert.rejects(
    client.beginPreparedRegistrationJoin({
      continuationId: 'regjoin_missing',
      operationId: 'registration-access-native-test',
      ttlSeconds: 600,
      userPresenceConfirmed: false,
    }),
    error => error instanceof ImCoreNodeError
      && !error.message.includes('regjoin_missing'),
  )
  await client.close()
  await client.close()
  await assert.rejects(
    client.getDefaultIdentity(),
    error => error instanceof ImCoreNodeError && error.code === 'client_closed' && error.message === error.safeMessage,
  )
  await assert.rejects(
    client.prepareExternalHttpRequest({
      url: 'https://api.example.test/orders',
      method: 'GET',
      headers: [],
    }),
    error => error instanceof ImCoreNodeError && error.code === 'client_closed',
  )
  await assert.rejects(
    client.getLocalConversationTimeline({ conversationId: 'dm:did:example:bob' }),
    error => error instanceof ImCoreNodeError && error.code === 'client_closed',
  )
})

test('realtime facade requires an identity and returns only the stable redacted error', async t => {
  const root = await mkdtemp(join(tmpdir(), 'awiki-im-core-node-realtime-'))
  t.after(() => rm(root, { recursive: true, force: true }))
  const client = await openImCoreNodeClient(options(root))
  t.after(() => client.close())
  await assert.rejects(
    client.startRealtime(),
    error => error instanceof ImCoreNodeError
      && error.code === 'identity_required'
      && error.safeMessage === 'A registered IM identity is required.'
      && !error.message.includes('websocket')
      && !error.message.includes('http'),
  )
})

test('loads native v10 candidate Join and device-management methods', async t => {
  const root = await mkdtemp(join(tmpdir(), 'awiki-im-core-node-device-v10-'))
  t.after(() => rm(root, { recursive: true, force: true }))
  const client = await openImCoreNodeClient(options(root))
  t.after(() => client.close())

  assert.deepEqual(await client.listLocalDeviceJoinSessions(), [])
  await assert.rejects(
    client.getCurrentDeviceSummary(),
    error => error instanceof ImCoreNodeError
      && error.code === 'identity_required'
      && !error.message.includes(root),
  )
})

test('mail facade shares the identity gate and exposes only stable redacted errors', async t => {
  const root = await mkdtemp(join(tmpdir(), 'awiki-im-core-node-mail-'))
  t.after(() => rm(root, { recursive: true, force: true }))
  const client = await openImCoreNodeClient({
    ...options(root),
    mailServiceEndpoint: 'https://mail.example.test',
  })
  t.after(() => client.close())
  const operations = [
    () => client.getMailAccount(),
    () => client.listMailInbox(),
    () => client.readMail('mail-1'),
    () => client.markMailRead({ messageIds: ['mail-1'] }),
    () => client.sendMail({
      to: ['recipient@example.test'],
      subject: 'Subject',
      bodyText: 'Body',
    }),
  ]
  for (const operation of operations) {
    await assert.rejects(
      operation(),
      error => error instanceof ImCoreNodeError
        && error.code === 'identity_required'
        && error.safeMessage === 'A registered IM identity is required.'
        && !error.message.includes('mail.example.test'),
    )
  }
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

test('routes group, profile, recovery attestation, and payload operations through native v10 with structured identity errors', async t => {
  const root = await mkdtemp(join(tmpdir(), 'awiki-im-core-node-groups-'))
  t.after(() => rm(root, { recursive: true, force: true }))
  const client = await openImCoreNodeClient(options(root))
  t.after(() => client.close())

  await assert.rejects(
    client.createGroup({ name: 'Release Crew' }),
    error => error instanceof ImCoreNodeError
      && error.code === 'identity_required'
      && error.message === error.safeMessage,
  )
  const identityOperations = [
    () => client.getProfile(),
    () => client.getGroup({ groupDid: 'did:wba:example.test:group:release-crew' }),
    () => client.listGroups(),
    () => client.joinGroup({ groupDid: 'did:wba:example.test:group:release-crew' }),
    () => client.leaveGroup({ groupDid: 'did:wba:example.test:group:release-crew' }),
    () => client.listGroupMembers({ groupDid: 'did:wba:example.test:group:release-crew' }),
    () => client.removeGroupMember({
      groupDid: 'did:wba:example.test:group:release-crew',
      member: 'alice.example.test',
    }),
    () => client.sendPayload({
      conversationId: 'group:did:wba:example.test:group:release-crew',
      payloadJson: JSON.stringify({ value: true }),
    }),
  ]
  for (const operation of identityOperations) {
    await assert.rejects(
      operation(),
      error => error instanceof ImCoreNodeError
        && error.code === 'identity_required'
        && error.message === error.safeMessage,
    )
  }
  await assert.rejects(
    client.addGroupMember({
      groupDid: 'did:wba:example.test:group:release-crew',
      member: 'alice.example.test',
    }),
    error => error instanceof ImCoreNodeError
      && error.code === 'identity_required'
      && error.message === error.safeMessage,
  )
  await assert.rejects(
    client.getLocalConversationTimeline({ conversationId: 'group:did:wba:example.test:group:release-crew' }),
    error => error instanceof ImCoreNodeError
      && error.code === 'identity_required'
      && error.message === error.safeMessage,
  )
  await assert.rejects(
    client.hydrateDisplayProfiles({ peers: ['did:wba:example.test:user:alice'] }),
    error => error instanceof ImCoreNodeError
      && error.code === 'identity_required'
      && error.message === error.safeMessage,
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
