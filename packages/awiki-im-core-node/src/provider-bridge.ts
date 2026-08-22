import type {
  NativeIdentityProviderCall,
  NativeIdentityProviderDispatch,
  NativeIdentityProviderReply,
} from './native.js'
import {
  ImCoreNodeError,
  type ImCoreIdentityProvider,
  type ImCoreIdentityReference,
} from './types.js'

const IDENTITY_PROVIDER_PROTOCOL = 'anp-identity-provider-ts/1'
const REQUIRED_CAPABILITIES = [
  'IDENTITY_READ',
  'IDENTITY_SIGN',
  'IDENTITY_HTTP_SIGNATURE',
] as const

/** @internal Host-only bridge; this file is not exported by the package. */
export function createIdentityProviderDispatch(
  provider: ImCoreIdentityProvider,
): NativeIdentityProviderDispatch {
  if (provider.protocol !== IDENTITY_PROVIDER_PROTOCOL) throw incompatible()
  const capabilities = new Set(provider.capabilities)
  if (REQUIRED_CAPABILITIES.some(capability => !capabilities.has(capability))) throw incompatible()
  for (const method of [
    'info', 'recover', 'list', 'publicIdentity', 'recoverIdentity',
    'sign', 'signOriginProof', 'prepareHttpSignature',
  ] as const) {
    if (typeof provider[method] !== 'function') throw incompatible()
  }

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
        default: return failure('provider_incompatible', false)
      }
    }
    catch (error) {
      return failure(errorCode(error), errorRetryable(error))
    }
  }
}

function incompatible(): ImCoreNodeError {
  return new ImCoreNodeError('provider_incompatible', 'The identity provider is incompatible.', false)
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
