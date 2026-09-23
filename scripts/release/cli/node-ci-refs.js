'use strict';
const { readFileSync } = require('node:fs');
function selectRefs(sourceMode, release, source) {
  if (!sourceMode) return { anp_repository: release.anp_repository, anp_commit: release.anp_commit, anp_identity_commit: release.anp_identity_commit };
  const anp = source.dependencies.anp;
  const identity = source.dependencies['anp-identity'];
  if (anp.repository !== 'https://github.com/agent-network-protocol/anp.git' ||
      identity.repository !== 'https://github.com/agent-network-protocol/anp-identity.git' ||
      !/^[a-f0-9]{40}$/.test(anp.commit) || !/^[a-f0-9]{40}$/.test(identity.commit)) {
    throw new Error('Invalid fixed source dependency refs');
  }
  return { anp_repository: 'agent-network-protocol/anp', anp_commit: anp.commit, anp_identity_commit: identity.commit };
}
module.exports = { selectRefs };
if (require.main === module) {
  const read = p => JSON.parse(readFileSync(p, 'utf8'));
  const sourceMode = process.env.AWIKI_NODE_SOURCE_REFS === '1';
  const refs = selectRefs(sourceMode, read('scripts/release/cli/release-config.json'), sourceMode ? read('dependencies.source.json') : undefined);
  for (const [key, value] of Object.entries(refs)) console.log(`${key}=${value}`);
}
