import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFile, writeFile, mkdir, mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { pathToFileURL } from 'node:url';
import vm from 'node:vm';
import { awikiNormalizeGeminiHistory, load } from '../src/acp/gemini_replay.mjs';

const source = await readFile(new URL('./fixtures/gemini_060_converter.js', import.meta.url), 'utf8');
const fixture = JSON.parse(await readFile(new URL('./fixtures/gemini_060_history.json', import.meta.url), 'utf8'));
const evaluate = text => vm.runInNewContext(
  text.replace('export { convertSessionToClientHistory };', '').replace('export function recording', 'function recording')
    + '\n({convert:convertSessionToClientHistory, recording})', { structuredClone },
);
const plain = value => JSON.parse(JSON.stringify(value));
const call = (id, name = 'shell', args = {}) => ({ functionCall: { id, name, args } });
const response = (id, name = 'shell', value = id) => ({ functionResponse: { id, name, response: { output: value } } });
const model = (id, content, toolCalls = []) => ({ id, type: 'gemini', content, toolCalls });
const user = (id, content) => ({ id, type: 'user', content });
const metadata = (id, result = [response(id)]) => ({ id, name: 'shell', args: {}, result });
function requirePaired(history) {
  let outstanding = new Set();
  const ids = [];
  for (const { content } of history) {
    if (content.role === 'model') {
      assert.equal(outstanding.size, 0, 'No tools may cross a model round');
      for (const part of content.parts) {
        if (part.functionCall) {
          assert.ok(!ids.includes(part.functionCall.id), 'Call IDs stay unique');
          ids.push(part.functionCall.id);
          outstanding.add(part.functionCall.id);
        }
      }
    } else {
      for (const part of content.parts) if (part.functionResponse) {
        assert.ok(outstanding.delete(part.functionResponse.id), 'Exactly one matching result');
      }
    }
  }
  assert.equal(outstanding.size, 0);
  return ids;
}

test('0.60 runtime hook repairs the recorded second/third turn without modifying files', async () => {
  const root = await mkdtemp(`${tmpdir()}/awiki-gemini-history-`);
  try {
    const pkg = `${root}/@google/gemini-cli`;
    await mkdir(`${pkg}/bundle`, { recursive: true });
    const url = pathToFileURL(`${pkg}/bundle/chunk-fixture.js`).href;
    await writeFile(`${pkg}/package.json`, JSON.stringify({ name: '@google/gemini-cli', version: '0.60.0' }));
    await writeFile(new URL(url), source);
    const next = async () => ({ format: 'module', source });
    const patched = await load(url, {}, next);
    const original = evaluate(source).convert(fixture.messages);
    assert.throws(() => requirePaired(original));
    const { convert, recording } = evaluate(patched.source);
    const before = JSON.stringify(fixture);
    const history = plain(convert(fixture.messages));
    assert.equal(requirePaired(history).length, 4);
    assert.deepEqual(history.filter(t => t.content.parts.some(p => p.functionCall)).map(t => t.content.parts.filter(p => p.functionCall).length), [3, 1]);
    assert.equal(JSON.stringify(fixture), before);
    assert.equal(await readFile(new URL(url), 'utf8'), source);
    const recorded = [];
    const originalParts = [{ text: 'second round' }, { ...call('third'), thoughtSignature: 'native-signature' }];
    recording.call({chatRecordingService:{ recordMessage: m => { recorded.push(m); return 'next'; } }}, 'model', 'second round', originalParts);
    assert.deepEqual(plain(recorded[0].content), originalParts);
    assert.equal(requirePaired(plain(convert([
      ...fixture.messages, user('next-user', 'third prompt'),
      model('next', recorded[0].content, [metadata('third')]), user('next-result', [response('third')]),
      model('end', 'done'),
    ]))).length, 5);
    await assert.rejects(load(url, {}, async () => ({ format: 'module', source: source.replace('const clientHistory = [];', 'const clientHistory = new Array();') })), /compatibility_mismatch/);
    await writeFile(`${pkg}/package.json`, JSON.stringify({ name: '@google/gemini-cli', version: '0.61.0' }));
    assert.equal((await load(url, {}, next)).source, source);
    await writeFile(`${pkg}/package.json`, JSON.stringify({ name: 'unrelated', version: '0.60.0' }));
    assert.equal((await load(url, {}, next)).source, source);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('same-name parallel tools keep exact IDs, order, signatures and attachments', () => {
  const messages = [
    user('u', [{ text: 'prompt' }, { inlineData: { mimeType: 'image/png', data: 'exact' } }]),
    model('m', [call('a'), { ...call('b'), thoughtSignature: 'signature' }], [metadata('a'), metadata('b')]),
    user('r', [response('b'), response('a')]), model('end', 'done'),
  ];
  const normalized = awikiNormalizeGeminiHistory(messages);
  assert.deepEqual(normalized[0], messages[0]);
  assert.deepEqual(normalized[1].content, messages[1].content);
  assert.deepEqual(normalized[2], messages[2]);
  assert.deepEqual(normalized[1].toolCalls, []);
  const result = evaluate(source).convert(normalized);
  assert.deepEqual(requirePaired(result), ['a', 'b']);
});

test('metadata-only old results remain, but missing, ambiguous or cross-prompt history fails closed', () => {
  const valid = [user('u', 'prompt'), model('m', 'text', [metadata('a')]), model('end', 'done')];
  assert.deepEqual(requirePaired(evaluate(source).convert(awikiNormalizeGeminiHistory(valid))), ['a']);
  for (const invalid of [
    [user('u', 'prompt'), model('m', [call('a')]), model('end', 'done')],
    [model('m', '', [metadata('a')]), user('r', [response('unknown')])],
    [model('m', '', [metadata('a')]), user('r', [response('a', 'different')])],
    [model('m', '', [metadata('a')]), user('u', 'new human prompt'), model('n', 'text'), user('r', [response('a')])],
    [model('m', [call('a', 'shell', {x: 1})], [metadata('a')]), user('r', [response('a')])],
    [model('m', [call('a')]), user('r', [response('a')]), model('m2', [call('a')]), user('r2', [response('a')])],
    [model('m', '', [metadata('a')]), user('r', [response('a')]), model('m2', 'text'), user('r2', [response('a')])],
  ]) assert.throws(() => awikiNormalizeGeminiHistory(invalid), /^Error: awiki_gemini_history_unrecoverable$/);
});
