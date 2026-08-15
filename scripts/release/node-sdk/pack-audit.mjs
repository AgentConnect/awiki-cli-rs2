import { createHash } from 'node:crypto'
import { spawnSync } from 'node:child_process'
import { mkdir, readFile, writeFile } from 'node:fs/promises'
import { basename, dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../../..')

function fail(message) {
  throw new Error(message)
}

function argument(name) {
  const index = process.argv.indexOf(`--${name}`)
  if (index === -1 || !process.argv[index + 1]) fail(`missing --${name}`)
  return process.argv[index + 1]
}

async function sha256(path) {
  return createHash('sha256').update(await readFile(path)).digest('hex')
}

const packageDirectory = resolve(repositoryRoot, argument('package-dir'))
const destination = resolve(repositoryRoot, argument('destination'))
await mkdir(destination, { recursive: true })
const result = spawnSync('npm', ['pack', '--json', '--ignore-scripts', '--pack-destination', destination], {
  cwd: packageDirectory,
  encoding: 'utf8',
  maxBuffer: 16 * 1024 * 1024,
})
if (result.status !== 0) fail((result.stderr || result.stdout).trim())
const packed = JSON.parse(result.stdout)[0]
if (!packed?.filename || !Array.isArray(packed.files)) fail('npm pack did not return an auditable file list')

const manifest = JSON.parse(await readFile(join(packageDirectory, 'package.json'), 'utf8'))
if (manifest.license !== 'AGPL-3.0-only') fail('package must declare AGPL-3.0-only')
for (const hook of ['preinstall', 'install', 'postinstall']) {
  if (manifest.scripts?.[hook]) fail(`runtime installation hook is forbidden: ${hook}`)
}
const paths = packed.files.map(file => file.path).sort()
for (const required of [
  'COMMERCIAL-LICENSING.md',
  'LICENSE',
  'NOTICE.md',
  'README.md',
  'SOURCE.md',
  'checksums.json',
  'package.json',
  'provenance.json',
  'sbom.cdx.json',
]) {
  if (!paths.includes(required)) fail(`packed package is missing ${required}`)
}
for (const path of paths) {
  const sourceExtension = path.endsWith('.rs') || (path.endsWith('.ts') && !path.endsWith('.d.ts'))
  if (/(^|\/)(node_modules|src|scripts|test|test-d)(\/|$)/.test(path) || sourceExtension) {
    fail(`source/build-only file leaked into package: ${path}`)
  }
}

const nativeFiles = paths.filter(path => path.endsWith('.node'))
if (manifest.main?.endsWith('.node')) {
  if (nativeFiles.length !== 1 || nativeFiles[0] !== manifest.main.replace(/^\.\//, '')) {
    fail('platform package must contain exactly its declared native addon')
  }
}
else {
  if (nativeFiles.length !== 0) fail('root wrapper must not embed a native addon')
  for (const required of ['dist/index.js', 'dist/index.d.ts']) {
    if (!paths.includes(required)) fail(`wrapper package is missing ${required}`)
  }
  const optionalVersions = Object.values(manifest.optionalDependencies || {})
  if (optionalVersions.length !== 5 || optionalVersions.some(version => version !== manifest.version)) {
    fail('wrapper must pin all five candidate platform packages to its exact version')
  }
}

const provenance = JSON.parse(await readFile(join(packageDirectory, 'provenance.json'), 'utf8'))
if (provenance.package.name !== manifest.name || provenance.package.version !== manifest.version) {
  fail('provenance package identity mismatch')
}
if (provenance.distributionPolicy !== 'temporary-test-artifact-only-license-approval-not-recorded') {
  fail('artifact is missing the non-release distribution gate')
}
const sbom = JSON.parse(await readFile(join(packageDirectory, 'sbom.cdx.json'), 'utf8'))
if (sbom.bomFormat !== 'CycloneDX' || sbom.specVersion !== '1.6') fail('invalid CycloneDX SBOM')
for (const name of Object.keys(manifest.optionalDependencies || {})) {
  if (!sbom.components?.some(component => component.name === name && component.scope === 'optional')) {
    fail(`wrapper SBOM is missing optional platform package ${name}`)
  }
}
const checksums = JSON.parse(await readFile(join(packageDirectory, 'checksums.json'), 'utf8'))
for (const entry of checksums.files) {
  if (await sha256(join(packageDirectory, entry.path)) !== entry.sha256) {
    fail(`checksum mismatch for ${entry.path}`)
  }
}

const tarball = join(destination, packed.filename)
const digest = await sha256(tarball)
await writeFile(`${tarball}.sha256`, `${digest}  ${basename(tarball)}\n`)
process.stdout.write(`${JSON.stringify({ name: manifest.name, tarball, sha256: digest, files: paths })}\n`)
