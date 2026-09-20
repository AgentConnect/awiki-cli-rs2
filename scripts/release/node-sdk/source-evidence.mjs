import { createHash } from 'node:crypto'
import { readFile } from 'node:fs/promises'
import { join } from 'node:path'

const digest = bytes => createHash('sha256').update(bytes).digest('hex')

export async function readSourceEvidence(root, commit) {
  const directory = join(root, '.artifacts/dependencies/source')
  const [receiptBytes, metadataBytes, manifest, lock] = await Promise.all([
    readFile(join(directory, 'resolution.json')),
    readFile(join(directory, 'metadata.json')),
    readFile(join(root, 'dependencies.source.json')),
    readFile(join(root, 'dependencies.source.Cargo.lock')),
  ])
  const receipt = JSON.parse(receiptBytes)
  if (receipt.mode !== 'source' || receipt.consumer?.commit !== commit || receipt.consumer?.dirty !== false ||
      receipt.source_manifest_sha256 !== digest(manifest) || receipt.source_lock_sha256 !== digest(lock) ||
      receipt.metadata_sha256 !== digest(metadataBytes)) {
    throw new Error('Source package evidence does not match the clean consumer, manifest, lock and resolved metadata')
  }
  return { receipt, metadata: JSON.parse(metadataBytes) }
}
