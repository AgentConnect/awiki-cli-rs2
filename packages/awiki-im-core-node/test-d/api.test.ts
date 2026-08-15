import {
  type ImCoreNodeClient,
  type ImCoreNodeOpenOptions,
  ImCoreNodeError,
  openImCoreNodeClient,
} from '../src/index.js'

const options: ImCoreNodeOpenOptions = {
  stateRoot: '/tmp/awiki-im-core-node-types',
  serviceBaseUrl: 'https://awiki.info',
  didDomain: 'awiki.info',
}

const opened: Promise<ImCoreNodeClient> = openImCoreNodeClient(options)
void opened.then(async client => {
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
