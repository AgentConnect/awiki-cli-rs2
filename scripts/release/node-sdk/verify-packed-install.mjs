import { createHash } from 'node:crypto'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { spawnSyncPortable } from './spawn-command.mjs'

function fail(message) {
  throw new Error(message)
}

function argument(name) {
  const index = process.argv.indexOf(`--${name}`)
  if (index === -1 || !process.argv[index + 1]) fail(`missing --${name}`)
  return resolve(process.cwd(), process.argv[index + 1])
}

function run(command, args, options) {
  const result = spawnSyncPortable(command, args, { encoding: 'utf8', timeout: 60_000, ...options })
  if (result.status !== 0) {
    const detail = result.error?.message || result.stderr || result.stdout || `exit status ${result.status}`
    fail(`${command} failed: ${String(detail).trim()}`)
  }
  return result.stdout.trim()
}

async function verifyTarball(path) {
  const expected = (await readFile(`${path}.sha256`, 'utf8')).trim().split(/\s+/)[0]
  const actual = createHash('sha256').update(await readFile(path)).digest('hex')
  if (actual !== expected) fail(`tarball checksum mismatch: ${path}`)
}

const wrapper = argument('wrapper')
const expectMissing = process.argv.includes('--expect-missing')
const platform = expectMissing ? undefined : argument('platform')
const workspace = await mkdtemp(join(tmpdir(), 'awiki-im-core-node-packed-'))
try {
  await verifyTarball(wrapper)
  if (platform) await verifyTarball(platform)
  run('npm', [
    'install', '--offline', '--ignore-scripts', '--no-audit', '--no-fund', '--package-lock=false',
    wrapper, ...(platform ? [platform] : []),
  ], {
    cwd: workspace,
    env: { ...process.env, CARGO: 'unavailable', RUSTC: 'unavailable' },
  })
  const stateRoot = join(workspace, 'state')
  const operation = expectMissing
    ? `
      try {
        await openImCoreNodeClient(options)
        throw new Error('expected native_addon_missing')
      }
      catch (error) {
        if (error?.code !== 'native_addon_missing') throw error
      }
      console.log('packed-missing-ok')
    `
    : `
      const client = await openImCoreNodeClient(options)
      if (typeof client.getLocalConversationTimeline !== 'function') {
        throw new Error('expected native API v4 local timeline facade')
      }
      if (typeof client.prepareExternalHttpRequest !== 'function') {
        throw new Error('expected native API v5 external HTTP auth facade')
      }
      if (typeof client.createGroup !== 'function' || typeof client.hydrateDisplayProfiles !== 'function') {
        throw new Error('expected native API v5 group and display-profile facade')
      }
      if (typeof client.getLocalConversationTimeline !== 'function') {
        throw new Error('expected native API v5 local timeline facade')
      }
      if (typeof client.startRealtime !== 'function' || typeof client.listMailInbox !== 'function') {
        throw new Error('expected native API v5 realtime and mail facades')
      }
      if (typeof client.completeRegistrationWithOutcome !== 'function'
        || typeof client.beginPreparedRegistrationJoin !== 'function'
        || typeof client.resumePreparedRegistrationJoin !== 'function'
        || typeof client.issueHandleRecoveryAttestation !== 'function') {
        throw new Error('expected native API v9 recovery facade')
      }
      if (typeof client.downloadMailAttachment !== 'function') {
        throw new Error('expected native API v10 mail attachment facade')
      }
      if (await client.getDefaultIdentity() !== null) throw new Error('expected an empty fixture')
      const cleared = await client.clearLocalData()
      if (cleared?.cleared !== true) throw new Error('expected initialized Rust state to be cleared')
      if (await client.getDefaultIdentity() !== null) throw new Error('expected a usable empty client after clear')
      await client.close()
      console.log('packed-smoke-ok')
    `
  const output = run(process.execPath, ['--input-type=module', '-e', `
    import { openImCoreNodeClient } from '@awiki/im-core-node'
    const options = {
      stateRoot: ${JSON.stringify(stateRoot)},
      serviceBaseUrl: 'https://example.test',
      didDomain: 'example.test',
      operationTimeoutMs: 1000,
      syncTimeoutMs: 100,
    }
    ${operation}
  `], { cwd: workspace })
  const expectedOutput = expectMissing ? 'packed-missing-ok' : 'packed-smoke-ok'
  if (output !== expectedOutput) fail(`unexpected smoke output: ${output}`)
  const packageManifest = JSON.parse(await readFile(
    join(workspace, 'node_modules/@awiki/im-core-node/package.json'),
    'utf8',
  ))
  if (packageManifest.scripts?.postinstall) fail('installed wrapper must not have postinstall')
  process.stdout.write(`${output}\n`)
}
finally {
  await rm(workspace, { recursive: true, force: true })
}
