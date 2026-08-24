import { spawnSync } from 'node:child_process'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const cargo = spawnSync(
  'cargo',
  ['tree', '-p', 'awiki-im-core-node', '--format', '{p}'],
  { cwd: repositoryRoot, encoding: 'utf8' },
)

if (cargo.status !== 0) {
  process.stderr.write(cargo.stderr)
  process.exit(cargo.status ?? 1)
}

if (/(?:^|\s)anp-identity v\d/m.test(cargo.stdout)) {
  process.stderr.write(
    'awiki-im-core-node must use the external identity provider and must not link anp-identity\n',
  )
  process.exit(1)
}
