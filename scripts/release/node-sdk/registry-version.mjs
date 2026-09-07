/** Convert a pinned Cargo requirement into the version recorded in provenance. */
export function exactDependencyVersion(requirement) {
  const match = /^=?(\d+\.\d+\.\d+)$/.exec(requirement)
  if (!match) throw new Error('release provenance requires an exact stable SDK version')
  return match[1]
}
