import assert from 'node:assert/strict'
import { mkdtemp, rm } from 'node:fs/promises'
import { createServer } from 'node:http'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

/** Exercise an actual installed native SDK; every request stays on loopback. */
export async function verifyClientProducts(openImCoreNodeClient) {
  const root = await mkdtemp(join(tmpdir(), 'awiki-client-products-'))
  const requests = []
  const did = 'did:wba:example.test:users:alice'
  const server = createServer((request, response) => {
    const chunks = []
    request.on('data', chunk => chunks.push(chunk))
    request.on('end', () => {
      requests.push({ path: request.url, headers: request.headers, rawHeaders: request.rawHeaders })
      let body
      if (request.url === '/user-service/v1/handle/rpc' && request.method === 'POST') {
        const input = JSON.parse(Buffer.concat(chunks).toString('utf8'))
        body = { jsonrpc: '2.0', id: input.id, result: {
          ok: true, retry_after_seconds: 60, retry_at: '2099-01-01T00:00:00Z',
        } }
      } else if (request.url === '/.well-known/handle/alice' && request.method === 'GET') {
        body = { status: 'active', handle: 'alice.example.test', did, binding_generation: '1' }
      } else {
        response.writeHead(404).end()
        return
      }
      response.writeHead(200, { 'content-type': 'application/json' })
      response.end(JSON.stringify(body))
    })
  })
  await new Promise((resolve, reject) => {
    server.once('error', reject)
    server.listen(0, '127.0.0.1', resolve)
  })
  const address = server.address()
  assert.ok(address && typeof address === 'object')
  const base = {
    serviceBaseUrl: `http://127.0.0.1:${address.port}`,
    didDomain: 'example.test', operationTimeoutMs: 5_000, syncTimeoutMs: 100,
  }
  try {
    for (const [product, version, build] of [
      ['awiki-me', '1.0.58', 214],
      ['awiki-cli', '1.0.58'],
      ['awiki-daemon', '0.1.93'],
      ['awiki-dsh', '0.3.17'],
      ['awiki-gateway', '0.1.0'],
    ]) {
      const clientVersionInfo = { product, release: '0815', version, ...(build === undefined ? {} : { build }) }
      const client = await openImCoreNodeClient({ ...base, stateRoot: join(root, product), clientVersionInfo })
      try {
        assert.equal(await client.getDefaultIdentity(), null)
        const challenge = await client.requestRegistrationOtp({ handle: 'version-fixture', phone: '+12025550123' })
        assert.equal(challenge.retryAfterSeconds, 60)
        const productRequest = requests.at(-1)
        assert.equal(productRequest.path, '/user-service/v1/handle/rpc')
        assert.equal(productRequest.headers['x-awiki-client-version'],
          `${product}/0815/${version}${build === undefined ? '' : `+${build}`}`)
        assert.equal(productRequest.rawHeaders.filter((name, index) =>
          index % 2 === 0 && name.toLowerCase() === 'x-awiki-client-version').length, 1)

        assert.equal(await client.resolveHandleForDeviceJoin('alice.example.test'), did)
        const publicRequest = requests.at(-1)
        assert.equal(publicRequest.path, '/.well-known/handle/alice')
        assert.equal(publicRequest.headers['x-awiki-client-version'], undefined)
        assert.equal(publicRequest.headers.authorization, undefined)
        assert.equal(await client.getDefaultIdentity(), null)
      } finally {
        await client.close()
      }
    }
    for (const [index, product] of [
      'custom', 'awiki-guest', 'AWIKI-DSH', ' awiki-dsh', 'awiki-gateway ',
      'awiki-dsh/0815', 'awiki-gateway\r\nx-injected: value',
    ].entries()) {
      let opened
      await assert.rejects(async () => {
        opened = await openImCoreNodeClient({ ...base, stateRoot: join(root, `invalid-${index}`),
          clientVersionInfo: { product, release: '0815', version: '0.1.0' } })
        await opened.close()
      }, error => error.code === 'invalid_input')
    }
    assert.equal(requests.length, 10)
    return { acceptedProducts: 5, rejectedProducts: 7, productHttpRequests: 5, publicDiscoveryRequests: 5 }
  } finally {
    await new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve()))
    await rm(root, { recursive: true, force: true })
  }
}
