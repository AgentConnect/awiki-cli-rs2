import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import path from 'node:path'
import test from 'node:test'

const repositoryRoot = fileURLToPath(new URL('../../../', import.meta.url))
const workflowPath = path.join(
  repositoryRoot,
  '.github/workflows/im-core-node-artifacts.yml',
)
const packagePaths = [
  'packages/awiki-im-core-node/package.json',
  'packages/awiki-im-core-node-platforms/darwin-arm64/package.json',
  'packages/awiki-im-core-node-platforms/darwin-x64/package.json',
  'packages/awiki-im-core-node-platforms/linux-arm64-gnu/package.json',
  'packages/awiki-im-core-node-platforms/linux-x64-gnu/package.json',
  'packages/awiki-im-core-node-platforms/win32-x64-msvc/package.json',
]
const expectedRepository = 'https://github.com/AgentConnect/awiki-cli-rs2'

test('trusted publishing is gated to the release branch and verified artifacts', () => {
  const workflow = readFileSync(workflowPath, 'utf8')

  assert.match(workflow, /publish_to_npm:/)
  assert.match(workflow, /inputs\.publish_to_npm == true/)
  assert.match(workflow, /github\.ref == 'refs\/heads\/release\/0714-dsh'/)
  assert.match(workflow, /publish-npm:[\s\S]*?needs: publish-test-channel/)
  assert.match(workflow, /publish-npm:[\s\S]*?id-token: write/)
  assert.match(workflow, /npm install --global npm@11\.19\.0/)
  assert.doesNotMatch(workflow, /publish-npm:[\s\S]*?NODE_AUTH_TOKEN/)
})

test('trusted publishing keeps all platform packages ahead of the root wrapper', () => {
  const workflow = readFileSync(workflowPath, 'utf8')
  const publishCommands = [...workflow.matchAll(
    /^\s+npm publish "channel\/([^"]+)" --access public$/gm,
  )].map((match) => match[1])

  assert.deepEqual(publishCommands, [
    'awiki-im-core-node-darwin-arm64-${version}.tgz',
    'awiki-im-core-node-darwin-x64-${version}.tgz',
    'awiki-im-core-node-linux-arm64-gnu-${version}.tgz',
    'awiki-im-core-node-linux-x64-gnu-${version}.tgz',
    'awiki-im-core-node-win32-x64-msvc-${version}.tgz',
    'awiki-im-core-node-${version}.tgz',
  ])
})

test('every published package declares the trusted GitHub repository', () => {
  for (const relativePath of packagePaths) {
    const manifest = JSON.parse(
      readFileSync(path.join(repositoryRoot, relativePath), 'utf8'),
    )
    assert.equal(manifest.repository, expectedRepository, manifest.name)
  }
})
