/** Convert a pinned Cargo requirement into the version recorded in provenance. */
export function exactDependencyVersion(requirement, { allowPrerelease = false } = {}) {
  const pattern = allowPrerelease
    ? /^=?(\d+\.\d+\.\d+(?:-(?:alpha|beta|rc)\.\d+)?)$/
    : /^=?(\d+\.\d+\.\d+)$/
  const match = pattern.exec(requirement)
  if (!match) throw new Error('release provenance requires an exact stable SDK version')
  return match[1]
}
