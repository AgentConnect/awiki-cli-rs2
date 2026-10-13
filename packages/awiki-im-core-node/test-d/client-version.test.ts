import type { ImCoreNodeOpenOptions } from '../src/index.js'

type ClientVersion = NonNullable<ImCoreNodeOpenOptions['clientVersionInfo']>

const products: readonly ClientVersion[] = [
  { product: 'awiki-me', release: '0815', version: '1.0.58', build: 214 },
  { product: 'awiki-cli', release: '0815', version: '1.0.58' },
  { product: 'awiki-daemon', release: '0815', version: '0.1.93' },
  { product: 'awiki-dsh', release: '0815', version: '0.3.17' },
  { product: 'awiki-gateway', release: '0815', version: '0.1.0' },
]
void products

// @ts-expect-error Guest uses the explicit awiki-gateway product, not an alias.
const alias: ClientVersion = { product: 'awiki-guest', release: '0815', version: '0.1.0' }
// @ts-expect-error The product contract is a closed union, not an arbitrary string.
const unknown: ClientVersion = { product: 'custom', release: '0815', version: '0.1.0' }
void [alias, unknown]
