#!/usr/bin/env python3
"""Actual mobile model and pinned TLS/WSS against two isolated Mac bridge fixtures."""
import os
from pathlib import Path
import subprocess
import tempfile
root = Path(__file__).resolve().parent.parent
with tempfile.TemporaryDirectory(prefix='pg-machine-network-', dir='/tmp') as directory:
    temp = Path(directory)
    helpers = (root / 'verification/MachineChecks.swift').read_text().split('@main struct MachineChecks')[0]
    (temp / 'Model.swift').write_text((root / 'PurePoint/ChatModel.swift').read_text() + '\n' + helpers + '\n' + (root / 'verification/NetworkMachineChecks.swift').read_text())
    subprocess.run(['swiftc', '-parse-as-library', '-strict-concurrency=complete', '-warnings-as-errors', str(root / 'PurePoint/ChatDomain.swift'), str(root / 'PurePoint/PairingSecret.swift'), str(temp / 'Model.swift'), '-o', str(temp / 'checks')], check=True)
    fixture = subprocess.Popen(['node', str(root / 'verification/machine-fixture.js'), directory], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    try:
        if fixture.stdout.readline().strip() != 'ready': raise RuntimeError('Fixture did not become ready')
        env = os.environ.copy(); env['CFFIXED_USER_HOME'] = directory
        subprocess.run([str(temp / 'checks'), directory], env=env, check=True, timeout=35)
    finally:
        if fixture.poll() is None:
            try:
                fixture.stdin.write('stop\n')
                fixture.stdin.flush()
                fixture.wait(timeout=10)
            except (BrokenPipeError, subprocess.TimeoutExpired):
                fixture.kill()
                fixture.wait()
