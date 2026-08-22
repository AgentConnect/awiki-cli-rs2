import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { createRequire } from 'node:module'
import { fileURLToPath } from 'node:url'

const require = createRequire(import.meta.url)
const spikeRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const nativeSource = path.join(spikeRoot, 'target/release/libidentity_provider_bridge_spike.so')
const nativeAddon = path.join(spikeRoot, 'target/release/identity_provider_bridge_spike.node')
fs.copyFileSync(nativeSource, nativeAddon)

const { IdentityProviderBridge } = require(nativeAddon)
const { DidStore } = require('../../../../anp/anp-identity/bindings/node')

type Identity = {
  sign(kid: string, message: Buffer): Promise<Buffer>
  publicKeyBytes(kid: string): Promise<Buffer>
  snapshot(): Promise<{ did: string }>
}

type ProofRequest = {
  method: string
  metaJson: string
  bodyJson: string
  publicKey: Buffer
  keyId: string
}

async function fixture(t: { after(callback: () => void): void }): Promise<{
  identity: Identity
  request: ProofRequest
}> {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'identity-provider-bridge-'))
  t.after(() => fs.rmSync(root, { recursive: true, force: true }))
  const store = await DidStore.initializeInjected(root, 'bridge-spike', Buffer.alloc(32, 0x31))
  const identity = await store.createIdentity(identitySpec())
  const { did } = await identity.snapshot()
  return {
    identity,
    request: {
      method: 'message.send',
      metaJson: JSON.stringify({
        sender_did: did,
        timestamp: 1_787_403_600,
        target: { kind: 'agent', did: 'did:wba:example.com:agents:recipient' },
        operation_id: 'bridge-operation',
        message_id: 'bridge-message',
        content_type: 'application/json',
      }),
      bodyJson: JSON.stringify({ message: 'bridge spike' }),
      publicKey: await identity.publicKeyBytes('#request'),
      keyId: `${did}#request`,
    },
  }
}

test('real Origin Proof crosses Rust to TypeScript and back once', async (t) => {
  const { identity, request } = await fixture(t)
  const bridge = new IdentityProviderBridge()
  let calls = 0
  const proofJson = await bridge.signOriginProof(
    request,
    async (error: Error | null, signingInput: Buffer) => {
      assert.equal(error, null)
      calls += 1
      return identity.sign('#request', signingInput)
    },
    1_000,
  )
  const proof = JSON.parse(proofJson)
  assert.equal(calls, 1)
  assert.match(proof.signature, /^sig1=:/)
  assert.match(proof.signatureInput, /keyid=/)
})

test('lease revoke fails before invoking TypeScript', async (t) => {
  const { identity, request } = await fixture(t)
  const bridge = new IdentityProviderBridge()
  bridge.revokeLease()
  let calls = 0
  await assert.rejects(
    bridge.signOriginProof(
      request,
      async (_error: Error | null, signingInput: Buffer) => {
        calls += 1
        return identity.sign('#request', signingInput)
      },
      1_000,
    ),
    /lease_revoked/,
  )
  assert.equal(calls, 0)
})

test('in-flight cancellation rejects a late provider result', async (t) => {
  const { identity, request } = await fixture(t)
  const bridge = new IdentityProviderBridge()
  let started!: () => void
  const callbackStarted = new Promise<void>((resolve) => {
    started = resolve
  })
  const pending = bridge.signOriginProof(
    request,
    async (_error: Error | null, signingInput: Buffer) => {
      started()
      await delay(30)
      return identity.sign('#request', signingInput)
    },
    1_000,
  )
  await callbackStarted
  bridge.cancelInFlight()
  await assert.rejects(pending, /request_cancelled/)
})

test('timeout, provider rejection, and Host shutdown are bounded', async (t) => {
  const { identity, request } = await fixture(t)
  const timedOut = new IdentityProviderBridge()
  await assert.rejects(
    timedOut.signOriginProof(
      request,
      async (_error: Error | null, signingInput: Buffer) => {
        await delay(80)
        return identity.sign('#request', signingInput)
      },
      10,
    ),
    /provider_timeout/,
  )

  const rejected = new IdentityProviderBridge()
  await assert.rejects(
    rejected.signOriginProof(
      request,
      async () => {
        throw new Error('provider stopped')
      },
      1_000,
    ),
    /provider_error/,
  )

  const shuttingDown = new IdentityProviderBridge()
  let started!: () => void
  const callbackStarted = new Promise<void>((resolve) => {
    started = resolve
  })
  const pending = shuttingDown.signOriginProof(
    request,
    async (_error: Error | null, signingInput: Buffer) => {
      started()
      await delay(30)
      return identity.sign('#request', signingInput)
    },
    1_000,
  )
  await callbackStarted
  shuttingDown.shutdown()
  await assert.rejects(pending, /host_shutdown/)
})

function delay(milliseconds: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, milliseconds))
}

function identitySpec() {
  return {
    profile: 'e1',
    domain: 'example.com',
    pathSegments: ['agents', 'bridge-spike'],
    capabilities: { didWba: true },
    managedKeys: [
      { fragment: 'root', role: 'root_control' },
      { fragment: 'request', role: 'request_signing' },
    ],
    externalKeys: [],
    services: [],
    extensions: [],
  }
}
