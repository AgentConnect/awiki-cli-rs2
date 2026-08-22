import assert from 'node:assert/strict'
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'

import { ImCoreNodeError, openImCoreNodeClient } from '../dist/index.js'
import { createIdentityProviderDispatch } from '../dist/provider-bridge.js'

function options(stateRoot, identityProvider) {
  return {
    stateRoot,
    serviceBaseUrl: 'https://example.test',
    didDomain: 'example.test',
    operationTimeoutMs: 1000,
    syncTimeoutMs: 100,
    identityProvider,
  }
}

function provider(overrides = {}) {
  return {
    protocol: 'anp-identity-provider-ts/1',
    capabilities: ['IDENTITY_READ', 'IDENTITY_SIGN', 'IDENTITY_HTTP_SIGNATURE'],
    info: async () => ({
      storeId: 'store_external_test',
      schemaCompatible: true,
      identityCount: 0,
      health: 'ready',
    }),
    recover: async () => ({ identityCount: 0 }),
    list: async () => [],
    publicIdentity: async () => {
      throw Object.assign(new Error('not found'), { code: 'identity_not_found' })
    },
    recoverIdentity: async () => {},
    sign: async () => {
      throw Object.assign(new Error('not found'), { code: 'identity_not_found' })
    },
    signOriginProof: async () => {
      throw Object.assign(new Error('not found'), { code: 'identity_not_found' })
    },
    prepareHttpSignature: async () => {
      throw Object.assign(new Error('not found'), { code: 'identity_not_found' })
    },
    ...overrides,
  }
}

test('External Provider open performs one versioned readiness handshake', async t => {
  const root = await mkdtemp(join(tmpdir(), 'awiki-im-core-node-provider-'))
  t.after(() => rm(root, { recursive: true, force: true }))
  const calls = []
  const identityProvider = provider({
    info: async () => {
      calls.push('info')
      return {
        storeId: 'store_external_test',
        schemaCompatible: true,
        identityCount: 0,
        health: 'ready',
      }
    },
  })

  const client = await openImCoreNodeClient(options(root, identityProvider))
  t.after(() => client.close())

  assert.equal(await client.getDefaultIdentity(), null)
  assert.deepEqual(calls, ['info'])
})

test('External Provider protocol and capabilities fail closed before Core opens', async t => {
  for (const identityProvider of [
    provider({ protocol: 'anp-identity-provider-ts/0' }),
    provider({ capabilities: ['IDENTITY_READ', 'IDENTITY_SIGN'] }),
  ]) {
    const root = await mkdtemp(join(tmpdir(), 'awiki-im-core-node-provider-invalid-'))
    t.after(() => rm(root, { recursive: true, force: true }))
    await assert.rejects(
      openImCoreNodeClient(options(root, identityProvider)),
      error => error instanceof ImCoreNodeError
        && error.code === 'provider_incompatible'
        && error.retryable === false,
    )
  }
})

test('External Provider rejection crosses the Promise bridge as a redacted stable error', async t => {
  const root = await mkdtemp(join(tmpdir(), 'awiki-im-core-node-provider-error-'))
  t.after(() => rm(root, { recursive: true, force: true }))
  const identityProvider = provider({
    info: async () => {
      throw Object.assign(new Error('sensitive provider detail'), {
        code: 'provider_disposed',
        retryable: false,
      })
    },
  })

  await assert.rejects(
    openImCoreNodeClient(options(root, identityProvider)),
    error => error instanceof ImCoreNodeError
      && error.code === 'provider_disposed'
      && error.safeMessage === 'The identity provider lease has been disposed.'
      && !error.message.includes('sensitive provider detail'),
  )
})

test('External Provider handshake is bounded by the Core operation timeout', async t => {
  const root = await mkdtemp(join(tmpdir(), 'awiki-im-core-node-provider-timeout-'))
  t.after(() => rm(root, { recursive: true, force: true }))
  const identityProvider = provider({ info: () => new Promise(() => {}) })

  await assert.rejects(
    openImCoreNodeClient({ ...options(root, identityProvider), operationTimeoutMs: 20 }),
    error => error instanceof ImCoreNodeError && error.code === 'timeout' && error.retryable === true,
  )
})

test('External Provider hot paths make one call and keep binary values out of JSON', async () => {
  const calls = []
  const identityProvider = provider({
    sign: async (identity, request) => {
      calls.push({ operation: 'sign', identity, request })
      return { kid: `${identity.did}#device`, algorithm: 'ed25519', bytes: Buffer.from('signature') }
    },
    signOriginProof: async (identity, request) => {
      calls.push({ operation: 'origin', identity, request })
      return { contentDigest: 'sha-256=:digest:', signatureInput: 'sig1=()', signature: 'sig1=:value:' }
    },
    prepareHttpSignature: async request => {
      calls.push({ operation: 'http', request })
      return {
        bindingDigest: 'sha256:binding',
        kid: `${request.identity.did}#device`,
        headerPatch: [{ name: 'signature', value: 'sig1=:value:' }],
      }
    },
  })
  const dispatch = createIdentityProviderDispatch(identityProvider)
  const identity = { storeId: 'store-1', identityId: 'identity-1', did: 'did:wba:example.test:alice' }

  const signInput = Buffer.from([0, 1, 2, 255])
  const sign = await dispatch([{
    operation: 'sign',
    payloadJson: JSON.stringify({ identity, purpose: 'device_assertion', kid: `${identity.did}#device` }),
    buffers: [signInput],
  }])
  assert.equal(sign.ok, true)
  assert.deepEqual(JSON.parse(sign.payloadJson), { kid: `${identity.did}#device`, algorithm: 'ed25519' })
  assert.deepEqual(sign.buffers, [Buffer.from('signature')])
  assert.equal(sign.payloadJson.includes(signInput.toString('base64')), false)

  const origin = await dispatch([{
    operation: 'signOriginProof',
    payloadJson: JSON.stringify({
      identity,
      request: { method: 'message.send', meta: { trace: 'one' }, body: { text: 'hello' } },
    }),
    buffers: [],
  }])
  assert.equal(origin.ok, true)
  assert.equal(origin.buffers.length, 0)

  const body = Buffer.from('request-body')
  const http = await dispatch([{
    operation: 'prepareHttpSignature',
    payloadJson: JSON.stringify({
      identity,
      url: 'https://example.test/rpc',
      method: 'POST',
      headers: [{ name: 'content-type', value: 'application/json' }],
      hasBody: true,
    }),
    buffers: [body],
  }])
  assert.equal(http.ok, true)
  assert.equal(http.buffers.length, 0)
  assert.equal(calls.length, 3)
  assert.equal(calls[0].request.payload, signInput)
  assert.equal(calls[2].request.body, body)
})

test('External Provider bridge rejects malformed binary arity without calling the signer', async () => {
  let signCalls = 0
  const dispatch = createIdentityProviderDispatch(provider({
    sign: async () => {
      signCalls += 1
      throw new Error('must not be reached')
    },
  }))
  const reply = await dispatch([{
    operation: 'sign',
    payloadJson: JSON.stringify({
      identity: { storeId: 's', identityId: 'i', did: 'did:wba:example.test:a' },
      purpose: 'authentication',
    }),
    buffers: [],
  }])
  assert.equal(reply.ok, false)
  assert.equal(reply.errorCode, 'invalid_request')
  assert.equal(signCalls, 0)
})
