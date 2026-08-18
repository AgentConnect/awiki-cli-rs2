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
  const identities = await client.listIdentities()
  identities satisfies readonly import('../src/index.js').NodeIdentity[]
  const defaultIdentity = await client.getDefaultIdentity()
  if (defaultIdentity !== null) {
    defaultIdentity.isDefault satisfies boolean
    const identityClient = await client.forIdentity(defaultIdentity.identityId)
    const selected = await identityClient.getIdentity()
    selected.identityId satisfies string
  }
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
  const localTimeline = await client.getLocalConversationTimeline({
    conversationId: 'group:did:example:group',
    limit: 50,
  })
  localTimeline.items satisfies readonly import('../src/index.js').NodeMessage[]
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
  const provisioned = await client.provisionSkillAgentIdentity({
    operationId: 'agbind_types_0001',
    displayName: 'Research Agent',
    controllerIdentityId: 'controller-identity',
  })
  provisioned.did satisfies string
  await client.acknowledgeSkillAgentProvision('agbind_types_0001')
  await client.close()
})

const error = new ImCoreNodeError('network', 'safe', true)
error.code satisfies string
error.retryable satisfies boolean
