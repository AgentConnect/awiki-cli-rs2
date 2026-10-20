import test from 'node:test'
import { openImCoreNodeClient } from '../dist/index.js'
import { verifyClientProducts } from '../../../scripts/release/node-sdk/verify-client-products.mjs'

test('real native SDK isolates five products and omits product headers on public discovery', async () => {
  await verifyClientProducts(openImCoreNodeClient)
})
