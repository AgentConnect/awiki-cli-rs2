import assert from 'node:assert/strict'
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import { openImCoreNodeClient } from '../dist/index.js'

function options(stateRoot) {
  return { stateRoot, serviceBaseUrl: 'https://example.test', didDomain: 'example.test' }
}

for (const field of ['multiDeviceDirectE2eeEnabled', 'multiDeviceGroupE2eeEnabled']) {
  test(`${field} reaches the actual native Boolean field without acronym recasing`, async t => {
    const root = await mkdtemp(join(tmpdir(), 'awiki-im-core-node-e2ee-options-'))
    t.after(() => rm(root, { recursive: true, force: true }))
    // An ignored/mis-cased field silently opens the client; the native DTO must reject it.
    let opened
    await assert.rejects(async () => {
      opened = await openImCoreNodeClient({ ...options(root), [field]: 'not-a-boolean' })
      await opened.close()
    })
    const client = await openImCoreNodeClient({ ...options(root), [field]: true })
    await client.close()
  })
}

