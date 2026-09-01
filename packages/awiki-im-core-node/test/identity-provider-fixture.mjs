import { createRequire } from 'node:module'
import { join } from 'node:path'

const require = createRequire(import.meta.url)
const { IdentityProvider } = require(
  '../../../../anp/anp-identity/bindings/node/provider.js',
)

const CAPABILITIES = [
  'IDENTITY_READ',
  'IDENTITY_CREATE',
  'IDENTITY_IMPORT',
  'IDENTITY_SIGN',
  'IDENTITY_ECDH_SEALED',
  'IDENTITY_DOCUMENT_UPDATE',
  'IDENTITY_KEY_LIFECYCLE',
  'IDENTITY_DELETE',
  'IDENTITY_HTTP_SIGNATURE',
  'AWIKI_LEGACY_ROOT_TRANSFER_V1',
]

export async function createIdentityProviderFixture(stateRoot) {
  const provider = await IdentityProvider.initialize({
    stateRoot: join(stateRoot, 'anp-identity'),
    rootKeyKind: 'local_private_file',
  })
  const lease = provider.acquireLease({
    consumer: '@awiki/im-core-node-test',
    capabilities: CAPABILITIES,
    ttlSeconds: 600,
  })
  return Object.assign(lease, {
    protocol: 'anp-identity-provider-ts/1',
    consumer: '@awiki/im-core-node-test',
    capabilities: CAPABILITIES,
  })
}
