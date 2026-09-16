// Gemini 0.59.0 returns session/load before its asynchronous history replay.
// Apply the missing await only while loading that exact npm package/version.
// Installed files, native history, and the ACP transport remain untouched.
import { readFile } from 'node:fs/promises';

export async function load(url, context, nextLoad) {
  const loaded = await nextLoad(url, context);
  if (!/^file:.*\/@google\/gemini-cli\/bundle\/gemini-[^/]+\.js$/.test(url) || loaded.format !== 'module') {
    return loaded;
  }
  const manifest = JSON.parse(await readFile(new URL('../package.json', url), 'utf8'));
  if (manifest.name !== '@google/gemini-cli' || manifest.version !== '0.59.0') {
    return loaded;
  }
  const source = typeof loaded.source === 'string'
    ? loaded.source : new TextDecoder().decode(loaded.source);
  if (!source.includes('async loadSession(')) return loaded;
  const call = '    session.streamHistory(sessionData.messages);';
  const awaited = '    await session.streamHistory(sessionData.messages);';
  if (source.includes(awaited) && !source.includes(call)) return loaded;
  if (source.split(call).length !== 2) {
    throw new Error('awiki_gemini_replay_compatibility_mismatch');
  }
  return { ...loaded, source: source.replace(call, awaited) };
}
