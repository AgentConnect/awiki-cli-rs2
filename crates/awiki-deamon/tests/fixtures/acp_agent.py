#!/usr/bin/env python3
"""Deterministic stdio peer for ACP transport tests; never calls a model."""
import json, os, pathlib, subprocess, sys, threading, urllib.request

if '--version' in sys.argv:
    print('1.0.0')
    raise SystemExit(0)

sid = 'native-exact-session'
mode_file = pathlib.Path.cwd()/'recovery-mode'
recovery = mode_file.read_text() if mode_file.exists() else ''
if recovery.startswith('startup-'):
    if recovery == 'startup-missing':
        print(f'Error resuming session: Invalid session identifier "{sid}".', file=sys.stderr, flush=True)
    elif recovery == 'startup-other-id':
        print('Error resuming session: Invalid session identifier "another-session".', file=sys.stderr, flush=True)
    else:
        print('Error resuming session: permission denied', file=sys.stderr, flush=True)
    raise SystemExit(42)
cwd = None
prompt_id = None
mcp = None
lock = threading.Lock()
def emit(value):
    with lock:
        print(json.dumps({'jsonrpc':'2.0', **value}), flush=True)
def reply(id, value): emit({'id':id,'result':value})
def chunk(text): emit({'method':'session/update','params':{'sessionId':sid,'update':{'sessionUpdate':'agent_message_chunk','content':{'type':'text','text':text}}}})
def finish(text):
    chunk(text)
    reply(prompt_id, {'stopReason':'end_turn'})
def tool_question():
    headers = {h['name']:h['value'] for h in mcp['headers']}
    headers.update({'Content-Type':'application/json','Accept':'application/json, text/event-stream'})
    req = urllib.request.Request(mcp['url'],headers=headers,data=json.dumps({'jsonrpc':'2.0','id':1,'method':'tools/call','params':{'name':'request_user_input','arguments':{'message':'Choose a color','schema_json':json.dumps({'type':'object','required':['color'],'properties':{'color':{'type':'string','enum':['red','blue']}}})}}}).encode())
    with urllib.request.urlopen(req,timeout=20) as response:
        value=json.load(response)
    if 'error' in value: finish('TOOL_ERROR')
    else: finish('ANSWER_'+json.loads(value['result']['content'][0]['text'])['content']['color'])

for line in sys.stdin:
    request=json.loads(line)
    method=request.get('method')
    params=request.get('params',{})
    id=request.get('id')
    if method=='initialize':
        reply(id,{'protocolVersion':1,'agentInfo':{'name':'fixture','version':'1.0.0'},'agentCapabilities':{'mcpCapabilities':{'http':True},'promptCapabilities':{'image':True},'sessionCapabilities':{'resume':{},'close':{},'list':{}}}})
    elif method in ('session/new','session/resume','session/load'):
        cwd=pathlib.Path(params['cwd']);cwd.mkdir(parents=True,exist_ok=True)
        assert cwd.resolve() == pathlib.Path.cwd().resolve(), 'process cwd must match the ACP session cwd'
        with (cwd/'protocol.jsonl').open('a') as log: log.write(json.dumps({'method':method,'sessionId':params.get('sessionId')})+'\n')
        if method!='session/new' and recovery:
            error = {'code':-32603,'message':'Internal error: OpenCode service failure','data':{'service':'session'}}
            if recovery == 'kimi': error = {'code':-32602,'message':f'Invalid params: Unknown sessionId: {sid}'}
            if recovery == 'dsh': error = {'code':-32602,'message':f'Invalid params: session is not resumable: {sid}'}
            emit({'id':id,'error':error})
            continue
        if method!='session/new' and (cwd/'missing-gemini').exists():
            assert sys.argv[-2:]==['--resume',sid]
            emit({'id':id,'error':{'code':-32603,'message':'Internal error','data':{'details':'No previous sessions found for this project.'}}})
            continue
        if method!='session/new' and (cwd/'missing').exists():
            emit({'id':id,'error':{'code':-32002,'message':'Session not found'}})
            continue
        if method!='session/new':
            assert params['sessionId']==sid
            chunk('REPLAY_MUST_NOT_APPEAR')
        mcp=params['mcpServers'][0]
        reply(id,{'sessionId':sid,'configOptions':[{'id':'model','name':'Model','category':'model','type':'select','currentValue':'flash','options':[{'value':'flash','name':'Flash'},{'value':'pro','name':'Pro'}]}]})
    elif method=='session/list':
        with (cwd/'protocol.jsonl').open('a') as log: log.write(json.dumps({'method':method,'cursor':params.get('cursor')})+'\n')
        assert pathlib.Path(params['cwd']).resolve() == cwd.resolve()
        if recovery == 'list-error': emit({'id':id,'error':{'code':-32603,'message':'unavailable'}})
        elif recovery == 'list-malformed': reply(id,{'sessions':[{'title':'invalid item'}]})
        elif recovery == 'list-bad-cursor': reply(id,{'sessions':[],'nextCursor':123})
        elif recovery == 'list-loop': reply(id,{'sessions':[],'nextCursor':'loop'})
        elif not params.get('cursor'): reply(id,{'sessions':[],'nextCursor':'page2'})
        else: reply(id,{'sessions':[{'sessionId':sid,'cwd':str(cwd)}] if recovery=='list-present' else []})
    elif method=='session/prompt':
        prompt_id=id
        with (cwd/'prompts.jsonl').open('a') as log:
            log.write(json.dumps(params['prompt'])+'\n')
        (cwd/'wrapper-environment.json').write_text(json.dumps({'token_present': bool(os.environ.get('AWIKI_RUNTIME_RPC_TOKEN')), 'socket_present': bool(os.environ.get('AWIKI_DAEMON_RPC_SOCKET')), 'executable_present': bool(os.environ.get('AWIKI_DAEMON_EXECUTABLE'))}))
        text=''.join(p.get('text','') for p in params['prompt'])
        if 'QUESTION_MCP' in text:
            threading.Thread(target=tool_question,daemon=True).start()
        elif 'QUESTION_NATIVE' in text:
            emit({'id':'question','method':'elicitation/create','params':{'sessionId':sid,'mode':'form','message':'Choose a color','requestedSchema':{'type':'object','required':['color'],'properties':{'color':{'type':'string','enum':['red','blue']}}}}})
            if 'ABANDON' in text:
                threading.Timer(0.2, lambda: finish('UNANSWERED_MUST_NOT_SUCCEED')).start()
        elif 'QUESTION_PERMISSION' in text:
            emit({'id':'permission','method':'session/request_permission','params':{'sessionId':sid,'toolCall':{'toolCallId':'q1','title':'AskUserQuestion','content':[{'type':'content','content':{'type':'text','text':'Choose a color'}}]},'options':[{'optionId':'red','name':'Red','kind':'allow_once'},{'optionId':'blue','name':'Blue','kind':'allow_once'}]}})
        elif 'UNKNOWN_INTERACTION' in text:
            emit({'id':'unknown','method':'_unknown_interaction','params':{}})
        elif 'CANCEL_CHILD' in text:
            child=subprocess.Popen(['sleep','120'])
            (cwd/'child.pid').write_text(str(child.pid))
            chunk('BEFORE_CANCEL')
        elif 'REFUSAL' in text:
            chunk('Refused')
            reply(id,{'stopReason':'refusal'})
        else:
            finish('FIXTURE_RESPONSE')
    elif method=='session/cancel':
        chunk('LATE_OUTPUT_MUST_NOT_APPEAR')
        reply(prompt_id,{'stopReason':'cancelled'})
    elif method in ('session/close','session/set_config_option','session/set_model'):
        reply(id,{'configOptions':[]})
    elif id=='question':
        value=request.get('result',{})
        finish('ANSWER_'+value.get('content',{}).get('color',value.get('action','ERROR')))
    elif id=='permission':
        finish('ANSWER_'+request['result']['outcome'].get('optionId','cancel'))
    elif id=='unknown':
        finish('IGNORED_ERROR_MUST_NOT_SUCCEED')
