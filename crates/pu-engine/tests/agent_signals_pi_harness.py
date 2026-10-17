#!/usr/bin/env python3
"""Isolated pi 1.3.0 RPC native UI qualification; no provider/auth or user config.

Usage: python3 ... --pi /isolated/node_modules/.bin/pi --pu target/debug/pu
RPC dialogs qualify event pairing only, never terminal/platform actionability.
"""
import argparse
import json
import os
from pathlib import Path
import select
import socket
import subprocess
import tempfile
import time

parser = argparse.ArgumentParser()
parser.add_argument('--pi', required=True)
parser.add_argument('--pu', required=True)
args = parser.parse_args()
repo = Path(__file__).resolve().parents[3]
extension = repo / 'crates/pu-engine/assets/agent-signals/pi.mjs'
with tempfile.TemporaryDirectory(prefix='pu-pi-proof-', dir='/tmp') as directory:
    root = Path(directory)
    home = root / 'home'
    home.mkdir()
    env = {'HOME': str(home), 'PATH': os.environ['PATH'],
           'PI_CODING_AGENT_DIR': str(home / 'agent'),
           'PU_SIGNAL_SOCKET': str(root / 's'), 'PU_SIGNAL_TOKEN': 'isolated-proof',
           'PU_SIGNAL_HELPER': str(Path(args.pu).resolve())}
    # Match launch readiness; this does not establish an OS scheduling bound.
    subprocess.run([str(Path(args.pu).resolve()), "__agent-signal"], env=env, cwd=root, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True, timeout=.5)
    version = subprocess.check_output([args.pi, '--version'], env=env, cwd=root, timeout=3).decode().strip()
    assert version == '1.3.0', version
    probe = root / 'probe.mjs'
    probe.write_text("export default pi => pi.registerCommand('probe', {handler:async(_,ctx)=>{await ctx.ui.confirm('Probe confirm','Continue?');await ctx.ui.select('Probe select',['a','b']);await ctx.ui.input('Probe input');}});")
    listener = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
    listener.bind(str(root / 's'))
    process = subprocess.Popen([args.pi, '--mode', 'rpc', '--extension', str(extension), '--extension', str(probe)], env=env, cwd=root, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    frames, requests, pending, buffer = [], [], None, b''
    deadline = time.monotonic() + 15
    try:
        process.stdin.write(b'{"id":"proof","type":"prompt","message":"/probe"}\n')
        process.stdin.flush()
        while time.monotonic() < deadline:
            ready, _, _ = select.select([listener, process.stdout], [], [], .1)
            for source in ready:
                if source is listener:
                    frame = json.loads(listener.recv(16384))
                    assert frame['token'] == 'isolated-proof'
                    event = frame['payload']
                    assert event['native_pid'] == process.pid
                    frames.append(event)
                else:
                    chunk = os.read(process.stdout.fileno(), 65536)
                    assert chunk, 'pi exited before completing probe'
                    buffer += chunk
                    while b'\n' in buffer:
                        line, buffer = buffer.split(b'\n', 1)
                        message = json.loads(line)
                        if message.get('type') == 'extension_ui_request':
                            requests.append({'method': message['method'], 'id': message['id']})
                            pending = message
                        if message.get('id') == 'proof' and message.get('type') == 'response':
                            assert message['success'], message
            # Wait until the observer actually captured this open UI before answering it.
            starts = [event for event in frames if event['type'] == 'ui_prompt_start']
            if pending and len(starts) >= len(requests):
                process.stdin.write((json.dumps({'type':'extension_ui_response','id':pending['id'],'confirmed':True,'value':'a'})+'\n').encode())
                process.stdin.flush()
                pending = None
            ends = [event for event in frames if event['type'] == 'ui_prompt_end']
            if len(ends) == 3:
                break
        starts = [event for event in frames if event['type'] == 'ui_prompt_start']
        ends = [event for event in frames if event['type'] == 'ui_prompt_end']
        assert len(starts) == len(ends) == 3, {"frames":frames,"requests":requests,"stderr":process.stderr.read1(65536).decode() if select.select([process.stderr],[],[],0)[0] else ""}
        assert [event['request_id'] for event in starts] == [event['request_id'] for event in ends]
        assert [event['kind'] for event in starts] == ['confirm','select','input']
        assert len({event['session_id'] for event in frames}) == 1
        assert [event['sequence'] for event in frames] == list(range(1, len(frames)+1)), frames
        print(json.dumps({'version':version,'mode':'rpc','auth':'none','home':'isolated temporary directory','native_pid_binding':True,'ui_pairs':3,'terminal_actionability':'NOT RUN','model_lifecycle':'NOT RUN','frames':frames}))
    finally:
        process.terminate()
        try:
            process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=3)
        listener.close()
