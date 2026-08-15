import { spawnSync } from 'node:child_process'

export function spawnSyncPortable(command, args, options = {}) {
  if (process.platform !== 'win32' || !['npm', 'pnpm'].includes(command)) {
    return spawnSync(command, args, options)
  }

  const argumentEnvironment = Object.fromEntries(
    args.map((argument, index) => [`AWIKI_NODE_SDK_ARG_${index}`, argument]),
  )
  const commandLine = [
    `${command}.cmd`,
    ...args.map((_, index) => `"%AWIKI_NODE_SDK_ARG_${index}%"`),
  ].join(' ')
  return spawnSync(process.env.ComSpec || 'cmd.exe', ['/d', '/s', '/c', commandLine], {
    ...options,
    env: { ...process.env, ...options.env, ...argumentEnvironment },
  })
}
