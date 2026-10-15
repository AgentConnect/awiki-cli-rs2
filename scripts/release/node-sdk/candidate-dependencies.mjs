// A local Core candidate may consume fixed registry ANP/Identity inputs.
export function candidateDependencies(metadata, versions, inspect) {
  return ['anp', 'anp-identity'].map(name => {
    const packages = metadata.packages.filter(pkg => pkg.name === name)
    if (packages.length !== 1 || packages[0].version !== versions[name]) throw new Error(`candidate must resolve exactly pinned ${name}`)
    const pkg = packages[0]
    if (pkg.source !== null && pkg.source !== 'registry+https://github.com/rust-lang/crates.io-index') throw new Error(`unapproved candidate source for ${name}`)
    return { name, version: pkg.version, source: pkg.source ?? 'local', ...inspect(pkg) }
  })
}
