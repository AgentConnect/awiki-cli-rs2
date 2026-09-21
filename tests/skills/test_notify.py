"""Subprocess tests of the shipped one-shot Skill; no real account or service."""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest

RUNNER = Path(__file__).resolve().parents[2] / 'skills/scripts/notify.py'
FAKE = r'''#!/usr/bin/env python3
import json, os, subprocess, sys, time
from pathlib import Path
args=sys.argv[1:]
cfg=json.loads(Path(os.environ['NOTIFY_FIXTURE']).read_text())
def arg(k): return args[args.index(k)+1]
with open(cfg['log'], 'a') as f:
 f.write(json.dumps({'args':args, 'workspace':os.environ.get('AWIKI_CLI_WORKSPACE_HOME_DIR')})+'\n')
mode=cfg.get('mode','accepted')
stage='current' if 'current' in args else 'resolve' if 'resolve' in args else 'dry' if '--dry-run' in args else 'send'
if cfg.get('delay_stage')==stage:
 if cfg.get('spawn_child'):
  p=subprocess.Popen([sys.executable,'-c', 'import time; time.sleep(60)'])
  Path(cfg['pid']).write_text(str(p.pid))
 time.sleep(60)
if stage=='current':
 data={'identity':{'identity_name': 'wrong' if mode=='wrong_sender' else arg('--identity'), 'did':'did:test:sender'}}
elif stage=='resolve':
 data={'resolve':{'did': 'did:test:other' if mode=='wrong_receiver' else arg('--did')}}
elif stage=='dry':
 data={'plan':{'action':'direct.send','identity':arg('--identity'),'target':{'did':arg('--to')},'listener_required':False,'transport_policy':'http_only','notify_level':arg('--notify'),'client_message_id':arg('--client-message-id'),'idempotency_key':arg('--idempotency-key')}}
 if mode=='wrong_plan': data['plan']['target']['did']='did:test:other'
else:
 if mode=='lost':
  print('truncated'); sys.exit(1)
 data={'message':{'id':arg('--client-message-id')},'delivery':{'accepted':True}}
 if mode=='rejected': data['delivery']={'accepted':False,'final_acceptance':False}
 if mode=='generic_error': print(json.dumps({'ok':False})); sys.exit(1)
 if mode=='missing_id': data['message']={}
 if mode=='final': data['delivery']={'final_acceptance':True}
print(json.dumps({'ok':True,'data':data}))
'''


class NotifyTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.context = self.root / 'context.json'
        self.log = self.root / 'calls.jsonl'
        self.fixture = self.root / 'fixture.json'
        self.cli = self.root / 'fake-cli'
        self.cli.write_text(FAKE)
        self.cli.chmod(0o700)
        self.env = dict(os.environ, NOTIFY_FIXTURE=str(self.fixture))
        self.configure()
        self.binding = dict(task_id='task-one', workspace=str(self.root / 'workspace'),
                            identity='agent', sender_did='did:test:sender',
                            receiver_did='did:test:receiver', authorized=True,
                            allowed_states=['action_required', 'blocked', 'completed', 'failed'])
        self.invoke('enable', self.binding)

    def configure(self, **kwargs):
        self.fixture.write_text(json.dumps(dict(log=str(self.log), **kwargs)))

    def invoke(self, action, value=None, timeout=30, context=None):
        args = [sys.executable, str(RUNNER), action, '--context', str(context or self.context),
                '--cli', str(self.cli), '--timeout', str(timeout)]
        if value is not None:
            request = self.root / 'request.json'
            request.write_text(json.dumps(value))
            args += ['--input', str(request)]
        proc = subprocess.run(args, env=self.env, capture_output=True, timeout=5)
        return json.loads(proc.stdout)

    def event(self, event_id='question-1', status='action_required', **kwargs):
        return dict(task_id='task-one', event_id=event_id, status=status,
                    title='导出任务', summary='选择导出格式', next_action='选择 JSON 或 CSV', **kwargs)

    def calls(self):
        return [json.loads(s) for s in self.log.read_text().splitlines()] if self.log.exists() else []

    def sends(self):
        return [c for c in self.calls() if 'send' in c['args'] and '--dry-run' not in c['args']]

    def test_two_questions_then_completion_deduplicate_each_event(self):
        for event_id, status in [('question-1', 'action_required'), ('question-2', 'action_required'), ('done', 'completed')]:
            event = self.event(event_id, status)
            result = self.invoke('send', event)
            self.assertEqual(result['outcome'], 'accepted')
            event['summary'] = 'Reworded text is still the same event'
            self.assertTrue(self.invoke('send', event)['duplicate'])
        self.assertEqual(len(self.sends()), 3)
        self.assertEqual(len({s['args'][s['args'].index('--client-message-id')+1] for s in self.sends()}), 3)
        self.assertIn('请回电脑', self.sends()[0]['args'][self.sends()[0]['args'].index('--text')+1])
        self.assertEqual(self.invoke('send', self.event('oops', 'failed'))['outcome'], 'blocked')

    def test_bad_identity_receiver_or_plan_never_send(self):
        for mode in ('wrong_sender', 'wrong_receiver', 'wrong_plan'):
            self.configure(mode=mode)
            self.assertEqual(self.invoke('send', self.event(mode))['outcome'], 'not_sent')
        self.assertFalse(self.sends())

    def test_lost_response_generic_failure_and_missing_id_remain_unknown(self):
        for mode in ('lost', 'generic_error', 'missing_id'):
            self.configure(mode=mode)
            event = self.event(mode)
            first = self.invoke('send', event)
            self.assertEqual(first['outcome'], 'pending_confirmation')
            self.configure()
            again = self.invoke('send', event)
            self.assertTrue(again['duplicate'])
            self.assertEqual(first['client_message_id'], again['client_message_id'])
        self.assertEqual(len(self.sends()), 3)

    def test_explicit_rejection_and_final_acceptance(self):
        self.configure(mode='rejected')
        self.assertEqual(self.invoke('send', self.event('reject'))['outcome'], 'not_sent')
        self.configure(mode='final')
        self.assertEqual(self.invoke('send', self.event('accept'))['outcome'], 'accepted')

    def test_deadline_kills_send_descendants_and_does_not_retry(self):
        pid = self.root / 'child.pid'
        self.configure(delay_stage='send', spawn_child=True, pid=str(pid))
        started = time.monotonic()
        result = self.invoke('send', self.event(), timeout=0.8)
        self.assertLess(time.monotonic() - started, 2)
        self.assertEqual(result['outcome'], 'pending_confirmation')
        self.assertTrue(self.invoke('send', self.event())['duplicate'])
        self.assertEqual(len(self.sends()), 1)
        # A killed descendant may briefly be a zombie until its parent is reaped.
        state = subprocess.run(['ps', '-o', 'stat=', '-p', pid.read_text()], capture_output=True, text=True).stdout.strip()
        self.assertTrue(not state or state.startswith('Z'), state)

    def test_deadline_in_preflight_never_sends(self):
        self.configure(delay_stage='resolve')
        self.assertEqual(self.invoke('send', self.event(), timeout=0.3)['outcome'], 'not_sent')
        self.assertFalse(self.sends())

    def test_disable_and_wrong_task_or_lost_context_never_send(self):
        event = self.event()
        event['task_id'] = 'other'
        self.assertEqual(self.invoke('send', event)['outcome'], 'blocked')
        self.invoke('disable')
        self.assertEqual(self.invoke('send', self.event())['outcome'], 'disabled')
        self.context.unlink()
        self.assertEqual(self.invoke('send', self.event())['outcome'], 'blocked')
        self.assertFalse(self.calls())

    def test_parallel_tasks_use_distinct_keys_and_pinned_workspace(self):
        other = self.root / 'other.json'
        binding = dict(self.binding, task_id='task-two')
        self.invoke('enable', binding, context=other)
        first = self.invoke('send', self.event())
        event = self.event()
        event['task_id'] = 'task-two'
        second = self.invoke('send', event, context=other)
        self.assertNotEqual(first['client_message_id'], second['client_message_id'])
        self.assertTrue(all(c['workspace'] == self.binding['workspace'] for c in self.calls()))

    def test_event_conflict_and_disabled_authorization(self):
        self.invoke('send', self.event())
        self.assertEqual(self.invoke('send', self.event(status='blocked'))['outcome'], 'blocked')
        other = self.root / 'unauthorized.json'
        self.assertEqual(self.invoke('enable', dict(self.binding, authorized=False), context=other)['outcome'], 'blocked')
        self.assertFalse(other.exists())
        self.assertEqual(self.invoke('enable', dict(self.binding, token='must-not-persist'), context=other)['outcome'], 'blocked')
        self.assertFalse(other.exists())

    def test_overlapping_calls_do_not_send_twice_and_cancellation_releases_lock(self):
        self.configure(delay_stage='send')
        event = self.root / 'slow.json'
        event.write_text(json.dumps(self.event()))
        proc = subprocess.Popen([sys.executable, str(RUNNER), 'send', '--context', str(self.context),
                                 '--input', str(event), '--cli', str(self.cli)],
                                env=self.env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.addCleanup(lambda: proc.kill() if proc.poll() is None else None)
        deadline = time.monotonic() + 3
        while not self.sends() and time.monotonic() < deadline:
            time.sleep(0.02)
        self.assertEqual(len(self.sends()), 1)
        self.assertEqual(self.invoke('send', self.event())['outcome'], 'blocked')
        proc.terminate()
        output, _ = proc.communicate(timeout=2)
        self.assertEqual(json.loads(output)['outcome'], 'pending_confirmation')
        self.assertTrue(self.invoke('send', self.event())['duplicate'])
        self.assertEqual(len(self.sends()), 1)
        self.assertEqual(self.invoke('disable')['outcome'], 'disabled')

    def test_receipts_do_not_store_body_and_arguments_are_not_shell_expanded(self):
        event = self.event()
        marker = str(self.root / 'must-not-exist')
        event['summary'] = 'private summary $(touch ' + marker + ')'
        self.assertEqual(self.invoke('send', event)['outcome'], 'accepted')
        self.assertFalse(Path(marker).exists())
        self.assertNotIn('private summary', self.context.read_text())
        self.assertEqual(self.context.stat().st_mode & 0o777, 0o600)


if __name__ == '__main__':
    unittest.main()
