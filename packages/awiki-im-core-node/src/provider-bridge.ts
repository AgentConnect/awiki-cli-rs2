import type {
  NativeIdentityProviderCall,
  NativeIdentityProviderDispatch,
  NativeIdentityProviderReply,
} from './native.js'
import {
  ImCoreNodeError,
  type ImCoreIdentityProvider,
  type ImCoreIdentityReference,
  type ImCoreJsonValue,
  type ImCorePreparedIdentityMaterialImport,
  type ImCoreProviderDocumentChangeSession,
  type ImCoreProviderEnrollmentSession,
  type ImCoreProviderIdentityTransitionSession,
  type ImCorePreparedRootImport,
} from './types.js'

const IDENTITY_PROVIDER_PROTOCOL = 'anp-identity-provider-ts/1'
const REQUIRED_CAPABILITIES = [
  'IDENTITY_READ',
  'IDENTITY_SIGN',
  'IDENTITY_ECDH_SEALED',
  'IDENTITY_HTTP_SIGNATURE',
  'IDENTITY_CREATE',
  'IDENTITY_DOCUMENT_UPDATE',
] as const

/** @internal Host-only bridge; this file is not exported by the package. */
export function createIdentityProviderDispatch(
  provider: ImCoreIdentityProvider,
): NativeIdentityProviderDispatch {
  if (provider.protocol !== IDENTITY_PROVIDER_PROTOCOL) throw incompatible()
  const capabilities = new Set(provider.capabilities)
  if (REQUIRED_CAPABILITIES.some(capability => !capabilities.has(capability))) throw incompatible()
  for (const method of [
    'info', 'recover', 'list', 'publicIdentity', 'hostStatus', 'create', 'delete', 'recoverIdentity',
    'sign', 'signOriginProof', 'signDocumentProof', 'prepareHttpSignature', 'ecdhSealed',
    'prepareDocumentChange', 'resumeDocumentChange', 'adoptVerifiedDocument',
    'prepareIdentityTransition', 'resumeIdentityTransition',
    'beginDeviceEnrollment', 'beginRequestSigningEnrollment', 'resumeEnrollment',
  ] as const) {
    if (typeof provider[method] !== 'function') throw incompatible()
  }

  const documentSessions = new Map<string, ImCoreProviderDocumentChangeSession>()
  const transitionSessions = new Map<string, ImCoreProviderIdentityTransitionSession>()
  const enrollmentSessions = new Map<string, ImCoreProviderEnrollmentSession>()
  const rootImportSessions = new Map<string, ImCorePreparedRootImport>()
  const identityImportSessions = new Map<string, ImCorePreparedIdentityMaterialImport>()
  let nextDocumentSession = 1
  let nextTransitionSession = 1
  let nextEnrollmentSession = 1
  let nextRootImportSession = 1
  let nextIdentityImportSession = 1

  return async (calls: readonly [NativeIdentityProviderCall]): Promise<NativeIdentityProviderReply> => {
    try {
      const [request] = calls
      const payload = object(JSON.parse(request.payloadJson))
      switch (request.operation) {
        case 'info': return success(await provider.info())
        case 'recover':
          await provider.recover()
          return success(null)
        case 'list': return success(await provider.list())
        case 'publicIdentity':
          return success(await provider.publicIdentity(reference(payload.identity)))
        case 'hostStatus':
          return success(await provider.hostStatus(reference(payload.identity)))
        case 'create': return success(await provider.create(payload))
        case 'delete':
          await provider.delete(reference(payload.identity))
          return success(null)
        case 'recoverIdentity':
          await provider.recoverIdentity(reference(payload.identity))
          return success(null)
        case 'sign': {
          const signature = await provider.sign(
            reference(payload.identity),
            { ...signingPurpose(payload), payload: singleBuffer(request.buffers) },
          )
          return success({ kid: signature.kid, algorithm: signature.algorithm }, [signature.bytes])
        }
        case 'signOriginProof':
          return success(await provider.signOriginProof(
            reference(payload.identity),
            object(payload.request) as never,
          ))
        case 'signDocumentProof': {
          return success(jsonValue(await provider.signDocumentProof(
            reference(payload.identity),
            object(payload.request) as never,
          )))
        }
        case 'ecdhSealed': {
          const [peerPublic, recipientPublicKey] = pairBuffers(request.buffers)
          return success(await provider.ecdhSealed({
            identity: reference(payload.identity),
            kid: requiredString(payload.kid),
            peerPublic,
            recipientPublicKey,
            requestId: requiredString(payload.requestId),
          }))
        }
        case 'exportRootKeySealed': {
          if (!capabilities.has('AWIKI_LEGACY_ROOT_TRANSFER_V1')
            || typeof provider.exportRootKeySealed !== 'function') throw unavailable()
          return success(await provider.exportRootKeySealed({
            identity: reference(payload.identity),
            kid: requiredString(payload.kid),
            recipientPublicKey: singleBuffer(request.buffers),
            requestId: requiredString(payload.requestId),
            userPresenceConfirmed: payload.userPresenceConfirmed === true,
          }))
        }
        case 'prepareLegacyRootImport': {
          if (!capabilities.has('AWIKI_LEGACY_ROOT_TRANSFER_V1')
            || typeof provider.prepareLegacyRootImport !== 'function') throw unavailable()
          exactBuffers(request.buffers, 0)
          const prepared = await provider.prepareLegacyRootImport({
            identity: reference(payload.identity),
            evidence: object(payload.evidence),
            encoding: privateKeyEncoding(payload.encoding),
            requestId: requiredString(payload.requestId),
          })
          const offer = prepared.offer()
          if (!Buffer.isBuffer(offer.recipientPublicKey) || offer.recipientPublicKey.length !== 32) {
            throw new TypeError('identity root import recipient key is invalid')
          }
          const sessionId = `root-import-${nextRootImportSession++}`
          rootImportSessions.set(sessionId, prepared)
          return success({
            sessionId,
            offer: {
              requestId: offer.requestId,
              token: offer.token,
              authorization: offer.authorization,
              aad: offer.aad,
            },
          }, [offer.recipientPublicKey])
        }
        case 'completeLegacyRootImport': {
          exactBuffers(request.buffers, 0)
          const sessionId = requiredString(payload.sessionId)
          const prepared = rootImportSession(rootImportSessions, sessionId)
          try {
            return success(await prepared.complete(
              requiredString(payload.token),
              object(payload.envelope) as never,
            ))
          }
          finally {
            rootImportSessions.delete(sessionId)
          }
        }
        case 'importWrappedRoot': {
          if (!capabilities.has('AWIKI_LEGACY_ROOT_TRANSFER_V1')
            || typeof provider.importWrappedRoot !== 'function') throw unavailable()
          exactBuffers(request.buffers, 0)
          return success(await provider.importWrappedRoot(
            reference(payload.identity),
            object(payload.envelope),
          ))
        }
        case 'prepareIdentityMaterialImport': {
          if (!capabilities.has('IDENTITY_IMPORT')
            || typeof provider.prepareIdentityMaterialImport !== 'function') throw unavailable()
          exactBuffers(request.buffers, 0)
          const prepared = await provider.prepareIdentityMaterialImport({
            remote: object(payload.remote),
            didWba: payload.didWba === true,
            keys: identityMaterialKeySpecs(payload.keys),
            requestId: requiredString(payload.requestId),
          })
          const offer = prepared.offer()
          if (!Buffer.isBuffer(offer.recipientPublicKey) || offer.recipientPublicKey.length !== 32) {
            throw new TypeError('identity material import recipient key is invalid')
          }
          const sessionId = `identity-import-${nextIdentityImportSession++}`
          identityImportSessions.set(sessionId, prepared)
          return success({
            sessionId,
            offer: {
              target: offer.target,
              requestId: offer.requestId,
              token: offer.token,
              authorization: offer.authorization,
              itemAad: offer.itemAad,
            },
          }, [offer.recipientPublicKey])
        }
        case 'completeIdentityMaterialImport': {
          exactBuffers(request.buffers, 0)
          const sessionId = requiredString(payload.sessionId)
          const prepared = identityImportSession(identityImportSessions, sessionId)
          try {
            return success(await prepared.complete(
              requiredString(payload.token),
              identityMaterialEnvelopes(payload.envelopes),
            ))
          }
          finally {
            identityImportSessions.delete(sessionId)
          }
        }
        case 'prepareHttpSignature': {
          const hasBody = payload.hasBody === true
          const body = hasBody ? singleBuffer(request.buffers) : undefined
          if (!hasBody) exactBuffers(request.buffers, 0)
          const { hasBody: _hasBody, ...wire } = payload
          return success(await provider.prepareHttpSignature({
            ...wire,
            identity: reference(payload.identity),
            body,
          } as never))
        }
        case 'prepareDocumentChange': {
          const session = await provider.prepareDocumentChange(
            reference(payload.identity),
            object(payload.request),
          )
          const sessionId = `document-change-${nextDocumentSession++}`
          documentSessions.set(sessionId, session)
          return success({ sessionId, candidate: await session.candidate() })
        }
        case 'resumeDocumentChange': {
          const session = await provider.resumeDocumentChange(reference(payload.identity))
          if (session === undefined) return success(null)
          const sessionId = `document-change-${nextDocumentSession++}`
          documentSessions.set(sessionId, session)
          return success({ sessionId, candidate: await session.candidate() })
        }
        case 'prepareIdentityTransition': {
          const session = await provider.prepareIdentityTransition(payload as never)
          const sessionId = `identity-transition-${nextTransitionSession++}`
          transitionSessions.set(sessionId, session)
          return success({ sessionId, candidate: await session.candidate() })
        }
        case 'resumeIdentityTransition': {
          const session = await provider.resumeIdentityTransition(
            requiredString(payload.expectedCurrentDid),
          )
          if (session === undefined) return success(null)
          const sessionId = `identity-transition-${nextTransitionSession++}`
          transitionSessions.set(sessionId, session)
          return success({ sessionId, candidate: await session.candidate() })
        }
        case 'adoptVerifiedDocument':
          return success(await provider.adoptVerifiedDocument(
            reference(payload.identity),
            object(payload.remote),
          ))
        case 'beginDeviceEnrollment': {
          const session = await provider.beginDeviceEnrollment(payload)
          const sessionId = `enrollment-${nextEnrollmentSession++}`
          enrollmentSessions.set(sessionId, session)
          return success({ sessionId, proposal: await session.proposal() })
        }
        case 'beginRequestSigningEnrollment': {
          const session = await provider.beginRequestSigningEnrollment(payload)
          const sessionId = `enrollment-${nextEnrollmentSession++}`
          enrollmentSessions.set(sessionId, session)
          return success({ sessionId, proposal: await session.proposal() })
        }
        case 'resumeEnrollment': {
          const session = await provider.resumeEnrollment(reference(payload.identity))
          if (session === undefined) return success(null)
          const sessionId = `enrollment-${nextEnrollmentSession++}`
          enrollmentSessions.set(sessionId, session)
          return success({ sessionId, proposal: await session.proposal() })
        }
        case 'confirmRootPromotion': {
          if (!capabilities.has('AWIKI_LEGACY_ROOT_TRANSFER_V1')
            || typeof provider.confirmRootPromotion !== 'function') throw unavailable()
          await provider.confirmRootPromotion(
            reference(payload.identity),
            object(payload.request) as { readonly remote: unknown },
          )
          return success(null)
        }
        case 'signPendingRootObjectProof': {
          if (!capabilities.has('AWIKI_LEGACY_ROOT_TRANSFER_V1')
            || typeof provider.signPendingRootObjectProof !== 'function') throw unavailable()
          return success(await provider.signPendingRootObjectProof(
            reference(payload.identity),
            object(payload.request) as {
              readonly kid?: string
              readonly document: unknown
              readonly issuerDid: string
              readonly created?: string
            },
          ))
        }
        case 'documentChangeBeginPublication':
          return success(await documentSession(documentSessions, payload.sessionId).beginPublication())
        case 'documentChangeHostPhase':
          return success(await documentSession(documentSessions, payload.sessionId).hostPhase())
        case 'documentChangeComplete': {
          const sessionId = requiredString(payload.sessionId)
          const outcome = await documentSession(documentSessions, sessionId).complete(
            object(payload.attempt),
            object(payload.result),
          )
          if (isFinalDocumentOutcome(outcome)) documentSessions.delete(sessionId)
          return success(outcome)
        }
        case 'documentChangeReconcile': {
          const sessionId = requiredString(payload.sessionId)
          const outcome = await documentSession(documentSessions, sessionId).reconcile(
            object(payload.observation),
          )
          if (isFinalDocumentOutcome(outcome)) documentSessions.delete(sessionId)
          return success(outcome)
        }
        case 'identityTransitionBeginPublication':
          return success(await transitionSession(
            transitionSessions,
            payload.sessionId,
          ).beginPublication())
        case 'identityTransitionComplete': {
          const sessionId = requiredString(payload.sessionId)
          const outcome = await transitionSession(transitionSessions, sessionId).complete(
            object(payload.attempt),
            object(payload.result),
          )
          if (isFinalDocumentOutcome(outcome)) transitionSessions.delete(sessionId)
          return success(outcome)
        }
        case 'identityTransitionReconcile': {
          const sessionId = requiredString(payload.sessionId)
          const outcome = await transitionSession(transitionSessions, sessionId).reconcile(
            object(payload.observation),
          )
          if (isFinalDocumentOutcome(outcome)) transitionSessions.delete(sessionId)
          return success(outcome)
        }
        case 'enrollmentSignDeviceAssertion': {
          const signature = await enrollmentSession(
            enrollmentSessions,
            payload.sessionId,
          ).signDeviceAssertion(singleBuffer(request.buffers))
          return success(null, [Buffer.from(signature)])
        }
        case 'enrollmentEcdhSealed': {
          const [peerPublic, recipientPublicKey] = pairBuffers(request.buffers)
          return success(await enrollmentSession(
            enrollmentSessions,
            payload.sessionId,
          ).deriveDeviceSharedSecretSealed({
            peerPublic,
            recipientPublicKey,
            requestId: requiredString(payload.requestId),
          }))
        }
        case 'enrollmentActivate': {
          const sessionId = requiredString(payload.sessionId)
          const outcome = await enrollmentSession(enrollmentSessions, sessionId)
            .activate(object(payload.remote))
          enrollmentSessions.delete(sessionId)
          return success(outcome)
        }
        case 'enrollmentCancel': {
          const sessionId = requiredString(payload.sessionId)
          await enrollmentSession(enrollmentSessions, sessionId).cancel()
          enrollmentSessions.delete(sessionId)
          return success(null)
        }
        default: return failure('provider_incompatible', false)
      }
    }
    catch (error) {
      return failure(errorCode(error), errorRetryable(error))
    }
  }
}

function documentSession(
  sessions: ReadonlyMap<string, ImCoreProviderDocumentChangeSession>,
  value: unknown,
): ImCoreProviderDocumentChangeSession {
  const session = sessions.get(requiredString(value))
  if (session === undefined) throw new TypeError('identity provider document session is invalid')
  return session
}

function transitionSession(
  sessions: ReadonlyMap<string, ImCoreProviderIdentityTransitionSession>,
  value: unknown,
): ImCoreProviderIdentityTransitionSession {
  const session = sessions.get(requiredString(value))
  if (session === undefined) throw new TypeError('identity transition session is invalid')
  return session
}

function enrollmentSession(
  sessions: ReadonlyMap<string, ImCoreProviderEnrollmentSession>,
  sessionId: unknown,
): ImCoreProviderEnrollmentSession {
  const session = sessions.get(requiredString(sessionId))
  if (session === undefined) throw new TypeError('identity enrollment session is invalid')
  return session
}

function rootImportSession(
  sessions: ReadonlyMap<string, ImCorePreparedRootImport>,
  sessionId: unknown,
): ImCorePreparedRootImport {
  const session = sessions.get(requiredString(sessionId))
  if (session === undefined) throw new TypeError('identity root import session is invalid')
  return session
}

function identityImportSession(
  sessions: ReadonlyMap<string, ImCorePreparedIdentityMaterialImport>,
  sessionId: unknown,
): ImCorePreparedIdentityMaterialImport {
  const session = sessions.get(requiredString(sessionId))
  if (session === undefined) throw new TypeError('identity material import session is invalid')
  return session
}

function identityMaterialKeySpecs(value: unknown): {
  readonly kid: string
  readonly purpose: 'root_control' | 'authentication' | 'device_assertion' | 'application_assertion' | 'key_agreement'
  readonly encoding: 'raw32' | 'pkcs8_der'
}[] {
  if (!Array.isArray(value)) throw new TypeError('identity material key specs are invalid')
  return value.map(item => {
    const key = object(item)
    const purpose = key.purpose
    if (purpose !== 'root_control'
      && purpose !== 'authentication'
      && purpose !== 'device_assertion'
      && purpose !== 'application_assertion'
      && purpose !== 'key_agreement') {
      throw new TypeError('identity material key purpose is invalid')
    }
    return {
      kid: requiredString(key.kid),
      purpose,
      encoding: privateKeyEncoding(key.encoding),
    }
  })
}

function identityMaterialEnvelopes(value: unknown): readonly {
  readonly protocol: 'anp-sealed-secret/1'
  readonly suite: 'hpke-base-x25519-hkdf-sha256-chacha20poly1305-v1'
  readonly encappedKey: string
  readonly ciphertext: string
}[] {
  if (!Array.isArray(value)) throw new TypeError('identity material envelopes are invalid')
  return value.map(item => {
    const envelope = object(item)
    if (envelope.protocol !== 'anp-sealed-secret/1'
      || envelope.suite !== 'hpke-base-x25519-hkdf-sha256-chacha20poly1305-v1') {
      throw new TypeError('identity material envelope is invalid')
    }
    return {
      protocol: envelope.protocol,
      suite: envelope.suite,
      encappedKey: requiredString(envelope.encappedKey),
      ciphertext: requiredString(envelope.ciphertext),
    }
  })
}

function privateKeyEncoding(value: unknown): 'raw32' | 'pkcs8_der' {
  if (value === 'raw32' || value === 'pkcs8_der') return value
  throw new TypeError('identity private-key encoding is invalid')
}

function requiredString(value: unknown): string {
  if (typeof value !== 'string' || value.length === 0) {
    throw new TypeError('identity provider string value is invalid')
  }
  return value
}

function isFinalDocumentOutcome(value: unknown): boolean {
  const outcome = object(value).outcome
  return outcome === 'committed' || outcome === 'aborted'
}

function incompatible(): ImCoreNodeError {
  return new ImCoreNodeError('provider_incompatible', 'The identity provider is incompatible.', false)
}

function unavailable(): ImCoreNodeError {
  return new ImCoreNodeError(
    'capability_unavailable',
    'The identity provider capability is unavailable.',
    false,
  )
}

function success(value: unknown, buffers: readonly Buffer[] = []): NativeIdentityProviderReply {
  return { ok: true, payloadJson: JSON.stringify(value), buffers }
}

function failure(errorCode: string, retryable: boolean): NativeIdentityProviderReply {
  return { ok: false, payloadJson: 'null', buffers: [], errorCode, retryable }
}

function object(value: unknown): Record<string, unknown> {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    throw new TypeError('identity provider payload must be an object')
  }
  return value as Record<string, unknown>
}

function jsonValue(value: unknown, ancestors = new Set<object>()): ImCoreJsonValue {
  if (value === null || typeof value === 'boolean' || typeof value === 'string') return value
  if (typeof value === 'number') {
    if (Number.isFinite(value)) return value
    throw new TypeError('identity provider JSON value is invalid')
  }
  if (Array.isArray(value)) {
    if (ancestors.has(value)) throw new TypeError('identity provider JSON value is invalid')
    const lengthDescriptor = Object.getOwnPropertyDescriptor(value, 'length')
    if (lengthDescriptor === undefined
      || !('value' in lengthDescriptor)
      || typeof lengthDescriptor.value !== 'number'
      || !Number.isInteger(lengthDescriptor.value)
      || lengthDescriptor.value < 0
      || lengthDescriptor.enumerable
      || lengthDescriptor.configurable) {
      throw new TypeError('identity provider JSON value is invalid')
    }
    const length = lengthDescriptor.value
    if (Reflect.ownKeys(value).length !== length + 1) {
      throw new TypeError('identity provider JSON value is invalid')
    }
    ancestors.add(value)
    const result: ImCoreJsonValue[] = []
    for (let index = 0; index < length; index += 1) {
      const descriptor = Object.getOwnPropertyDescriptor(value, String(index))
      if (descriptor === undefined || !descriptor.enumerable || !('value' in descriptor)) {
        throw new TypeError('identity provider JSON value is invalid')
      }
      result.push(jsonValue(descriptor.value, ancestors))
    }
    ancestors.delete(value)
    return result
  }
  if (typeof value !== 'object'
    || (Object.getPrototypeOf(value) !== Object.prototype && Object.getPrototypeOf(value) !== null)
    || ancestors.has(value)) {
    throw new TypeError('identity provider JSON value is invalid')
  }
  ancestors.add(value)
  const result: { [key: string]: ImCoreJsonValue } = {}
  for (const key of Reflect.ownKeys(value)) {
    const descriptor = Object.getOwnPropertyDescriptor(value, key)
    if (typeof key !== 'string'
      || descriptor === undefined
      || !descriptor.enumerable
      || !('value' in descriptor)) {
      throw new TypeError('identity provider JSON value is invalid')
    }
    Object.defineProperty(result, key, {
      value: jsonValue(descriptor.value, ancestors),
      enumerable: true,
      writable: true,
      configurable: true,
    })
  }
  ancestors.delete(value)
  return result
}

function reference(value: unknown): ImCoreIdentityReference {
  const input = object(value)
  if (
    typeof input.storeId !== 'string'
    || typeof input.identityId !== 'string'
    || typeof input.did !== 'string'
  ) throw new TypeError('identity provider reference is invalid')
  return { storeId: input.storeId, identityId: input.identityId, did: input.did }
}

function signingPurpose(payload: Record<string, unknown>):
  | { purpose: 'authentication'; kid?: string }
  | { purpose: 'device_assertion'; kid?: string }
  | { purpose: 'application_assertion'; domain: string; kid?: string } {
  const selected = typeof payload.kid === 'string' ? { kid: payload.kid } : {}
  if (payload.purpose === 'authentication') return { purpose: 'authentication', ...selected }
  if (payload.purpose === 'device_assertion') return { purpose: 'device_assertion', ...selected }
  if (payload.purpose === 'application_assertion' && typeof payload.domain === 'string') {
    return { purpose: 'application_assertion', domain: payload.domain, ...selected }
  }
  throw new TypeError('identity provider signing purpose is invalid')
}

function exactBuffers(buffers: readonly Buffer[], count: number): void {
  if (buffers.length !== count || buffers.some(buffer => !Buffer.isBuffer(buffer))) {
    throw new TypeError('identity provider binary payload is invalid')
  }
}

function singleBuffer(buffers: readonly Buffer[]): Buffer {
  exactBuffers(buffers, 1)
  const buffer = buffers[0]
  if (buffer === undefined) throw new TypeError('identity provider binary payload is invalid')
  return buffer
}

function pairBuffers(buffers: readonly Buffer[]): readonly [Buffer, Buffer] {
  exactBuffers(buffers, 2)
  const [first, second] = buffers
  if (first === undefined || second === undefined) {
    throw new TypeError('identity provider binary payload is invalid')
  }
  return [first, second]
}

function errorCode(error: unknown): string {
  if (error instanceof TypeError || error instanceof SyntaxError) return 'invalid_request'
  if (typeof error === 'object' && error !== null && 'code' in error && typeof error.code === 'string') {
    return error.code
  }
  return 'provider_unavailable'
}

function errorRetryable(error: unknown): boolean {
  if (typeof error === 'object' && error !== null && 'retryable' in error) {
    return error.retryable === true
  }
  return errorCode(error) === 'provider_unavailable'
}
