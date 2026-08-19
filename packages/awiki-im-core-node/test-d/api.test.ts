import {
  type ImCoreNodeClient,
  type ImCoreNodeOpenOptions,
  type RealtimeEvent,
  ImCoreNodeError,
  openImCoreNodeClient,
} from '../src/index.js'

const options: ImCoreNodeOpenOptions = {
  stateRoot: '/tmp/awiki-im-core-node-types',
  serviceBaseUrl: 'https://awiki.info',
  didDomain: 'awiki.info',
  mailServiceEndpoint: 'https://mail.awiki.info',
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
  const group = await client.createGroup({ name: 'Release Crew', description: 'ships together' })
  group.conversationId satisfies string
  const member = await client.addGroupMember({ groupDid: group.did, member: 'alice' })
  member.did satisfies string
  const localTimeline = await client.getLocalConversationTimeline({ conversationId: group.conversationId })
  localTimeline.items satisfies readonly import('../src/index.js').NodeMessage[]
  const profiles = await client.hydrateDisplayProfiles({ peers: ['did:wba:awiki.info:user:alice'] })
  profiles satisfies readonly import('../src/index.js').NodeDisplayProfile[]
  let realtime = await client.startRealtime()
  const event: RealtimeEvent | null = await realtime.nextEvent()
  if (event === null) {
    await realtime.stop()
    await client.syncNow({ reason: 'websocket_reconnect' })
    realtime = await client.startRealtime()
  }
  else if (event.kind === 'sync_required') {
    event.cause satisfies string
    event.dirty satisfies boolean
    await client.syncNow({
      reason: event.cause === 'reconnected' ? 'websocket_reconnect' : 'websocket_hint',
    })
  }
  await realtime.stop()
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
  const account = await client.getMailAccount()
  account.mailboxAddress satisfies string | undefined
  const inbox = await client.listMailInbox({
    folder: 'inbox',
    unreadOnly: true,
    limit: 20,
    offset: 0,
  })
  inbox.nextOffset satisfies number | undefined
  const mail = await client.readMail(inbox.items[0]?.id ?? 'mail-1')
  mail.bodyText satisfies string | undefined
  mail.hasHtmlBody satisfies boolean
  mail.attachments[0]?.sizeBytes satisfies string | undefined
  const marked = await client.markMailRead({ messageIds: ['mail-1'] })
  marked.updated satisfies number
  const sent = await client.sendMail({
    to: ['recipient@example.test'],
    cc: ['copy@example.test'],
    subject: 'Subject',
    bodyText: 'Plain-text body',
  })
  sent.accepted satisfies boolean
  await client.close()
})

const error = new ImCoreNodeError('network', 'safe', true)
error.code satisfies string
error.retryable satisfies boolean
