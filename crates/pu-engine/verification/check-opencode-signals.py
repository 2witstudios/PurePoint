#!/usr/bin/env python3
"""Real OpenCode 2.0.28 PTY and read-only production observer with a fake provider.
Pass an already-built pu-engine lib test executable, absolute OpenCode executable,
and question|permission|error|completion. Uses only a disposable HOME/config and
an isolated normal native managed-service port; no standalone/server override.
"""
import argparse
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('test_binary')
parser.add_argument('opencode')
parser.add_argument('scenario', choices=['question', 'permission', 'error', 'completion'])
args = parser.parse_args()
from pathlib import Path
import tempfile, os, json, subprocess, http.server, threading, pty, fcntl, termios, struct, signal, select, time, uuid, urllib.request, base64
import sys, socket
mode = args.scenario
root = Path(tempfile.mkdtemp(prefix='pp-opencode-native-')).resolve()
home = root / 'home'
repo = root / 'repo'
state = root / 'state'
config = root / 'config'
data = root / 'data'
for p in [home, repo, state, config, data]:
    p.mkdir()

class Handler(http.server.BaseHTTPRequestHandler):

    def do_POST(self):
        body = self.rfile.read(int(self.headers.get('Content-Length', 0)))
        request = json.loads(body)
        tool_results = [m.get('content') for m in request.get('messages', []) if m.get('role') == 'tool']
        if tool_results:
            (root / 'native-tool-results.json').write_text(json.dumps(tool_results))
        (root / 'request-metadata.json').write_text(json.dumps({'path': self.path, 'model': request.get('model'), 'tools': [x.get('function', {}).get('name') for x in request.get('tools', [])]}))
        time.sleep(2)
        if mode == 'error':
            self.send_response(400)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            self.wfile.write(b'{"error":{"message":"sandbox failure","type":"invalid_request_error"}}')
            return
        chunks = [{'id': 'chatcmpl_probe', 'object': 'chat.completion.chunk', 'created': int(time.time()), 'model': 'chat', 'choices': [{'index': 0, 'delta': {'role': 'assistant', 'content': 'Native probe done.'}, 'finish_reason': None}]}, {'id': 'chatcmpl_probe', 'object': 'chat.completion.chunk', 'created': int(time.time()), 'model': 'chat', 'choices': [{'index': 0, 'delta': {}, 'finish_reason': 'stop'}], 'usage': {'prompt_tokens': 10, 'completion_tokens': 5, 'total_tokens': 15}}]
        if mode in ['question', 'permission'] and (not any((m.get('role') == 'tool' for m in request.get('messages', [])))):
            arguments = {'questions': [{'question': 'Native exact question?', 'header': 'Probe', 'options': [{'label': 'First', 'description': 'First choice'}, {'label': 'Second', 'description': 'Second choice'}]}]} if mode == 'question' else {'path': 'native-file.txt', 'content': 'native permission proof'}
            chunks[0]['choices'][0]['delta'] = {'role': 'assistant', 'tool_calls': [{'index': 0, 'id': 'call_native_' + mode, 'type': 'function', 'function': {'name': 'question' if mode == 'question' else 'write', 'arguments': json.dumps(arguments)}}]}
            chunks[1]['choices'][0]['finish_reason'] = 'tool_calls'
        payload = (''.join(('data: ' + json.dumps(c) + '\n\n' for c in chunks)) + 'data: [DONE]\n\n').encode()
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.send_header('Content-Length', str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, *a):
        pass
server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
threading.Thread(target=server.serve_forever, daemon=True).start()
settings = {'model': 'fixture/chat', 'update': 'disable', 'share': 'disabled', 'permissions': [{'action': '*', 'resource': '*', 'effect': 'allow'}, {'action': 'edit', 'resource': '*', 'effect': 'ask'}], 'providers': {'fixture': {'name': 'Fixture', 'package': '@opencode/ai/providers/openai-compatible', 'settings': {'apiKey': 'sandbox-fixture', 'baseURL': f'http://127.0.0.1:{server.server_port}/v1'}, 'models': {'chat': {'name': 'Native fixture', 'limit': {'context': 32000, 'output': 1000}, 'capabilities': {'tools': True}}}}}}
env = {'HOME': str(home), 'XDG_STATE_HOME': str(state), 'XDG_CONFIG_HOME': str(config), 'XDG_DATA_HOME': str(data), 'XDG_CACHE_HOME': str(root / 'cache'), 'PATH': '/opt/homebrew/bin:/usr/bin:/bin', 'TERM': 'xterm-256color', 'SHELL': '/bin/sh', 'LANG': 'en_US.UTF-8', 'OPENCODE_DISABLE_AUTOUPDATE': '1', 'OPENCODE_DISABLE_MODELS_FETCH': '1', 'OPENCODE_CONFIG_CONTENT': json.dumps(settings)}
with socket.socket() as reservation:
    reservation.bind(('127.0.0.1', 0))
    port = reservation.getsockname()[1]
(config / 'opencode').mkdir()
service_config = config / 'opencode/service.json'
service_config.write_text(json.dumps({'hostname': '127.0.0.1', 'port': port}))
service_config.chmod(384)
sid = 'ses_probe_' + uuid.uuid4().hex
(a, b) = pty.openpty()
fcntl.ioctl(b, termios.TIOCSWINSZ, struct.pack('HHHH', 40, 120, 0, 0))
p = subprocess.Popen([args.opencode, '--session', sid], cwd=repo, env=env, stdin=b, stdout=b, stderr=b, start_new_session=True)
os.close(b)
output = bytearray()
snapshots = []
owned_service = None
context = root / 'observer-context.json'
context.write_text(json.dumps({'cwd': str(repo), 'env': list(env.items()), 'session': sid, 'pid': p.pid, 'expected': mode}))
observer_log = open(root / 'observer-test.log', 'w')
observer = subprocess.Popen([args.test_binary, '--exact', 'signals::opencode::tests::native_interactive_observer_probe', '--ignored', '--nocapture'], env={**os.environ, 'PUREPOINT_OPENCODE_PROBE_CONTEXT': str(context)}, stdout=observer_log, stderr=subprocess.STDOUT)
print('starting', root, 'PID', p.pid, flush=True)
try:
    for stage in range(25):
        deadline = time.monotonic() + 0.7
        while time.monotonic() < deadline:
            if select.select([a], [], [], 0.1)[0]:
                try:
                    chunk = os.read(a, 65536)
                    output.extend(chunk)
                    if b'\x1b[6n' in chunk:
                        os.write(a, b'\x1b[1;1R')
                except OSError:
                    break
        if p.poll() is not None:
            print('early exit', p.returncode, repr(output), flush=True)
            break
        if stage == 5:
            os.write(a, b'Native test hello')
        if stage == 6:
            os.write(a, b'\r')
        if stage in [16, 17] and mode in ['permission', 'question']:
            os.write(a, b'\r')
        registration = state / 'opencode/service.json'
        if registration.exists():
            r = json.loads(registration.read_text())
            owned_service = r['pid']
            try:
                auth = base64.b64encode(('opencode:' + r.get('password', '')).encode()).decode()
                req = urllib.request.Request(r['url'].rstrip('/') + '/api/session/' + sid, headers={'Authorization': 'Basic ' + auth})
                v = json.load(urllib.request.urlopen(req, timeout=0.5))
                snapshots.append({'stage': stage, 'session': v})
            except Exception as error:
                snapshots.append({'stage': stage, 'error': type(error).__name__})
    (root / 'terminal.bin').write_bytes(output)
    (root / 'snapshots.json').write_text(json.dumps(snapshots))
    (root / 'context.json').write_text(json.dumps({'cwd': str(repo), 'env': list(env.items()), 'session': sid, 'pid': p.pid}))
    observer.wait(timeout=40)
    observer_log.close()
    assert observer.returncode == 0, (root / 'observer-test.log').read_text()
    if mode == 'permission':
        assert (repo / 'native-file.txt').read_text() == 'native permission proof'
    if mode == 'question':
        assert 'First' in (root / 'native-tool-results.json').read_text()
    print('root', root, 'PID', p.pid, 'service', owned_service, 'request', (root / 'request-metadata.json').exists(), flush=True)
finally:
    if observer.poll() is None:
        observer.terminate()
        observer.wait(timeout=3)
    observer_log.close()
    (root / 'terminal.bin').write_bytes(output)
    if p.poll() is None:
        try:
            os.killpg(p.pid, signal.SIGTERM)
        except (ProcessLookupError, PermissionError):
            if p.poll() is None:
                p.terminate()
    try:
        p.wait(timeout=3)
    except subprocess.TimeoutExpired:
        os.killpg(p.pid, signal.SIGKILL)
        p.wait()
    if owned_service:
        try:
            os.kill(owned_service, signal.SIGTERM)
        except ProcessLookupError:
            pass
    os.close(a)
    server.shutdown()
