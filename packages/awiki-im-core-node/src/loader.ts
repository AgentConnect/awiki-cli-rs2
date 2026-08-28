import { createRequire } from 'node:module'
import { existsSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import type { NativeBinding } from './native.js'
import { currentNativeTarget, nativePlatformPackages } from './targets.js'
import { ImCoreNodeError } from './types.js'

const require = createRequire(import.meta.url)
const packageRoot = join(dirname(fileURLToPath(import.meta.url)), '..')

function target(): { readonly key: string, readonly packageName: string } {
  const key = currentNativeTarget()
  const packageName = nativePlatformPackages[key]
  if (!packageName) {
    throw new ImCoreNodeError('unsupported_platform', `Unsupported native platform: ${key}.`, false)
  }
  return { key, packageName }
}

function isModuleMissing(error: unknown): boolean {
  return error instanceof Error
    && 'code' in error
    && error.code === 'MODULE_NOT_FOUND'
}

/** Load only a bundled or installed platform addon; runtime downloads and path overrides are forbidden. */
export function loadNativeBinding(): NativeBinding {
  const { key, packageName } = target()
  let binding: NativeBinding | undefined
  try {
    binding = require(packageName) as NativeBinding
  }
  catch (error) {
    if (!isModuleMissing(error)) throw error
  }
  if (!binding) {
    const local = join(packageRoot, 'native', `awiki-im-core-node.${key}.node`)
    if (existsSync(local)) binding = require(local) as NativeBinding
  }
  if (!binding) {
    throw new ImCoreNodeError(
      'native_addon_missing',
      `The native addon for ${key} is not installed.`,
      false,
    )
  }
  if (binding.nativeApiVersion() !== 10) {
    throw new ImCoreNodeError('native_api_mismatch', 'The native addon API version is incompatible.', false)
  }
  return binding
}
