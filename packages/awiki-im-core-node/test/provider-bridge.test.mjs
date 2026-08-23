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
    capabilities: [
      'IDENTITY_READ',
      'IDENTITY_CREATE',
      'IDENTITY_SIGN',
      'IDENTITY_ECDH_SEALED',
      'IDENTITY_DOCUMENT_UPDATE',
      'IDENTITY_HTTP_SIGNATURE',
    ],
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
    hostStatus: async () => {
      throw Object.assign(new Error('not found'), { code: 'identity_not_found' })
    },
    create: async () => {
      throw Object.assign(new Error('not available'), { code: 'capability_unavailable' })
    },
    delete: async () => {},
    recoverIdentity: async () => {},
    ecdhSealed: async () => {
      throw Object.assign(new Error('not found'), { code: 'identity_not_found' })
    },
    sign: async () => {
      throw Object.assign(new Error('not found'), { code: 'identity_not_found' })
    },
    signOriginProof: async () => {
      throw Object.assign(new Error('not found'), { code: 'identity_not_found' })
    },
    prepareHttpSignature: async () => {
      throw Object.assign(new Error('not found'), { code: 'identity_not_found' })
    },
    prepareDocumentChange: async () => {
      throw Object.assign(new Error('not available'), { code: 'capability_unavailable' })
    },
    resumeDocumentChange: async () => undefined,
    adoptVerifiedDocument: async () => 'unchanged',
    beginDeviceEnrollment: async () => {
      throw Object.assign(new Error('not available'), { code: 'capability_unavailable' })
    },
    beginRequestSigningEnrollment: async () => {
      throw Object.assign(new Error('not available'), { code: 'capability_unavailable' })
    },
    resumeEnrollment: async () => undefined,
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

test('External Provider keeps document workflow handles inside the bridge', async () => {
  const calls = []
  const session = {
    candidate: async () => ({
      operationId: 'operation-1',
      candidateDocument: { id: 'did:wba:example.test:alice' },
      candidateDigest: 'sha256:candidate',
    }),
    hostPhase: async () => 'prepared',
    beginPublication: async () => {
      calls.push('begin')
      return {
        operationId: 'operation-1',
        candidateDigest: 'sha256:candidate',
        publicationGeneration: 2,
      }
    },
    complete: async (attempt, result) => {
      calls.push({ attempt, result })
      return { outcome: 'aborted' }
    },
    reconcile: async observation => {
      calls.push({ observation })
      return { outcome: 'ready_for_publication' }
    },
  }
  const dispatch = createIdentityProviderDispatch(provider({
    prepareDocumentChange: async () => session,
  }))
  const identity = { storeId: 'store-1', identityId: 'identity-1', did: 'did:wba:example.test:alice' }
  const prepared = await dispatch([{
    operation: 'prepareDocumentChange',
    payloadJson: JSON.stringify({
      identity,
      request: { changes: [{ change: 'replace_services', services: [] }] },
    }),
    buffers: [],
  }])
  assert.equal(prepared.ok, true)
  const { sessionId, candidate } = JSON.parse(prepared.payloadJson)
  assert.equal(candidate.operationId, 'operation-1')

  const phase = await dispatch([{
    operation: 'documentChangeHostPhase',
    payloadJson: JSON.stringify({ sessionId }),
    buffers: [],
  }])
  assert.equal(JSON.parse(phase.payloadJson), 'prepared')

  const begun = await dispatch([{
    operation: 'documentChangeBeginPublication',
    payloadJson: JSON.stringify({ sessionId }),
    buffers: [],
  }])
  const attempt = JSON.parse(begun.payloadJson)
  const completed = await dispatch([{
    operation: 'documentChangeComplete',
    payloadJson: JSON.stringify({
      sessionId,
      attempt,
      result: { result: 'rejected_before_acceptance' },
    }),
    buffers: [],
  }])
  assert.deepEqual(JSON.parse(completed.payloadJson), { outcome: 'aborted' })
  assert.equal(calls.length, 2)

  const stale = await dispatch([{
    operation: 'documentChangeBeginPublication',
    payloadJson: JSON.stringify({ sessionId }),
    buffers: [],
  }])
  assert.equal(stale.ok, false)
  assert.equal(stale.errorCode, 'invalid_request')
})

test('External Provider keeps enrollment handles and private operations inside the bridge', async () => {
  const calls = []
  const session = {
    proposal: async () => ({
      enrollmentId: 'enrollment-1',
      identity: {
        storeId: 'store-1',
        identityId: 'identity-1',
        did: 'did:wba:example.test:alice',
      },
      kind: {
        kind: 'device',
        deviceId: 'device-1',
        signingKey: { kid: 'signing', publicKeyMultibase: 'zSigning' },
        agreementKey: { kid: 'agreement', publicKeyMultibase: 'zAgreement' },
        profiles: [],
      },
      rootKeyFingerprint: 'sha256:root',
      checkpoint: { documentVersion: 1, registryVersion: 1, documentDigest: 'sha256:doc' },
    }),
    signDeviceAssertion: async payload => {
      calls.push(Buffer.from(payload))
      return Buffer.from('signed')
    },
    deriveDeviceSharedSecretSealed: async request => {
      calls.push(request)
      return sealedDelivery()
    },
    activate: async remote => {
      calls.push(remote)
      return 'activated'
    },
    cancel: async () => calls.push('cancel'),
  }
  const dispatch = createIdentityProviderDispatch(provider({
    beginDeviceEnrollment: async () => session,
  }))
  const begun = await dispatch([{
    operation: 'beginDeviceEnrollment',
    payloadJson: JSON.stringify({ remote: { document: { id: 'did:wba:example.test:alice' } } }),
    buffers: [],
  }])
  const { sessionId, proposal } = JSON.parse(begun.payloadJson)
  assert.equal(proposal.enrollmentId, 'enrollment-1')

  const signed = await dispatch([{
    operation: 'enrollmentSignDeviceAssertion',
    payloadJson: JSON.stringify({ sessionId }),
    buffers: [Buffer.from('payload')],
  }])
  assert.deepEqual(signed.buffers, [Buffer.from('signed')])

  const enrollmentEcdh = await dispatch([{
    operation: 'enrollmentEcdhSealed',
    payloadJson: JSON.stringify({ sessionId, requestId: 'enrollment-ecdh-1' }),
    buffers: [Buffer.alloc(32, 1), Buffer.alloc(32, 2)],
  }])
  assert.equal(enrollmentEcdh.ok, true)
  assert.deepEqual(JSON.parse(enrollmentEcdh.payloadJson), sealedDelivery())

  const activated = await dispatch([{
    operation: 'enrollmentActivate',
    payloadJson: JSON.stringify({ sessionId, remote: { document: { id: proposal.identity.did } } }),
    buffers: [],
  }])
  assert.equal(JSON.parse(activated.payloadJson), 'activated')
  assert.equal(calls.length, 3)

  const stale = await dispatch([{
    operation: 'enrollmentCancel',
    payloadJson: JSON.stringify({ sessionId }),
    buffers: [],
  }])
  assert.equal(stale.ok, false)
  assert.equal(stale.errorCode, 'invalid_request')
})

test('External Provider gates root promotion on the dedicated capability', async () => {
  const identity = { storeId: 'store-1', identityId: 'identity-1', did: 'did:wba:example.test:alice' }
  const request = {
    remote: {
      document: { id: identity.did },
      evidence: { documentVersion: 2, registryVersion: 3, documentDigest: 'sha256:document' },
    },
  }
  const unavailable = createIdentityProviderDispatch(provider())
  const rejected = await unavailable([{
    operation: 'confirmRootPromotion',
    payloadJson: JSON.stringify({ identity, request }),
    buffers: [],
  }])
  assert.equal(rejected.ok, false)
  assert.equal(rejected.errorCode, 'capability_unavailable')

  const calls = []
  const enabled = provider({
    capabilities: [
      ...provider().capabilities,
      'AWIKI_LEGACY_ROOT_TRANSFER_V1',
    ],
    confirmRootPromotion: async (reference, value) => calls.push({ reference, value }),
    signPendingRootObjectProof: async (reference, value) => {
      calls.push({ reference, value })
      return { ...value.document, proof: { verificationMethod: value.kid } }
    },
    exportRootKeySealed: async value => {
      calls.push(value)
      return sealedDelivery('AWIKI_LEGACY_ROOT_TRANSFER_V1')
    },
  })
  const accepted = await createIdentityProviderDispatch(enabled)([{
    operation: 'confirmRootPromotion',
    payloadJson: JSON.stringify({ identity, request }),
    buffers: [],
  }])
  assert.equal(accepted.ok, true)
  assert.deepEqual(calls, [{ reference: identity, value: request }])

  const proofRequest = {
    kid: `${identity.did}#root`,
    document: { type: 'root-possession' },
    issuerDid: identity.did,
    created: '2026-08-23T00:00:00Z',
  }
  const proof = await createIdentityProviderDispatch(enabled)([{
    operation: 'signPendingRootObjectProof',
    payloadJson: JSON.stringify({ identity, request: proofRequest }),
    buffers: [],
  }])
  assert.equal(proof.ok, true)
  assert.deepEqual(JSON.parse(proof.payloadJson), {
    ...proofRequest.document,
    proof: { verificationMethod: proofRequest.kid },
  })
  assert.deepEqual(calls[1], { reference: identity, value: proofRequest })

  const recipientPublicKey = Buffer.alloc(32, 9)
  const exported = await createIdentityProviderDispatch(enabled)([{
    operation: 'exportRootKeySealed',
    payloadJson: JSON.stringify({
      identity,
      kid: `${identity.did}#root`,
      requestId: 'root-export-1',
      userPresenceConfirmed: true,
    }),
    buffers: [recipientPublicKey],
  }])
  assert.equal(exported.ok, true)
  assert.deepEqual(
    JSON.parse(exported.payloadJson),
    sealedDelivery('AWIKI_LEGACY_ROOT_TRANSFER_V1'),
  )
  assert.equal(calls[2].recipientPublicKey, recipientPublicKey)
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
    ecdhSealed: async request => {
      calls.push({ operation: 'ecdh', request })
      return sealedDelivery()
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
  const peerPublic = Buffer.alloc(32, 3)
  const recipientPublicKey = Buffer.alloc(32, 4)
  const ecdh = await dispatch([{
    operation: 'ecdhSealed',
    payloadJson: JSON.stringify({
      identity,
      kid: `${identity.did}#agreement`,
      requestId: 'ecdh-1',
    }),
    buffers: [peerPublic, recipientPublicKey],
  }])
  assert.equal(ecdh.ok, true)
  assert.deepEqual(JSON.parse(ecdh.payloadJson), sealedDelivery())
  assert.equal(calls.length, 4)
  assert.equal(calls[0].request.payload, signInput)
  assert.equal(calls[2].request.body, body)
  assert.equal(calls[3].request.peerPublic, peerPublic)
  assert.equal(calls[3].request.recipientPublicKey, recipientPublicKey)
})

function sealedDelivery(capability = 'IDENTITY_ECDH_SEALED') {
  return {
    envelope: {
      protocol: 'anp-sealed-secret/1',
      suite: 'hpke-base-x25519-hkdf-sha256-chacha20poly1305-v1',
      encappedKey: 'encapped',
      ciphertext: 'ciphertext',
    },
    authorization: {
      providerInstanceId: 'provider-1',
      parentLeaseId: 'lease-1',
      consumer: 'dsh-awiki',
      capability,
      storeId: 'store-1',
      expiresAt: 2_000_000_000,
    },
    aad: 'aad',
  }
}

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
