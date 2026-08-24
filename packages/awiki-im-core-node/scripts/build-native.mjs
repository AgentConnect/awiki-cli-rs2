import { copyFile, mkdir } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'
import { dirname, join, resolve } from 'node:path'
import { spawnSync } from 'node:child_process'

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const repositoryRoot = resolve(packageRoot, '../..')
const profile = process.env.NODE_ENV === 'production' ? 'release' : 'debug'
const cargoArguments = ['build', '-p', 'awiki-im-core-node']
if (profile === 'release') cargoArguments.push('--release')

const cargo = spawnSync('cargo', cargoArguments, {
  cwd: repositoryRoot,
  stdio: 'inherit',
})
if (cargo.status !== 0) process.exit(cargo.status ?? 1)

const sourceName = process.platform === 'win32'
  ? 'awiki_im_core_node.dll'
  : process.platform === 'darwin'
    ? 'libawiki_im_core_node.dylib'
    : 'libawiki_im_core_node.so'
const glibcVersion = process.platform === 'linux'
  ? process.report.getReport().header.glibcVersionRuntime
  : undefined
const libc = process.platform === 'linux' ? (glibcVersion ? 'gnu' : 'musl') : undefined
const targetName = [process.platform, process.arch, libc].filter(Boolean).join('-')
const nativeDir = join(packageRoot, 'native')
await mkdir(nativeDir, { recursive: true })
await copyFile(
  join(repositoryRoot, 'target', profile, sourceName),
  join(nativeDir, `awiki-im-core-node.${targetName}.node`),
)
