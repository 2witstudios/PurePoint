#!/usr/bin/env python3
"""Real Codex 0.163.0 PTY plus production observer, with a local fake provider.
Pass the already-built pu-engine lib test executable, absolute native Codex path,
and question|permission|error. Configuration and credentials stay isolated.
The test seam mirrors the captured raw native PTY into the real OutputBuffer;
this does not claim a public Engine Spawn or native-app route proof.
"""
import argparse
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('test_binary')
parser.add_argument('codex')
parser.add_argument('scenario', choices=['question', 'permission', 'error'])
args = parser.parse_args()
from pathlib import Path
import tempfile, os, json, subprocess, http.server, threading, pty, fcntl, termios, struct, signal, select, time
import sys
mode = args.scenario
root = Path(tempfile.mkdtemp(prefix='pp-codex-native-')).resolve()
home = root / 'home'
config = root / 'config'
repo = root / 'repo'
for p in [home, config, repo]:
    p.mkdir()

class Handler(http.server.BaseHTTPRequestHandler):

    def do_POST(self):
        body = self.rfile.read(int(self.headers.get('Content-Length', 0)))
        (root / 'request-shape.json').write_text(json.dumps({'path': self.path, 'length': len(body)}))
        request = json.loads(body)
        (root / 'request-metadata.json').write_text(json.dumps({'tools': [t.get('name') for t in request.get('tools', [])]}))
        time.sleep(2)
        if mode == 'error':
            self.send_response(400)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            self.wfile.write(b'{"error":{"message":"sandbox failure","type":"invalid_request_error"}}')
            return
        events = [{'type': 'response.created', 'response': {'id': 'resp_probe'}}, {'type': 'response.output_item.done', 'item': {'type': 'message', 'role': 'assistant', 'id': 'msg_probe', 'content': [{'type': 'output_text', 'text': 'Native probe done.'}]}}, {'type': 'response.completed', 'response': {'id': 'resp_probe', 'usage': {'input_tokens': 10, 'output_tokens': 5, 'total_tokens': 15}}}]
        if mode in ['permission', 'question'] and (not any((x.get('type') == 'function_call_output' for x in request.get('input', []) if isinstance(x, dict)))):
            arguments = {'cmd': 'printf native-proof', 'sandbox_permissions': 'require_escalated', 'justification': 'Approve the isolated native probe?'} if mode == 'permission' else {'questions': [{'id': 'probe', 'header': 'Probe', 'question': 'Native exact question?', 'options': [{'label': 'First', 'description': 'First choice'}, {'label': 'Second', 'description': 'Second choice'}]}]}
            events[1] = {'type': 'response.output_item.done', 'item': {'type': 'function_call', 'call_id': 'call_native_' + mode, 'name': 'exec_command' if mode == 'permission' else 'request_user_input', 'arguments': json.dumps(arguments)}}
        data = ''.join(('event: ' + e['type'] + '\ndata: ' + json.dumps(e) + '\n\n' for e in events)).encode()
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.send_header('Content-Length', str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, *a):
        pass
server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
threading.Thread(target=server.serve_forever, daemon=True).start()
(config / 'config.toml').write_text(f'model = "gpt-5.4"\nmodel_provider = "fixture"\napproval_policy = "on-request"\nsandbox_mode = "read-only"\n[model_providers.fixture]\nname = "Isolated fixture"\nbase_url = "http://127.0.0.1:{server.server_port}/v1"\nwire_api = "responses"\nrequires_openai_auth = false\n[projects."{repo}"]\ntrust_level = "trusted"\n')
env = {'HOME': str(home), 'CODEX_HOME': str(config), 'PATH': '/opt/homebrew/bin:/usr/bin:/bin', 'TERM': 'xterm-256color', 'SHELL': '/bin/sh', 'LANG': 'en_US.UTF-8'}
(a, b) = pty.openpty()
fcntl.ioctl(b, termios.TIOCSWINSZ, struct.pack('HHHH', 40, 120, 0, 0))
p = subprocess.Popen([args.codex, '--no-daemon', '--no-alt-screen'], cwd=repo, env=env, stdin=b, stdout=b, stderr=b, start_new_session=True)
os.close(b)
output = bytearray()
stream = open(root / 'stream.bin', 'wb', buffering=0)
context = root / 'observer-context.json'
context.write_text(json.dumps({'cwd': str(repo), 'pid': p.pid, 'expected': mode}))
observer_log = open(root / 'observer-test.log', 'w')
observer = subprocess.Popen([args.test_binary, '--exact', 'signals::codex::tests::native_interactive_observer_probe', '--ignored', '--nocapture'], env={**os.environ, 'PUREPOINT_CODEX_PROBE_CONTEXT': str(context)}, stdout=observer_log, stderr=subprocess.STDOUT)
try:
    for stage in range(22):
        deadline = time.monotonic() + 0.7
        while time.monotonic() < deadline:
            if select.select([a], [], [], 0.1)[0]:
                try:
                    chunk = os.read(a, 65536)
                    output.extend(chunk)
                    stream.write(chunk)
                    if b'\x1b[6n' in chunk:
                        os.write(a, b'\x1b[1;1R')
                except OSError:
                    break
        if stage == 1 and mode == 'question':
            os.write(a, b'/plan\r')
        if stage == 3:
            os.write(a, b'Native test hello')
        if stage == 4:
            os.write(a, b'\r')
        if stage == 12:
            os.write(a, b'y\r' if mode == 'permission' else b'\r')
        if stage == 13 and mode == 'question':
            os.write(a, b'\r')
    observer.wait(timeout=40)
    observer_log.close()
    assert observer.returncode == 0, (root / 'observer-test.log').read_text()
    print('root', root, 'PID', p.pid, 'request', (root / 'request-shape.json').exists(), flush=True)
    (root / 'terminal.bin').write_bytes(output)
finally:
    if observer.poll() is None:
        observer.terminate()
        observer.wait(timeout=3)
    observer_log.close()
    stream.close()
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
