// Version- and source-bound compatibility for official Gemini npm bundles.
// Installed files and pre-existing native history remain untouched.
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';

// Gemini 0.60 records tool metadata cumulatively, sometimes on an earlier model
// message. Real user functionResponse records define the original round. Recover
// only when exact IDs, names and boundaries prove where each call belongs.
// This function is injected into the core chunk, so it has no outer dependencies.
export function awikiNormalizeGeminiHistory(messages) {
  const fail = () => { throw new Error('awiki_gemini_history_unrecoverable'); };
  const parts = value => value == null || value === '' ? []
    : (Array.isArray(value) ? value : [value]).map(p => typeof p === 'string' ? { text: p } : p);
  const key = call => {
    if (!call || typeof call.id !== 'string' || !call.id || typeof call.name !== 'string' || !call.name) fail();
    return call.id;
  };
  const canonical = value => JSON.stringify(value, function (_, v) {
    return v && typeof v === 'object' && !Array.isArray(v)
      ? Object.fromEntries(Object.keys(v).sort().map(k => [k, v[k]])) : v;
  });
  const records = structuredClone(messages);
  const calls = new Map();
  const responses = new Map();
  let lastUser = -1;
  const userBoundary = [];
  for (let i = 0; i < records.length; i++) {
    const msg = records[i];
    const content = parts(msg.content);
    // A new human prompt is an absolute boundary. Pure tool results and their
    // accompanying image/text parts stay in the tool response round.
    if (msg.type === 'user' && !content.some(p => p.functionResponse)) lastUser = i;
    userBoundary[i] = lastUser;
    if (msg.type === 'gemini') {
      for (const part of content.filter(p => p.functionCall)) {
        const call = part.functionCall;
        const id = key(call);
        if (calls.get(id)?.part) fail();
        const previous = calls.get(id);
        if (previous && (previous.call.name !== call.name || canonical(previous.call.args ?? {}) !== canonical(call.args ?? {}))) fail();
        calls.set(id, { ...previous, call, part, index: i });
      }
      for (const tool of msg.toolCalls ?? []) {
        const id = key(tool);
        const previous = calls.get(id);
        if (previous && (previous.call.name !== tool.name || canonical(previous.call.args ?? {}) !== canonical(tool.args ?? {}))) fail();
        if (!previous) calls.set(id, { call: { id, name: tool.name, args: tool.args ?? {} }, index: i });
      }
    } else if (msg.type === 'user') {
      for (const part of content.filter(p => p.functionResponse)) {
        const id = key(part.functionResponse);
        const entry = responses.get(id) ?? [];
        entry.push({ index: i, response: part.functionResponse });
        responses.set(id, entry);
      }
    }
  }
  for (const [id, entries] of responses) {
    const known = calls.get(id);
    if (!known || entries.some(e => e.response.name !== known.call.name)) fail();
    const first = entries[0].index;
    if (known.index >= first || userBoundary[known.index] !== userBoundary[first]) fail();
    let target = first - 1;
    while (target >= 0 && !['gemini', 'user'].includes(records[target].type)) target--;
    // Multiple response records in one uninterrupted tool round are valid.
    while (target >= 0 && records[target].type === 'user' && parts(records[target].content).some(p => p.functionResponse)) target--;
    if (target < 0 || records[target].type !== 'gemini') fail();
    if (known.part) {
      // Never relocate an explicitly recorded native call or its signature.
      if (known.index > target) fail();
      for (let j = known.index + 1; j <= target; j++) if (records[j].type === 'user') fail();
    } else {
      if (known.index > target) fail();
      records[target].content = [...parts(records[target].content), { functionCall: known.call }];
      known.part = true;
    }
    for (let j = first + 1; j <= entries.at(-1).index; j++) {
      if (records[j].type === 'gemini' || (records[j].type === 'user' && !parts(records[j].content).some(p => p.functionResponse))) fail();
    }
  }
  for (const [id, known] of calls) {
    if (!responses.has(id)) {
      const tool = records[known.index].toolCalls?.find(t => t.id === id);
      if (tool?.result == null) fail();
      if (typeof tool.result !== 'string') {
        const saved = parts(tool.result).filter(p => p.functionResponse);
        if (!saved.length || saved.some(p => key(p.functionResponse) !== id || p.functionResponse.name !== known.call.name)) fail();
      }
    }
    if (!known.part) {
      // Older recordings may have only metadata and its recorded result. Keep
      // that native fallback, but never fabricate a missing or failed result.
      const record = records[known.index];
      const tool = record.toolCalls?.find(t => t.id === id);
      if (tool?.result == null) fail();
      record.content = [...parts(record.content), { functionCall: known.call }];
    }
  }
  for (const record of records) {
    if (record.type !== 'gemini' || !record.toolCalls) continue;
    // The explicit user record is the result already saved by Gemini. Avoid
    // synthesizing a second copy from display metadata; do not drop real turns.
    record.toolCalls = record.toolCalls.filter(t => !responses.has(t.id));
  }
  return records;
}

const converterStart = 'function convertSessionToClientHistory(messages) {';
const converter060Hash = '0bd784d44e28dae4f606fcb5ac038a15a0ce660c7cc882d1e2b57dcdf64d8625';
const recording060 = `      id = this.chatRecordingService.recordMessage({
        model,
        type: "gemini",
        content: responseText
      });`;

export async function load(url, context, nextLoad) {
  const loaded = await nextLoad(url, context);
  if (!/^file:.*\/@google\/gemini-cli\/bundle\/[^/]+\.js$/.test(url) || loaded.format !== 'module') {
    return loaded;
  }
  const manifest = JSON.parse(await readFile(new URL('../package.json', url), 'utf8'));
  if (manifest.name !== '@google/gemini-cli' || !['0.59.0', '0.60.0'].includes(manifest.version)) {
    return loaded;
  }
  let source = typeof loaded.source === 'string'
    ? loaded.source : new TextDecoder().decode(loaded.source);
  if (/\/gemini-[^/]+\.js$/.test(url) && source.includes('async loadSession(')) {
    const call = '    session.streamHistory(sessionData.messages);';
    const awaited = '    await session.streamHistory(sessionData.messages);';
    if (!(source.includes(awaited) && !source.includes(call))) {
      if (source.split(call).length !== 2) throw new Error('awiki_gemini_replay_compatibility_mismatch');
      source = source.replace(call, awaited);
    }
  }
  if (manifest.version === '0.60.0' && source.includes(converterStart)) {
    const start = source.indexOf(converterStart);
    const end = source.indexOf('\n// ', start);
    if (source.split(converterStart).length !== 2 || end < start
      || createHash('sha256').update(source.slice(start, end)).digest('hex') !== converter060Hash
      || source.split(recording060).length !== 2) throw new Error('awiki_gemini_history_compatibility_mismatch');
    source = source.replace(converterStart, `${awikiNormalizeGeminiHistory.toString()}\n${converterStart}\n  messages = awikiNormalizeGeminiHistory(messages);`)
      // Save the original model parts for future resumes: exact IDs, round,
      // images and thought signatures. Existing recording files are not edited.
      .replace(recording060, recording060.replace('content: responseText', 'content: consolidatedParts'));
  }
  return { ...loaded, source };
}
