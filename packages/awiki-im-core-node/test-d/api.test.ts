import {
  type ImCoreNodeClient,
  type ImCoreNodeOpenOptions,
  ImCoreNodeError,
  openImCoreNodeClient,
} from '../src/index.js'

const options: ImCoreNodeOpenOptions = {
  stateRoot: '/tmp/awiki-im-core-node-types',
  vaultRootKey: new Uint8Array(32),
  vaultWorkspaceId: 'type-tests',
  vaultDeviceId: 'test-device',
  serviceBaseUrl: 'https://awiki.info',
  didDomain: 'awiki.info',
}

const opened: Promise<ImCoreNodeClient> = openImCoreNodeClient(options)
void opened.then(async client => {
  const attempt = await client.prepareExternalHttpRequest({
    url: 'https://api.example.com/orders',
    method: 'POST',
    headers: [{ name: 'content-type', value: 'application/json' }],
    body: new Uint8Array(),
  })
  attempt.targetUrl satisfies string
  attempt.headerPatch satisfies readonly { readonly name: string, readonly value: string }[]
  const retry = await attempt.handleResponse({ statusCode: 200, headers: [] })
  retry satisfies import('../src/index.js').ExternalHttpAuthAttempt | null
  await client.sendAttachment({
    conversationId: 'group:did:example:group',
    fileName: 'bytes.bin',
    mimeType: 'application/octet-stream',
    bytes: new Uint8Array(256 * 1024),
  })
  const download = await client.downloadAttachment({
    conversationId: 'group:did:example:group',
    messageId: 'message-1',
  })
  download.bytes satisfies Uint8Array
  await client.close()
})

const error = new ImCoreNodeError('network', 'safe', true)
error.code satisfies string
error.retryable satisfies boolean
