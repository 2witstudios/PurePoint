#!/usr/bin/env python3
"""Public-only Engine QA. Reuses the exact local fake-provider class from CLI probes."""
import argparse, ast, base64, hashlib, http.server, json, os, signal, socket, subprocess, tempfile, threading, time, urllib.request
from pathlib import Path
from signal_probe_support import source_state_sha256, stop_process
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('tool', choices=['claude', 'codex', 'opencode'])
parser.add_argument('scenario', choices=['question', 'permission', 'error', 'interrupt', 'permission_deny', 'question_cancel', 'resume', 'resume_pending', 'worktree', 'question_hold', 'image'])
parser.add_argument('cli')
parser.add_argument('--runner', required=True, help='Exact candidate owned-native-signals.rs executable; compile against cargo compiler-artifact paths')
args = parser.parse_args()
if args.scenario == 'resume_pending' and args.tool != 'codex':
    parser.error('resume_pending checks the Codex interrupted-history regression')
if args.scenario == 'image' and args.tool == 'opencode':
    parser.error('image checks the Claude/Codex native JSONL record regression')
mode = {'resume_pending': 'permission', 'permission_deny': 'permission', 'question_cancel': 'question', 'question_hold': 'question'}.get(args.scenario, args.scenario)
runner_path = Path(args.runner).resolve()
provenance = json.loads((runner_path.parent / 'provenance.json').read_text())
assert provenance['runner_sha256'] == hashlib.sha256(runner_path.read_bytes()).hexdigest(), 'Runner differs from recorded compiler artifacts'
assert provenance['source_sha'] == subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip(), 'Build the runner at this candidate'
assert provenance['source_diff_sha256'] == source_state_sha256(Path.cwd()), 'Source changed after runner build'
root = Path(tempfile.mkdtemp(prefix='ppqa-', dir='/tmp')).resolve()
home = root / 'home'
config = root / 'config'
repo = root / 'project'
state = root / 'state'
data = root / 'data'
for directory in [home, config, repo, state, data, root / 'tmp', root / 'cache']:
    directory.mkdir()
helper = Path.cwd() / f'crates/pu-engine/verification/check-{args.tool}-signals.py'
node = next((n for n in ast.parse(helper.read_text()).body if isinstance(n, ast.ClassDef) and n.name == 'Handler'))
fixture_globals = dict(globals())
fixture_globals['args'] = argparse.Namespace(scenario=mode)
exec(compile(ast.Module(body=[node], type_ignores=[]), str(helper), 'exec'), fixture_globals)
Handler = fixture_globals['Handler']
server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
threading.Thread(target=server.serve_forever, daemon=True).start()
env = {'HOME': str(home), 'TMPDIR': str(root / 'tmp'), 'PATH': '/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin', 'SHELL': '/bin/sh', 'TERM': 'xterm-256color', 'LANG': 'en_US.UTF-8'}
if args.tool == 'claude':
    seed = {'hasCompletedOnboarding': True, 'customApiKeyResponses': {'approved': ['sandbox-fixture'], 'rejected': []}, 'projects': {str(repo): {'hasTrustDialogAccepted': True, 'hasCompletedProjectOnboarding': True}}}
    for path in [home / '.claude.json', config / '.claude.json']:
        path.write_text(json.dumps(seed))
    env.update({'CLAUDE_CONFIG_DIR': str(config), 'ANTHROPIC_API_KEY': 'sandbox-fixture', 'ANTHROPIC_BASE_URL': f'http://127.0.0.1:{server.server_port}', 'CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC': '1', 'DISABLE_AUTOUPDATER': '1'})
elif args.tool == 'codex':
    (config / 'config.toml').write_text(f'model = "gpt-5.4"\nmodel_provider = "fixture"\napproval_policy = "on-request"\nsandbox_mode = "read-only"\n[model_providers.fixture]\nname = "Isolated fixture"\nbase_url = "http://127.0.0.1:{server.server_port}/v1"\nwire_api = "responses"\nrequires_openai_auth = false\n[projects."{repo}"]\ntrust_level = "trusted"\n')
    env['CODEX_HOME'] = str(config)
else:
    settings = {'model': 'fixture/chat', 'update': 'disable', 'share': 'disabled', 'permissions': [{'action': '*', 'resource': '*', 'effect': 'allow'}, {'action': 'edit', 'resource': '*', 'effect': 'ask'}], 'providers': {'fixture': {'name': 'Fixture', 'package': '@opencode/ai/providers/openai-compatible', 'settings': {'apiKey': 'sandbox-fixture', 'baseURL': f'http://127.0.0.1:{server.server_port}/v1'}, 'models': {'chat': {'name': 'Native fixture', 'limit': {'context': 32000, 'output': 1000}, 'capabilities': {'tools': True}}}}}}
    env.update({'XDG_STATE_HOME': str(state), 'XDG_CONFIG_HOME': str(config), 'XDG_DATA_HOME': str(data), 'XDG_CACHE_HOME': str(root / 'cache'), 'OPENCODE_DISABLE_AUTOUPDATE': '1', 'OPENCODE_DISABLE_MODELS_FETCH': '1', 'OPENCODE_CONFIG_CONTENT': json.dumps(settings)})
    with socket.socket() as reservation:
        reservation.bind(('127.0.0.1', 0))
        port = reservation.getsockname()[1]
    (config / 'opencode').mkdir()
    path = config / 'opencode/service.json'
    path.write_text(json.dumps({'hostname': '127.0.0.1', 'port': port}))
    path.chmod(384)
if args.scenario == 'worktree':
    subprocess.run(['git', 'init', '-q', str(repo)], env=env, check=True)
    (repo / 'fixture.txt').write_text('Isolated native agent worktree proof\n')
    subprocess.run(['git', '-C', str(repo), 'add', 'fixture.txt'], env=env, check=True)
    subprocess.run(['git', '-C', str(repo), '-c', 'user.name=Native QA', '-c', 'user.email=qa@example.invalid', 'commit', '-qm', 'Isolated fixture'], env=env, check=True)
if args.scenario == 'image':
    import struct, zlib
    width = 1024
    pixels = b''.join(b'\0' + os.urandom(width * 3) for _ in range(width))
    def png_chunk(kind, data):
        return struct.pack('!I', len(data)) + kind + data + struct.pack('!I', zlib.crc32(kind + data) & 0xffffffff)
    (repo / 'noise.png').write_bytes(b'\x89PNG\r\n\x1a\n' + png_chunk(b'IHDR', struct.pack('!IIBBBBB', width, width, 8, 2, 0, 0, 0)) + png_chunk(b'IDAT', zlib.compress(pixels)) + png_chunk(b'IEND', b''))
print('ROOT', root, 'TOOL', args.tool, 'SCENARIO', args.scenario, flush=True)
(root / 'runner-provenance.json').write_text(json.dumps(provenance, indent=2))
(root / 'fixture.json').write_text(json.dumps({'tool': args.tool, 'scenario': args.scenario, 'provider_source': str(helper), 'worktree_source': subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip(), 'runner': str(Path(args.runner).resolve())}))
log = open(root / 'runner.log', 'w')
process = None
try:
    process = subprocess.Popen([args.runner, str(root), args.tool, args.cli, args.scenario], env=env, stdout=log, stderr=subprocess.STDOUT)
    code = process.wait(timeout=200 if args.scenario.startswith('resume') else 100)
    log.close()
    print('RUNNER_EXIT', code, flush=True)
    print((root / 'runner.log').read_text()[-3000:], flush=True)
    assert code == 0, root
    if args.scenario == 'image':
        directory = config / ('projects' if args.tool == 'claude' else 'sessions')
        records = [line for path in directory.rglob('*.jsonl') for line in path.read_text().splitlines()]
        def image_blocks(value):
            if isinstance(value, dict):
                return int(value.get('type') in ['image', 'input_image']) + sum(image_blocks(v) for v in value.values())
            return sum(map(image_blocks, value)) if isinstance(value, list) else 0
        images = sum(image_blocks(json.loads(line)) for line in records)
        maximum = max((len(line.encode('utf-8')) for line in records), default=0)
        assert images > 0, 'Native tool did not produce image content'
        if args.tool == 'claude':
            assert maximum > 1024 * 1024, 'Native large-image regression was not exercised'
        (root / 'native-image-checks.json').write_text(json.dumps({'passed': True, 'native_image_blocks': images, 'max_record_bytes': maximum}, indent=2))
    if args.scenario in ['permission', 'question', 'permission_deny', 'question_cancel', 'question_hold']:
        if args.tool == 'opencode':
            results = [{'output': r, 'is_error': False} for r in json.loads((root / 'native-tool-results.json').read_text())]
        else:
            results = []
            directory = config / ('projects' if args.tool == 'claude' else 'sessions')
            for path in directory.rglob('*.jsonl'):
                for line in path.read_text().splitlines():
                    item = json.loads(line)
                    if args.tool == 'codex':
                        payload = item.get('payload', {})
                        if payload.get('type') == 'function_call_output' and payload.get('call_id') == 'call_native_' + mode:
                            results.append({'output': payload.get('output', ''), 'is_error': False})
                    else:
                        content = item.get('message', {}).get('content', [])
                        if isinstance(content, list):
                            results.extend(({'output': c.get('content', ''), 'is_error': c.get('is_error', False)} for c in content if isinstance(c, dict) and c.get('type') == 'tool_result' and (c.get('tool_use_id') == 'toolu_native_question')))
        if args.scenario in ['permission_deny', 'question_cancel']:
            assert any(('reject' in str(r['output']).lower() or 'aborted' in str(r['output']).lower() for r in results)), 'No exact native tool rejection result'
            if args.scenario == 'permission_deny' and args.tool in ['claude', 'opencode']:
                assert not (repo / 'native-file.txt').exists(), 'Denied native write was actually executed'
        elif args.scenario == 'permission':
            if args.tool in ['claude', 'opencode']:
                expected = 'sandbox permission proof' if args.tool == 'claude' else 'native permission proof'
                assert (repo / 'native-file.txt').read_text() == expected, 'Approved native write did not execute correctly'
            else:
                assert any(('Process exited with code 0' in r['output'] and 'native-proof' in r['output'] for r in results)), 'Approved command did not execute'
        elif args.tool == 'codex':
            answers = []
            for r in results:
                try:
                    value = json.loads(r['output'])
                except json.JSONDecodeError:
                    continue
                if isinstance(value, dict):
                    answers.append(value.get('answers', {}).get('probe', {}).get('answers'))
            assert ['First'] in answers, 'Expected native question answer missing'
        else:
            assert any((not r['is_error'] and '"First"' in str(r['output']) for r in results)), 'Expected native question answer missing'
        (root / 'native-response-checks.json').write_text(json.dumps({'passed': True, 'scenario': args.scenario, 'exact_results': results}, indent=2))
    print('NATIVE_RESPONSE_SEMANTICS_PASS', flush=True)
    pid_path = root / 'owned-pid.txt'
    if pid_path.exists():
        pid = int(pid_path.read_text())
        deadline = time.monotonic() + 3
        while True:
            try:
                os.kill(pid, 0)
            except ProcessLookupError:
                break
            if time.monotonic() > deadline:
                raise AssertionError(f'owned PID {pid} still alive after cleanup')
            time.sleep(0.1)
    print('OWNED_CLI_CLEANUP_PASS', flush=True)
finally:
    try:
        if process is not None:
            stop_process(process)
    finally:
        log.close()
        try:
            registration = state / 'opencode/service.json'
            if args.tool == 'opencode' and registration.exists():
                try:
                    record = json.loads(registration.read_text())
                    header = 'Basic ' + base64.b64encode(('opencode:' + record.get('password', '')).encode()).decode()
                    request = urllib.request.Request(record['url'].rstrip('/') + '/api/info', headers={'Authorization': header})
                    health = json.load(urllib.request.urlopen(request, timeout=1))
                    if health.get('pid') == record['pid'] and health.get('version') == '2.0.28':
                        os.kill(record['pid'], signal.SIGTERM)
                except (OSError, ValueError, KeyError):
                    pass
        finally:
            server.shutdown()
