#!/usr/bin/env python3
"""Isolated real pi PTY question route; does not qualify app notification/device acceptance."""
import argparse
import json
import os
import pathlib
import pty
import select
import shutil
import signal
import struct
import subprocess
import tempfile
import termios
import fcntl
import time

parser = argparse.ArgumentParser()
parser.add_argument('--pi', required=True)
args = parser.parse_args()
with tempfile.TemporaryDirectory(prefix='pu-native-pi-') as directory:
    root = pathlib.Path(directory)
    home = root / 'home'
    home.mkdir(mode=0o700)
    events = root / 'events.jsonl'
    extension = root / 'probe.ts'
    extension.write_text('''import { appendFileSync } from 'node:fs';
export default function(pi: any) {
 const record = (e: any) => appendFileSync(EVENTS, JSON.stringify(e) + '\\n');
 for (const event of ['ui_prompt_start', 'ui_prompt_end']) pi.on(event, record);
 pi.on('session_start', () => record({type: 'probe_ready'}));
 pi.registerCommand('probe', { description: 'Isolated native route proof', handler: async (_: any, ctx: any) => {
   const answer = await ctx.ui.input('SIGNALS exact question request native-123');
   record({ type: 'probe_answer', answer, session: ctx.sessionManager.getSessionId() });
 }});
}
'''.replace('EVENTS', json.dumps(str(events))))
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 40, 120, 0, 0))
    env = {'HOME': str(home), 'PI_CODING_AGENT_DIR': str(home / '.pi'),
           'PATH': os.environ.get('PATH', '/usr/bin:/bin'),
           'TERM': 'xterm-256color', 'LANG': 'en_US.UTF-8'}
    process = subprocess.Popen([args.pi, '--no-session', '--no-extensions', '--extension', str(extension)],
        cwd=root, env=env, stdin=slave, stdout=slave, stderr=slave, start_new_session=True)
    os.close(slave)
    output = bytearray()
    def drain_until(predicate, timeout=10):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if predicate():
                return
            if process.poll() is not None:
                raise AssertionError('Isolated pi exited before route completed')
            if select.select([master], [], [], 0.1)[0]:
                output.extend(os.read(master, 65536))
        raise AssertionError('Timed out waiting for isolated native terminal route: ' + repr(bytes(output[-1000:])))
    def records():
        return [json.loads(line) for line in events.read_text().splitlines()] if events.exists() else []
    try:
        drain_until(lambda: any(e.get('type') == 'probe_ready' for e in records()), timeout=30)
        os.write(master, b'/probe\r')
        drain_until(lambda: b'SIGNALS exact question request native-123' in output)
        drain_until(lambda: any(e.get('type') == 'ui_prompt_start' for e in records()))
        os.write(master, b'native-response-123\r')
        drain_until(lambda: any(e.get('type') == 'probe_answer' for e in records()))
        observed = records()
        answer = next(e for e in observed if e.get('type') == 'probe_answer')
        assert answer['answer'] == 'native-response-123'
        assert answer['session']
        assert [e['type'] for e in observed if e['type'].startswith('ui_prompt_')] == ['ui_prompt_start', 'ui_prompt_end']
        print('PASS: real isolated pi interactive PTY displayed exact question, accepted typed answer, emitted UI start/end and native session identity')
        print('NOT RUN: PurePoint notification tap, SwiftTerm view, badge/sound/device acceptance; actionability remains false')
    finally:
        os.killpg(process.pid, signal.SIGTERM) if process.poll() is None else None
        try:
            process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait(timeout=3)
        os.close(master)
