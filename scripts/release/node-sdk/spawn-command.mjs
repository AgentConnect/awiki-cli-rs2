import { spawnSync } from 'node:child_process'

export function spawnSyncPortable(command, args, options = {}) {
  if (process.platform !== 'win32' || !['npm', 'pnpm'].includes(command)) {
    return spawnSync(command, args, options)
  }

  return spawnSync(
    process.env.ComSpec || 'cmd.exe',
    ['/d', '/c', `${command}.cmd`, ...args],
    options,
  )
}
