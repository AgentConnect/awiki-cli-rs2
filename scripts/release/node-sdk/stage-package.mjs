import { createHash } from 'node:crypto'
import { copyFile, cp, mkdir, readFile, readdir, rm, stat, writeFile } from 'node:fs/promises'
import { basename, dirname, isAbsolute, join, relative, resolve, sep } from 'node:path'
import { fileURLToPath } from 'node:url'
import { spawnSyncPortable } from './spawn-command.mjs'

const scriptDir = dirname(fileURLToPath(import.meta.url))
const repositoryRoot = resolve(scriptDir, '../../..')
const stagingRoot = resolve(repositoryRoot, 'dist/node-sdk')

function fail(message) {
  throw new Error(message)
}

function argument(name) {
  const index = process.argv.indexOf(`--${name}`)
  if (index === -1 || !process.argv[index + 1]) fail(`missing --${name}`)
  return process.argv[index + 1]
}

function run(command, args, options = {}) {
  const result = spawnSyncPortable(command, args, {
    cwd: repositoryRoot,
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
    ...options,
  })
  if (result.status !== 0) {
    const detail = result.error?.message || result.stderr || result.stdout || `exit status ${result.status}`
    fail(`${command} ${args.join(' ')} failed: ${String(detail).trim()}`)
  }
  return result.stdout.trim()
}

async function json(path) {
  return JSON.parse(await readFile(path, 'utf8'))
}

async function sha256(path) {
  const digest = createHash('sha256')
  digest.update(await readFile(path))
  return digest.digest('hex')
}

function packageVersion(path) {
  const source = run('git', ['show', `HEAD:${relative(repositoryRoot, path).replaceAll(sep, '/')}`])
  const match = source.match(/^version\s*=\s*"([^"]+)"/m)
  if (!match) fail(`version is missing from ${path}`)
  return match[1]
}

function workspaceDependencyVersion(name) {
  const source = run('git', ['show', 'HEAD:Cargo.toml'])
  const match = source.match(new RegExp(`^${name}\\s*=\\s*\\{[^\\n]*version\\s*=\\s*"([^"]+)"`, 'm'))
  if (!match) fail(`workspace dependency version is missing for ${name}`)
  return match[1]
}

function sourceRevision() {
  const requested = process.env.AWIKI_NODE_SDK_SOURCE_SHA?.trim()
  const head = run('git', ['rev-parse', 'HEAD'])
  const commit = requested || head
  if (!/^[a-f0-9]{40}$/i.test(commit)) fail('source revision must be a full Git commit SHA')
  if (commit !== head) fail('source revision must match the checked-out commit')
  return {
    commit,
    committedAt: run('git', ['show', '-s', '--format=%cI', commit]),
    dirty: run('git', ['status', '--short']).length > 0,
  }
}

function cargoSbom(manifest) {
  const metadata = JSON.parse(run('cargo', ['metadata', '--format-version', '1', '--locked']))
  const cargoComponents = metadata.packages
    .map(pkg => ({
      type: 'library',
      name: pkg.name,
      version: pkg.version,
      ...(pkg.license ? { licenses: [{ expression: pkg.license }] } : {}),
      ...(pkg.source?.startsWith('registry+')
        ? { purl: `pkg:cargo/${encodeURIComponent(pkg.name)}@${encodeURIComponent(pkg.version)}` }
        : {}),
    }))
  const npmComponents = Object.entries(manifest.optionalDependencies || {}).map(([name, value]) => {
    const version = value.replace(/^workspace:/, '')
    return {
      type: 'library',
      name,
      version,
      purl: `pkg:npm/${encodeURIComponent(name)}@${encodeURIComponent(version)}`,
      scope: 'optional',
    }
  })
  const components = [...cargoComponents, ...npmComponents]
    .sort((left, right) => `${left.name}@${left.version}`.localeCompare(`${right.name}@${right.version}`))
  return {
    bomFormat: 'CycloneDX',
    specVersion: '1.6',
    version: 1,
    metadata: {
      component: {
        type: 'library',
        name: manifest.name,
        version: manifest.version,
      },
    },
    components,
  }
}

async function writeCommonFiles(output, manifest, target, binary) {
  const source = sourceRevision()
  const binaryDigest = binary ? await sha256(binary) : undefined
  const releaseConfig = await json(join(repositoryRoot, 'scripts/release/cli/release-config.json'))
  const imCoreVersion = packageVersion(join(repositoryRoot, 'crates/im-core/Cargo.toml'))
  const nativeBridgeVersion = packageVersion(join(repositoryRoot, 'crates/im-core-node/Cargo.toml'))
  const provenance = {
    schemaVersion: 1,
    package: { name: manifest.name, version: manifest.version },
    target: target || 'platform-independent-wrapper',
    nativeApiVersion: 3,
    source: {
      repository: 'https://github.com/AgentConnect/awiki-cli-rs2',
      commit: source.commit,
      committedAt: source.committedAt,
      dirty: source.dirty,
    },
    sdk: {
      imCoreVersion,
      nativeBridgeVersion,
      anpRustVersion: workspaceDependencyVersion('anp'),
      anpCommit: releaseConfig.anp_commit,
    },
    toolchain: {
      rustc: run('rustc', ['--version']),
      node: process.version,
      pnpm: run('pnpm', ['--version']),
    },
    ...(binaryDigest ? { binarySha256: binaryDigest } : {}),
    distributionPolicy: 'agpl-3.0-only-approved-test-channel',
  }
  const sourceText = `# Corresponding Source\n\nPackage: ${manifest.name}@${manifest.version}\nTarget: ${target || 'platform-independent-wrapper'}\nRepository: https://github.com/AgentConnect/awiki-cli-rs2\nCommit: ${source.commit}\nANP commit: ${releaseConfig.anp_commit}\n\nBuild instructions: docs/node-sdk/awiki-im-core-node-artifacts.md\n`
  const notice = `# Notices\n\nThis package is AWiki CLI S2 software distributed under AGPL-3.0-only. Corresponding source and build provenance are identified in SOURCE.md and provenance.json. Third-party components and their declared licenses are enumerated in sbom.cdx.json. The verified GitHub Actions artifact is the approved test channel; npm publication is a separate release action.\n`

  await copyFile(join(repositoryRoot, 'LICENSE'), join(output, 'LICENSE'))
  await copyFile(join(repositoryRoot, 'COMMERCIAL-LICENSING.md'), join(output, 'COMMERCIAL-LICENSING.md'))
  await writeFile(join(output, 'NOTICE.md'), notice)
  await writeFile(join(output, 'SOURCE.md'), sourceText)
  await writeFile(join(output, 'provenance.json'), `${JSON.stringify(provenance, null, 2)}\n`)
  await writeFile(
    join(output, 'sbom.cdx.json'),
    `${JSON.stringify(cargoSbom(manifest), null, 2)}\n`,
  )
}

async function writeChecksums(output) {
  const entries = []
  async function visit(directory) {
    const children = await readdir(directory, { withFileTypes: true })
    for (const child of children.sort((left, right) => left.name.localeCompare(right.name))) {
      const path = join(directory, child.name)
      if (child.isDirectory()) await visit(path)
      else if (child.isFile() && child.name !== 'checksums.json') {
        entries.push({ path: relative(output, path).replaceAll(sep, '/'), sha256: await sha256(path) })
      }
    }
  }
  await visit(output)
  await writeFile(join(output, 'checksums.json'), `${JSON.stringify({ schemaVersion: 1, files: entries }, null, 2)}\n`)
}

function assertStagingPath(output) {
  const prefix = `${stagingRoot}${sep}`
  if (!isAbsolute(output) || !output.startsWith(prefix)) {
    fail(`output must be a child of ${stagingRoot}`)
  }
}

async function stage() {
  const kind = argument('kind')
  const packageDirectory = resolve(repositoryRoot, argument('package-dir'))
  const output = resolve(repositoryRoot, argument('output'))
  assertStagingPath(output)
  if (!['platform', 'wrapper'].includes(kind)) fail('--kind must be platform or wrapper')

  const manifest = await json(join(packageDirectory, 'package.json'))
  await rm(output, { recursive: true, force: true })
  await mkdir(output, { recursive: true })
  const stagedManifest = structuredClone(manifest)
  if (kind === 'wrapper') {
    for (const [name, version] of Object.entries(stagedManifest.optionalDependencies || {})) {
      if (typeof version !== 'string' || !version.startsWith('workspace:')) {
        fail(`wrapper optional dependency ${name} must use an exact workspace version`)
      }
      stagedManifest.optionalDependencies[name] = version.slice('workspace:'.length)
    }
  }
  await writeFile(join(output, 'package.json'), `${JSON.stringify(stagedManifest, null, 2)}\n`)
  await copyFile(join(packageDirectory, 'README.md'), join(output, 'README.md'))
  try {
    await copyFile(join(packageDirectory, 'CHANGELOG.md'), join(output, 'CHANGELOG.md'))
  }
  catch (error) {
    if (error?.code !== 'ENOENT') throw error
  }

  let target
  let binary
  if (kind === 'platform') {
    target = argument('target')
    binary = resolve(repositoryRoot, argument('binary'))
    if ((await stat(binary)).size === 0) fail('native addon is empty')
    if (basename(manifest.main) !== `awiki-im-core-node.${target}.node`) {
      fail(`platform package main does not match ${target}`)
    }
    await copyFile(binary, join(output, basename(manifest.main)))
  }
  else {
    const dist = join(packageDirectory, 'dist')
    await cp(dist, join(output, 'dist'), { recursive: true })
  }

  await writeCommonFiles(output, manifest, target, binary)
  await writeChecksums(output)
  process.stdout.write(`${output}\n`)
}

await stage()
