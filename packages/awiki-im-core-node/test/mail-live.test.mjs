import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { mkdtemp, rm } from 'node:fs/promises'
import { createServer } from 'node:http'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'

import { ImCoreNodeError, openImCoreNodeClient } from '../dist/index.js'

function syntheticAccessToken({ did, userId, deviceId, keyId }) {
  const now = Math.floor(Date.now() / 1000)
  const claims = {
    iss: 'user-service',
    aud: ['awiki-user-service', 'awiki-message-service'],
    sub: did,
    type: 'access',
    purpose: 'awiki.device.access.v1',
    did,
    user_id: userId,
    device_id: deviceId,
    key_id: keyId,
    auth_generation: 1,
    scopes: ['device:manage', 'device:read', 'message:connect'],
    iat: now,
    nbf: now,
    exp: now + 3600,
    jti: `synthetic-${deviceId}`,
  }
  return `e30.${Buffer.from(JSON.stringify(claims)).toString('base64url')}.synthetic`
}

async function readRpc(request) {
  const chunks = []
  for await (const chunk of request) chunks.push(chunk)
  return JSON.parse(Buffer.concat(chunks).toString('utf8'))
}

function sendRpcResult(response, rpc, result) {
  const body = JSON.stringify({ jsonrpc: '2.0', id: rpc.id, result })
  response.writeHead(200, {
    'content-type': 'application/json',
    'content-length': Buffer.byteLength(body),
    connection: 'close',
  })
  response.end(body)
}

function sendRpcError(response, rpc, code, message, data) {
  const body = JSON.stringify({
    jsonrpc: '2.0',
    id: rpc.id,
    error: { code, message, ...(data === undefined ? {} : { data }) },
  })
  response.writeHead(200, {
    'content-type': 'application/json',
    'content-length': Buffer.byteLength(body),
    connection: 'close',
  })
  response.end(body)
}

async function listen(server) {
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve))
  const address = server.address()
  assert(address && typeof address === 'object')
  return `http://127.0.0.1:${address.port}`
}

function closeServer(server) {
  return new Promise((resolve, reject) => {
    server.close(error => error ? reject(error) : resolve())
  })
}

async function syntheticIdentityService() {
  const requests = []
  const server = createServer(async (request, response) => {
    try {
      const rpc = await readRpc(request)
      requests.push({ method: rpc.method, path: request.url })
      let result
      if (rpc.method === 'send_otp') {
        result = {
          ok: true,
          retry_after_seconds: 60,
          retry_at: '2026-08-18T00:00:00Z',
        }
      }
      else if (rpc.method === 'register') {
        const document = rpc.params.did_document
        const device = document.deviceManifest.devices[0]
        const handle = rpc.params.handle
        const domain = document.id.slice('did:wba:'.length).split(':')[0]
        const userId = `synthetic-user-${handle}`
        result = {
          state: 'registered',
          did: document.id,
          user_id: userId,
          message: 'Registration successful',
          access_token: syntheticAccessToken({
            did: document.id,
            userId,
            deviceId: device.device_id,
            keyId: device.signing_key_id,
          }),
          handle,
          domain,
          full_handle: `${handle}.${domain}`,
          binding_generation: '1',
        }
      }
      else if (rpc.method === 'direct.e2ee.publish_prekey_bundle') {
        const body = rpc.params.body
        result = {
          published: true,
          owner_did: body.prekey_bundle.owner_did,
          owner_device_id: body.prekey_bundle.owner_device_id,
          bundle_id: body.prekey_bundle.bundle_id,
          published_at: '2026-08-18T00:00:00Z',
          published_opk_count: body.one_time_prekeys.length,
        }
      }
      else if (rpc.method === 'get_me') {
        result = {
          did: 'did:wba:legacy.test:user:mail-live:synthetic-profile',
          user_id: 'synthetic-user-mail-live',
          handle: 'mail-live',
          full_handle: 'mail-live.legacy.test',
          nick_name: 'mail-live',
        }
      }
      else {
        throw new Error(`unexpected identity RPC method: ${rpc.method}`)
      }
      sendRpcResult(response, rpc, result)
    }
    catch {
      response.writeHead(500, { connection: 'close' })
      response.end()
    }
  })
  return {
    baseUrl: await listen(server),
    requests,
    close: () => closeServer(server),
  }
}

async function syntheticMailService() {
  const requests = []
  const attachmentBytes = Buffer.from([0, 1, 2, 3, 254, 255])
  let markBlockedStarted
  let releaseBlocked
  const blockedStarted = new Promise(resolve => { markBlockedStarted = resolve })
  const blocked = new Promise(resolve => { releaseBlocked = resolve })
  const server = createServer(async (request, response) => {
    try {
      const rpc = await readRpc(request)
      requests.push({
        method: rpc.method,
        params: rpc.params,
        path: request.url,
        authorization: request.headers.authorization,
        contentDigest: request.headers['content-digest'],
        signature: request.headers.signature,
        signatureInput: request.headers['signature-input'],
      })

      if (rpc.method === 'mail.send' && rpc.params.subject === 'transport-secret-subject') {
        response.destroy()
        return
      }
      if (rpc.method === 'mail.send' && rpc.params.subject === 'final-mime-limit') {
        sendRpcError(response, rpc, -32004, 'localized service text changed completely', {
          awiki_code: 'mail.message_size_limit',
        })
        return
      }
      if (rpc.method === 'mail.getAttachment' && rpc.params.message_id === 'blocked-mail') {
        markBlockedStarted()
        await blocked
      }
      if (rpc.method === 'mail.getAttachment' && rpc.params.attachment_index === 99) {
        sendRpcError(response, rpc, -32004, 'private-index-secret')
        return
      }

      let result
      if (rpc.method === 'mail.getMailbox') {
        result = {
          mailbox_address: 'mail-live@legacy.test',
          display_name: 'Mail Live',
          status: 'active',
          private_attribute: 'account-secret',
        }
      }
      else if (rpc.method === 'mail.getInbox') {
        result = {
          messages: [{
            id: 'mail-live-1',
            folder: 'inbox',
            from: ['sender@example.test'],
            to: ['mail-live@legacy.test'],
            cc: [],
            subject: 'Live fixture subject',
            preview: 'Live fixture preview',
            received_at: '2026-08-18T07:00:00Z',
            unread: true,
            has_attachments: true,
            attachment_count: 1,
            private_attribute: 'summary-secret',
          }],
          has_more: true,
        }
      }
      else if (rpc.method === 'mail.getMessage') {
        result = {
          id: rpc.params.message_id,
          folder: 'inbox',
          from: ['sender@example.test'],
          to: ['mail-live@legacy.test'],
          cc: [],
          subject: 'Live fixture subject',
          received_at: '2026-08-18T07:00:00Z',
          unread: true,
          has_attachments: true,
          attachment_count: 1,
          body_text: 'Plain text fixture body',
          body_html: '<p>html-secret</p>',
          attachments: [{
            index: 0,
            filename: 'fixture.txt',
            content_type: 'text/plain',
            size: attachmentBytes.length,
          }],
          private_attribute: 'message-secret',
        }
      }
      else if (rpc.method === 'mail.markRead') {
        result = { updated: rpc.params.message_ids.length }
      }
      else if (rpc.method === 'mail.getAttachment') {
        result = {
          index: rpc.params.attachment_index,
          filename: 'fixture.bin',
          content_type: 'application/octet-stream',
          size: rpc.params.attachment_index === 98
            ? attachmentBytes.length + 1
            : attachmentBytes.length,
          content_base64: attachmentBytes.toString('base64'),
        }
      }
      else if (rpc.method === 'mail.send') {
        result = rpc.params.subject === 'ambiguous-send'
          ? { accepted: true, message_id: 'must-not-pass' }
          : { accepted: true, status: 'sent', message_id: 'mail-sent-1', warnings: ['queued'] }
      }
      else {
        throw new Error(`unexpected mail RPC method: ${rpc.method}`)
      }
      sendRpcResult(response, rpc, result)
    }
    catch {
      if (!response.destroyed) {
        response.writeHead(500, { connection: 'close' })
        response.end()
      }
    }
  })
  return {
    baseUrl: await listen(server),
    requests,
    attachmentBytes,
    blockedStarted,
    releaseBlocked: () => releaseBlocked(),
    close: () => closeServer(server),
  }
}

function options(stateRoot, identityUrl, mailUrl) {
  return {
    stateRoot,
    serviceBaseUrl: identityUrl,
    userServiceEndpoint: identityUrl,
    messageServiceEndpoint: identityUrl,
    mailServiceEndpoint: mailUrl,
    anpServiceEndpoint: identityUrl,
    didDomain: 'legacy.test',
    operationTimeoutMs: 10_000,
    syncTimeoutMs: 1_000,
  }
}

test('mail facade uses the identity-bound mail transport and keeps the v1 projection closed', {
  timeout: 30_000,
}, async t => {
  const stateRoot = await mkdtemp(join(tmpdir(), 'awiki-im-core-node-mail-live-'))
  const identityService = await syntheticIdentityService()
  const mailService = await syntheticMailService()
  let client
  t.after(async () => {
    mailService.releaseBlocked()
    await client?.close()
    await Promise.all([identityService.close(), mailService.close()])
    await rm(stateRoot, { recursive: true, force: true })
  })

  client = await openImCoreNodeClient(options(
    stateRoot,
    identityService.baseUrl,
    mailService.baseUrl,
  ))
  await client.requestRegistrationOtp({
    handle: 'mail-live.legacy.test',
    phone: '+15551234569',
  })
  const identity = await client.completeRegistration({
    handle: 'mail-live.legacy.test',
    phone: '+15551234569',
    otp: '123456',
  })

  assert.deepEqual(identityService.requests.slice(0, 3), [
    { method: 'send_otp', path: '/user-service/v1/handle/rpc' },
    { method: 'register', path: '/user-service/v1/did-auth/rpc' },
    { method: 'direct.e2ee.publish_prekey_bundle', path: '/im/rpc' },
  ])
  assert.equal(identityService.requests.some(request => request.path === '/mail/rpc'), false)

  const account = await client.getMailAccount()
  assert.deepEqual(account, {
    mailboxAddress: 'mail-live@legacy.test',
    displayName: 'Mail Live',
    status: 'active',
  })

  const inbox = await client.listMailInbox()
  assert.equal(inbox.items.length, 1)
  assert.equal(inbox.items[0].id, 'mail-live-1')
  assert.equal(inbox.items[0].subject, 'Live fixture subject')
  assert.equal(inbox.nextOffset, 1)
  assert.equal(inbox.hasMore, true)

  const message = await client.readMail('mail-live-1')
  assert.equal(message.bodyText, 'Plain text fixture body')
  assert.equal(message.bodyTruncated, false)
  assert.equal(message.hasHtmlBody, true)
  assert.equal(message.attachments[0].sizeBytes, String(mailService.attachmentBytes.length))
  const projected = JSON.stringify({ account, inbox, message })
  for (const hidden of ['bodyHtml', 'html-secret', 'privateAttribute', 'account-secret', 'summary-secret', 'message-secret']) {
    assert.equal(projected.includes(hidden), false)
  }

  assert.deepEqual(await client.markMailRead({ messageIds: ['mail-live-1'] }), { updated: 1 })
  const outboundBytes = new Uint8Array([0, 1, 2, 3, 254, 255])
  const sendPromise = client.sendMail({
    to: ['recipient@example.test'],
    cc: ['copy@example.test'],
    subject: 'Fixture subject',
    bodyText: 'Fixture body',
    attachments: [{
      fileName: 'fixture.bin',
      contentType: 'application/octet-stream',
      bytes: outboundBytes,
    }],
  })
  outboundBytes.fill(42)
  assert.deepEqual(await sendPromise, {
    accepted: true,
    messageId: 'mail-sent-1',
    warnings: ['queued'],
  })

  const download = await client.downloadMailAttachment({
    messageId: 'mail-live-1',
    attachmentIndex: 0,
  })
  assert.equal(download.fileName, 'fixture.bin')
  assert.equal(download.contentType, 'application/octet-stream')
  assert.equal(download.sizeBytes, String(mailService.attachmentBytes.length))
  assert.deepEqual(Buffer.from(download.bytes), mailService.attachmentBytes)
  assert.equal(
    createHash('sha256').update(download.bytes).digest('hex'),
    createHash('sha256').update(mailService.attachmentBytes).digest('hex'),
  )

  const successful = mailService.requests.slice(0, 6)
  assert.deepEqual(successful.map(request => ({
    method: request.method,
    params: request.params,
    path: request.path,
  })), [
    { method: 'mail.getMailbox', params: {}, path: '/mail/rpc' },
    {
      method: 'mail.getInbox',
      params: { folder: 'inbox', limit: 20, offset: 0, unread_only: false },
      path: '/mail/rpc',
    },
    { method: 'mail.getMessage', params: { message_id: 'mail-live-1' }, path: '/mail/rpc' },
    {
      method: 'mail.markRead',
      params: { message_ids: ['mail-live-1'], is_read: true },
      path: '/mail/rpc',
    },
    {
      method: 'mail.send',
      params: {
        to: ['recipient@example.test'],
        cc: ['copy@example.test'],
        subject: 'Fixture subject',
        body_text: 'Fixture body',
        body_html: null,
        attachments: [{
          filename: 'fixture.bin',
          content_type: 'application/octet-stream',
          content_base64: mailService.attachmentBytes.toString('base64'),
        }],
      },
      path: '/mail/rpc',
    },
    {
      method: 'mail.getAttachment',
      params: { message_id: 'mail-live-1', attachment_index: 0 },
      path: '/mail/rpc',
    },
  ])
  for (const request of successful) {
    assert.equal(request.authorization, undefined)
    assert.equal(typeof request.contentDigest, 'string')
    assert.equal(typeof request.signature, 'string')
    assert.equal(typeof request.signatureInput, 'string')
    assert.equal(request.signatureInput.includes(identity.did), true)
  }

  const transportSecrets = [
    'transport-secret@example.test',
    'transport-secret-subject',
    'transport-secret-body',
    '/mail/rpc',
    '127.0.0.1',
  ]
  await assert.rejects(client.sendMail({
    to: [transportSecrets[0]],
    subject: transportSecrets[1],
    bodyText: transportSecrets[2],
  }), error => {
    assert(error instanceof ImCoreNodeError)
    assert.equal(error.code, 'transport_unavailable')
    for (const secret of transportSecrets) assert.equal(error.message.includes(secret), false)
    return true
  })
  assert.equal(mailService.requests.filter(request =>
    request.method === 'mail.send' && request.params.subject === transportSecrets[1]).length, 1)

  await assert.rejects(client.sendMail({
    to: ['recipient@example.test'],
    subject: 'ambiguous-send',
    bodyText: 'Body',
  }), error => error instanceof ImCoreNodeError && error.code === 'service_error')

  await assert.rejects(client.sendMail({
    to: ['recipient@example.test'],
    subject: 'final-mime-limit',
    bodyText: 'Body',
  }), error => error instanceof ImCoreNodeError
    && error.code === 'invalid_input'
    && error.retryable === false
    && !error.message.includes('localized service text'))

  const requestsBeforeInvalidIndex = mailService.requests.length
  for (const attachmentIndex of [-1, 1.5, 2 ** 32, Number.NaN, Number.POSITIVE_INFINITY]) {
    await assert.rejects(client.downloadMailAttachment({
      messageId: 'mail-live-1',
      attachmentIndex,
    }), error => error instanceof ImCoreNodeError && error.code === 'invalid_input')
  }
  assert.equal(mailService.requests.length, requestsBeforeInvalidIndex)

  await assert.rejects(client.downloadMailAttachment({
    messageId: 'mail-live-1',
    attachmentIndex: 98,
  }), error => error instanceof ImCoreNodeError && error.code === 'internal')
  await assert.rejects(client.downloadMailAttachment({
    messageId: 'mail-live-1',
    attachmentIndex: 99,
  }), error => {
    assert(error instanceof ImCoreNodeError)
    assert.equal(error.code, 'service_error')
    assert.equal(error.message.includes('private-index-secret'), false)
    return true
  })

  const blockedDownload = client.downloadMailAttachment({
    messageId: 'blocked-mail',
    attachmentIndex: 0,
  })
  const blockedRejection = assert.rejects(blockedDownload, error =>
    error instanceof ImCoreNodeError
      && error.code === 'cancelled'
      && error.safeMessage === 'The IM operation was cancelled.')
  await mailService.blockedStarted
  await client.close()
  await blockedRejection
  mailService.releaseBlocked()
  await assert.rejects(client.getMailAccount(), error =>
    error instanceof ImCoreNodeError && error.code === 'client_closed')
  assert.equal(mailService.requests.filter(request =>
    request.method === 'mail.getAttachment' && request.params.message_id === 'blocked-mail').length, 1)
})
