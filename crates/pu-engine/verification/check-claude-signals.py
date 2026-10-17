#!/usr/bin/env python3
"""Exercise real Claude 2.1.300 PTY + production observer with a local fake provider.
No real credentials or configuration are read. Pass an already-built pu-engine
lib test executable, the native Claude executable, and question|permission|error.
"""
import argparse
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('test_binary', type=str)
parser.add_argument('claude')
parser.add_argument('scenario', choices=['question', 'permission', 'error', 'reject', 'reject_after_complete'])
args = parser.parse_args()
from pathlib import Path
import tempfile, os, json, subprocess, http.server, threading, pty, fcntl, termios, struct, signal, select, time, uuid
root = Path(tempfile.mkdtemp(prefix='pp-claude-native-')).resolve()
home = root / 'home'
config = root / 'config'
repo = root / 'repo'
for p in [home, config, repo]:
    p.mkdir()

class Handler(http.server.BaseHTTPRequestHandler):

    def do_POST(self):
        body = self.rfile.read(int(self.headers.get('Content-Length', 0)))
        if 'count_tokens' in self.path:
            payload = b'{"input_tokens":10}'
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Content-Length', str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)
            return
        request = json.loads(body)
        (root / 'request-shape.json').write_text(json.dumps({'path': self.path, 'keys': list(request), 'model': request.get('model')}))
        events = [('message_start', {'type': 'message_start', 'message': {'id': 'msg_probe', 'type': 'message', 'role': 'assistant', 'content': [], 'model': request.get('model', 'claude-sonnet-4-6'), 'stop_reason': None, 'stop_sequence': None, 'usage': {'input_tokens': 10, 'output_tokens': 0}}}), ('content_block_start', {'type': 'content_block_start', 'index': 0, 'content_block': {'type': 'text', 'text': ''}}), ('content_block_delta', {'type': 'content_block_delta', 'index': 0, 'delta': {'type': 'text_delta', 'text': 'Native probe done.'}}), ('content_block_stop', {'type': 'content_block_stop', 'index': 0}), ('message_delta', {'type': 'message_delta', 'delta': {'stop_reason': 'end_turn', 'stop_sequence': None}, 'usage': {'output_tokens': 5}}), ('message_stop', {'type': 'message_stop'})]
        if args.scenario in ['question','permission'] and any((t.get('name') == ('Write' if args.scenario == 'permission' else 'AskUserQuestion') for t in request.get('tools', []))) and 'Native test hello' in json.dumps(request) and (not any((isinstance(m.get('content'), list) and any((isinstance(c, dict) and c.get('type') == 'tool_result' for c in m['content'])) for m in request.get('messages', [])))):
            question = {'questions': [{'question': 'PurePoint native exact question?', 'header': 'Probe', 'options': [{'label': 'First', 'description': 'First choice'}, {'label': 'Second', 'description': 'Second choice'}], 'multiSelect': False}]}
            if args.scenario == 'permission':
                question = {'file_path': str(repo / 'native-file.txt'), 'content': 'sandbox permission proof'}
            events = [events[0], ('content_block_start', {'type': 'content_block_start', 'index': 0, 'content_block': {'type': 'tool_use', 'id': 'toolu_native_question', 'name': 'Write' if args.scenario == 'permission' else 'AskUserQuestion', 'input': {}}}), ('content_block_delta', {'type': 'content_block_delta', 'index': 0, 'delta': {'type': 'input_json_delta', 'partial_json': json.dumps(question)}}), ('content_block_stop', {'type': 'content_block_stop', 'index': 0}), ('message_delta', {'type': 'message_delta', 'delta': {'stop_reason': 'tool_use', 'stop_sequence': None}, 'usage': {'output_tokens': 50}}), ('message_stop', {'type': 'message_stop'})]
        time.sleep(2)
        if args.scenario == 'error':
            self.send_response(400)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            self.wfile.write(b'{"type":"error","error":{"type":"invalid_request_error","message":"sandbox terminal failure"}}')
            return
        payload = ''.join(('event: ' + kind + '\ndata: ' + json.dumps(data) + '\n\n' for (kind, data) in events)).encode()
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.send_header('Content-Length', str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, *a):
        pass
server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
threading.Thread(target=server.serve_forever, daemon=True).start()
seed = {'hasCompletedOnboarding': True, 'customApiKeyResponses': {'approved': ['sandbox-fixture'], 'rejected': []}, 'projects': {str(repo): {'hasTrustDialogAccepted': True, 'hasCompletedProjectOnboarding': True}}}
for p in [home / '.claude.json', config / '.claude.json']:
    p.write_text(json.dumps(seed))
import shlex
hook = root/'deny-settings.json'
marker = shlex.quote(str(root/'accepted-once'))
command = 'sleep 2; echo sandbox-deny >&2; exit 2'
if args.scenario == 'reject_after_complete':command=f'if test -f {marker}; then sleep 2; echo sandbox-deny >&2; exit 2; fi; touch {marker}'
if args.scenario.startswith('reject'):hook.write_text(json.dumps({'hooks':{'UserPromptSubmit':[{'hooks':[{'type':'command','command':command}]}]}}))
env = {'HOME': str(home), 'CLAUDE_CONFIG_DIR': str(config), 'ANTHROPIC_API_KEY': 'sandbox-fixture', 'ANTHROPIC_BASE_URL': f'http://127.0.0.1:{server.server_port}', 'CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC': '1', 'DISABLE_AUTOUPDATER': '1', 'PATH': '/opt/homebrew/bin:/usr/bin:/bin', 'TERM': 'xterm-256color', 'SHELL': '/bin/sh', 'LANG': 'en_US.UTF-8'}
sid = str(uuid.uuid4())
(a, b) = pty.openpty()
fcntl.ioctl(b, termios.TIOCSWINSZ, struct.pack('HHHH', 40, 120, 0, 0))
p = subprocess.Popen([args.claude, '--session-id', sid, '--setting-sources', '', '--model', 'claude-sonnet-4-6'] + (['--permission-mode', 'manual'] if args.scenario == 'permission' else []) + (['--settings',str(hook)] if args.scenario.startswith('reject') else []), cwd=repo, env=env, stdin=b, stdout=b, stderr=b, start_new_session=True)
os.close(b)
output = bytearray()
context = root / 'observer-context.json'
context.write_text(json.dumps({'command': args.claude, 'env': list(env.items()), 'cwd': str(repo), 'session': sid, 'pid': p.pid, 'expected': args.scenario}))
observer_log = open(root / 'observer-test.log', 'w')
observer = subprocess.Popen([args.test_binary, '--exact', 'signals::claude::tests::native_interactive_observer_probe', '--ignored', '--nocapture'], env={**os.environ, 'PUREPOINT_CLAUDE_PROBE_CONTEXT': str(context)}, stdout=observer_log, stderr=subprocess.STDOUT)
try:
    for stage in range(16):
        deadline = time.monotonic() + 0.7
        while time.monotonic() < deadline:
            if select.select([a], [], [], 0.1)[0]:
                try:
                    output.extend(os.read(a, 65536))
                except OSError:
                    break
        result = subprocess.run([args.claude, 'agents', '--json'], env=env, cwd=repo, capture_output=True, text=True, timeout=3)
        if result.returncode == 0:
            try:
                rows = json.loads(result.stdout)
                (root / f'agents-{stage}.json').write_text(json.dumps(rows))
                registry=config/'sessions'/f'{p.pid}.json'
                if registry.exists():(root/f'registry-{stage}.json').write_text(registry.read_text())
                print('stage', stage, 'rows', rows, flush=True)
            except:
                print('nonjson', result.stdout[:200], flush=True)
        if stage == 3:
            os.write(a, b'Native test hello\r')
        if stage == 10:
            if args.scenario == 'reject_after_complete':os.write(a,b'Rejected second prompt\r')
            elif args.scenario in ['question','permission']:os.write(a,b'\r')
    observer.wait(timeout=30)
    observer_log.close()
    assert observer.returncode == 0, (root / 'observer-test.log').read_text()
    print('root', root, 'sid', sid, 'PID', p.pid, 'request', (root / 'request-shape.json').exists(), flush=True)
    (root / 'terminal.bin').write_bytes(output)
    print('transcript files', list(config.rglob('*.jsonl')), flush=True)
finally:
    if observer.poll() is None:
        observer.terminate()
        observer.wait(timeout=3)
    observer_log.close()
    try:
        os.killpg(p.pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    try:
        p.wait(timeout=3)
    except subprocess.TimeoutExpired:
        os.killpg(p.pid, signal.SIGKILL)
        p.wait()
    os.close(a)
    server.shutdown()
