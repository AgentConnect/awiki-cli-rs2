/** Approved first-release Tier 1 native packages. */
export const nativePlatformPackages: Readonly<Record<string, string>> = {
  'linux-x64-gnu': '@awiki/im-core-node-linux-x64-gnu',
  'linux-arm64-gnu': '@awiki/im-core-node-linux-arm64-gnu',
  'darwin-x64': '@awiki/im-core-node-darwin-x64',
  'darwin-arm64': '@awiki/im-core-node-darwin-arm64',
  'win32-x64': '@awiki/im-core-node-win32-x64-msvc',
}

/** Resolve the native target without treating a musl host as glibc. */
export function resolveNativeTarget(
  platform: NodeJS.Platform,
  arch: string,
  glibcVersionRuntime?: string,
): string {
  if (platform === 'linux') {
    return `linux-${arch}-${glibcVersionRuntime ? 'gnu' : 'musl'}`
  }
  return `${platform}-${arch}`
}

export function currentNativeTarget(): string {
  const report = process.platform === 'linux'
    ? process.report.getReport() as { readonly header?: { readonly glibcVersionRuntime?: unknown } }
    : undefined
  const glibcVersionRuntime = report?.header?.glibcVersionRuntime
  return resolveNativeTarget(
    process.platform,
    process.arch,
    typeof glibcVersionRuntime === 'string' ? glibcVersionRuntime : undefined,
  )
}
