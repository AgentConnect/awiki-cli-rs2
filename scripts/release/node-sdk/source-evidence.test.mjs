import { test } from 'node:test'
import assert from 'node:assert/strict'
import { mkdtemp, mkdir, writeFile, rm } from 'node:fs/promises'
import { join } from 'node:path'
import { tmpdir } from 'node:os'
import { createHash } from 'node:crypto'
import { readSourceEvidence } from './source-evidence.mjs'

test('source package provenance rejects stale locks, metadata and consumer commits', async () => {
  const root = await mkdtemp(join(tmpdir(), 'node-source-evidence-'))
  try {
    const dir = join(root, '.artifacts/dependencies/source')
    await mkdir(dir, { recursive: true })
    const hash = text => createHash('sha256').update(text).digest('hex')
    const metadata = '{"packages":[{"name":"awiki-im-core","version":"0.1.5"}]}'
    const receipt = { mode: 'source', consumer: { commit: 'a'.repeat(40), dirty: false }, source_manifest_sha256: hash('{}'), source_lock_sha256: hash('locked'), metadata_sha256: hash(metadata) }
    await writeFile(join(root, 'dependencies.source.json'), '{}')
    await writeFile(join(root, 'dependencies.source.Cargo.lock'), 'locked')
    await writeFile(join(dir, 'metadata.json'), metadata)
    await writeFile(join(dir, 'resolution.json'), JSON.stringify(receipt))
    assert.equal((await readSourceEvidence(root, 'a'.repeat(40))).metadata.packages[0].version, '0.1.5')
    await assert.rejects(readSourceEvidence(root, 'b'.repeat(40)))
    for (const [file, altered, original] of [
      [join(root, 'dependencies.source.json'), '{"changed":true}', '{}'],
      [join(root, 'dependencies.source.Cargo.lock'), 'stale', 'locked'],
      [join(dir, 'metadata.json'), '{"packages":[]}', metadata],
      [join(dir, 'resolution.json'), JSON.stringify({ ...receipt, consumer: { ...receipt.consumer, dirty: true } }), JSON.stringify(receipt)],
    ]) {
      await writeFile(file, altered)
      await assert.rejects(readSourceEvidence(root, 'a'.repeat(40)))
      await writeFile(file, original)
    }
  } finally { await rm(root, { recursive: true, force: true }) }
})
