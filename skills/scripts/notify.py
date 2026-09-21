#!/usr/bin/env python3
"""One-shot, POSIX task-local Notify runner. No listener, retry loop or daemon."""
import argparse
import fcntl
import hashlib
import json
import math
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import tempfile
import time

STATES = {'completed', 'blocked', 'failed', 'action_required'}
STATE_LABELS = {'completed': '已完成', 'blocked': '暂时受阻',
                'failed': '执行失败', 'action_required': '需要你处理'}
OPAQUE = re.compile(r'^[A-Za-z0-9_-]{1,96}$')


def require(condition, reason):
    if not condition:
        raise ValueError(reason)


def nonempty(value):
    return isinstance(value, str) and bool(value.strip())


def read_json(path):
    with open(path, encoding='utf-8') as stream:
        return json.load(stream)


def save(path, value):
    fd, name = tempfile.mkstemp(dir=path.parent, prefix='.notify-')
    try:
        with os.fdopen(fd, 'w', encoding='utf-8') as stream:
            json.dump(value, stream, ensure_ascii=False)
            stream.write('\n')
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(name, path)
    finally:
        if os.path.exists(name):
            os.unlink(name)


def validate_binding(binding):
    require(isinstance(binding, dict), 'invalid_binding')
    require(set(binding) - {'notify_level'} == {'task_id', 'workspace', 'identity', 'sender_did',
                             'receiver_did', 'allowed_states', 'authorized'},
            'unexpected_binding_fields')
    for key in ('task_id', 'workspace', 'identity', 'sender_did', 'receiver_did'):
        require(nonempty(binding.get(key)), 'missing_' + key)
    require(OPAQUE.fullmatch(binding['task_id']), 'invalid_task_id')
    require(Path(binding['workspace']).is_absolute(), 'workspace_must_be_absolute')
    require(binding['sender_did'].startswith('did:'), 'invalid_sender')
    require(binding['receiver_did'].startswith('did:'), 'invalid_receiver')
    allowed = binding.get('allowed_states')
    require(isinstance(allowed, list) and allowed and all(s in STATES for s in allowed),
            'invalid_allowed_states')
    require(binding.get('authorized') is True, 'notification_not_authorized')
    require(binding.get('notify_level', 'normal') in ('normal', 'urgent'), 'invalid_notify_level')


def stop_child(proc):
    # Also kill descendants holding stdout open, even if the direct child exited.
    try:
        os.killpg(proc.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    proc.wait()


def run_cli(binary, binding, args, deadline):
    remaining = deadline - time.monotonic()
    if remaining <= 0:
        raise TimeoutError('deadline')
    env = dict(os.environ, AWIKI_CLI_WORKSPACE_HOME_DIR=binding['workspace'],
               AWIKI_CLI_UPDATE_CACHE_ONLY='1')
    for key in ('AWIKI_WORKSPACE', 'AWIKI_WORKSPACE_HOME', 'AWIKI_HOME'):
        env.pop(key, None)
    proc = subprocess.Popen([binary, '--identity', binding['identity'], *args,
                             '--format', 'json'], stdin=subprocess.DEVNULL,
                            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                            env=env, start_new_session=True)
    try:
        output, _ = proc.communicate(timeout=max(0.001, deadline - time.monotonic()))
    except subprocess.TimeoutExpired:
        raise TimeoutError('deadline') from None
    finally:
        stop_child(proc)
    value = json.loads(output)
    require(isinstance(value, dict), 'invalid_cli_envelope')
    return value


def send(context, event, persist, binary, timeout):
    deadline = time.monotonic() + timeout
    binding = context['binding']
    validate_binding(binding)
    require(context.get('version') == 1 and isinstance(context.get('events'), dict),
            'invalid_context')
    require(isinstance(event, dict) and event.get('task_id') == binding['task_id'],
            'wrong_task')
    event_id = event.get('event_id')
    require(isinstance(event_id, str) and OPAQUE.fullmatch(event_id), 'invalid_event_id')
    status = event.get('status')
    require(status in STATES, 'invalid_status')
    if context.get('enabled') is not True:
        return {'outcome': 'disabled'}
    require(status in binding['allowed_states'], 'state_not_authorized')
    previous = context['events'].get(event_id)
    if previous is not None:
        require(previous['status'] == status, 'event_conflict')
        return dict(previous, duplicate=True)
    require(not context.get('terminal'), 'task_already_terminal')
    for key in ('title', 'summary', 'next_action'):
        require(nonempty(event.get(key)), 'missing_' + key)
    require('\n' not in event['title'] and '\r' not in event['title'], 'invalid_title')
    next_action = event['next_action']
    if status in ('action_required', 'blocked'):
        next_action = '请回电脑的 Codex 任务处理。' + next_action
    text = f"{STATE_LABELS[status]} · {event['title']}\n{event['summary']}\n下一步：{next_action}"
    # Task identity, receiver and event define the key; never the text or status alone.
    key = hashlib.sha256(json.dumps([binding, event_id], sort_keys=True).encode()).hexdigest()[:40]
    message_id, idem = 'msg-notify-' + key, 'notify-' + key
    receipt = {'status': status, 'outcome': 'not_sent', 'reason': 'preflight_interrupted',
               'client_message_id': message_id, 'idempotency_key': idem}
    context['events'][event_id] = receipt
    if status in ('completed', 'failed'):
        context['terminal'] = status
    # Claim before any network work. A lost receipt must never cause another attempt.
    persist(context)
    transmitting = False
    try:
        identity = run_cli(binary, binding, ['id', 'current'], deadline)
        actual = (identity.get('data') or {}).get('identity') or {}
        require(identity.get('ok') is True and actual.get('identity_name') == binding['identity']
                and actual.get('did') == binding['sender_did'], 'sender_mismatch')
        resolved = run_cli(binary, binding, ['id', 'resolve', '--did', binding['receiver_did']], deadline)
        data = resolved.get('data') or {}
        require(resolved.get('ok') is True and
                (data.get('resolve') or {}).get('did') == binding['receiver_did'] and
                ('lookup' not in data or (data['lookup'] or {}).get('did') == binding['receiver_did']),
                'receiver_mismatch')
        args = ['msg', 'send', '--to', binding['receiver_did'], '--text', text,
                '--client-message-id', message_id, '--idempotency-key', idem,
                '--notify', binding.get('notify_level', 'normal')]
        dry = run_cli(binary, binding, [*args, '--dry-run'], deadline)
        plan = (dry.get('data') or {}).get('plan') or {}
        require(dry.get('ok') is True and plan.get('action') == 'direct.send' and
                plan.get('identity') == binding['identity'] and
                (plan.get('target') or {}).get('did') == binding['receiver_did'] and
                plan.get('listener_required') is False and nonempty(plan.get('transport_policy')) and
                plan.get('notify_level') == binding.get('notify_level', 'normal') and
                plan.get('client_message_id') == message_id and plan.get('idempotency_key') == idem,
                'dry_run_mismatch')
        require(time.monotonic() < deadline, 'deadline_before_send')
        receipt.update(outcome='pending_confirmation', reason='send_started')
        persist(context)
        transmitting = True
        result = run_cli(binary, binding, args, deadline)
        data = result.get('data') or {}
        delivery, message = data.get('delivery') or {}, data.get('message') or {}
        accepted = delivery.get('accepted') is True or delivery.get('final_acceptance') is True
        if result.get('ok') is True and accepted and nonempty(message.get('id')):
            receipt.update(outcome='accepted', reason='server_accepted', message_id=message['id'])
        elif delivery.get('accepted') is False and delivery.get('final_acceptance') is False:
            receipt.update(outcome='not_sent', reason='explicit_rejection')
        else:
            # A generic ok:false/exit failure can be a lost response after acceptance.
            receipt.update(outcome='pending_confirmation', reason='acceptance_unconfirmed')
    except (OSError, ValueError, TypeError, AttributeError, TimeoutError, KeyboardInterrupt):
        receipt.update(outcome='pending_confirmation' if transmitting else 'not_sent',
                       reason='send_unconfirmed' if transmitting else 'preflight_failed')
    persist(context)
    return dict(receipt)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=('enable', 'send', 'status', 'disable'))
    parser.add_argument('--context', type=Path, required=True)
    parser.add_argument('--input', type=Path, help='Trusted binding (enable) or event (send) JSON')
    parser.add_argument('--cli', default='awiki-cli')
    parser.add_argument('--timeout', type=float, default=30.0)
    args = parser.parse_args()
    require(os.name == 'posix', 'posix_required')
    require(math.isfinite(args.timeout) and 0 < args.timeout <= 30, 'timeout_out_of_range')
    path = args.context.absolute()
    require(not path.is_symlink(), 'context_symlink')
    # Keep the lock inode stable across atomic context replacements. Never wait for it.
    fd = os.open(str(path) + '.lock', os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
    try:
        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        if args.action == 'enable':
            require(not path.exists(), 'context_already_exists')
            binding = read_json(args.input)
            validate_binding(binding)
            save(path, {'version': 1, 'binding': binding, 'enabled': True, 'events': {}})
            return {'outcome': 'enabled'}
        context = read_json(path)  # Never recreate missing task context automatically.
        if args.action == 'status':
            return {'enabled': context['enabled'], 'events': context['events']}
        if args.action == 'disable':
            context['enabled'] = False
            save(path, context)
            return {'outcome': 'disabled'}
        return send(context, read_json(args.input), lambda c: save(path, c), args.cli, args.timeout)
    finally:
        os.close(fd)


if __name__ == '__main__':
    def cancelled(_signum, _frame):
        raise KeyboardInterrupt()
    signal.signal(signal.SIGTERM, cancelled)
    try:
        print(json.dumps(main(), ensure_ascii=False))
    except (OSError, ValueError, TypeError, KeyError, AttributeError, KeyboardInterrupt):
        # Do not echo CLI output, paths, credentials or message bodies.
        print(json.dumps({'outcome': 'blocked', 'reason': 'invalid_or_unavailable_task_context'}))
        sys.exit(1)
