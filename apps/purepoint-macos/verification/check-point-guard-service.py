#!/usr/bin/env python3
"""Focused native model checks; never runs Xcode/build phases or touches live state."""
from pathlib import Path
import subprocess
import tempfile
root = Path(__file__).resolve().parents[1]
source = root / 'purepoint-macos'
with tempfile.TemporaryDirectory(prefix='pointguard-service-', dir='/tmp') as folder:
    binary = str(Path(folder) / 'checks')
    subprocess.run(['swiftc', '-parse-as-library', '-strict-concurrency=complete', '-warnings-as-errors',
                    str(source / 'Services/PiPairingSecret.swift'), str(source / 'Models/PointGuardRuntime.swift'),
                    str(root / 'verification/PointGuardServiceChecks.swift'), '-o', binary], check=True)
    subprocess.run([binary], check=True, timeout=30)
subprocess.run(['swiftc', '-typecheck', '-strict-concurrency=complete', '-warnings-as-errors',
                *[str(source / path) for path in ['Models/PiChatDomain.swift', 'Models/PointGuardRuntime.swift',
                'Services/PiPairingSecret.swift', 'Services/PointGuardTailnet.swift',
                'State/PiChatModel.swift', 'State/PointGuardServiceModel.swift']]], check=True)

with tempfile.TemporaryDirectory(prefix='pointguard-service-', dir='/tmp') as folder:
    binary = str(Path(folder) / 'auth-checks')
    # Test-only same-file extension attaches a disposable sleep child; production
    # launch remains bundle-only and cannot adopt an arbitrary process.
    fixture_service = Path(folder) / 'PointGuardServiceModel.swift'
    fixture_service.write_text((source / 'State/PointGuardServiceModel.swift').read_text() + '''
@MainActor extension PointGuardServiceModel {
    func fixtureAttach(_ child: Process) { process = child; ready = true }
}
''')
    paths = ['Models/PiChatDomain.swift' , 'Models/PointGuardRuntime.swift', 'Services/PiPairingSecret.swift',
             'Services/PointGuardTailnet.swift', 'State/PiChatModel.swift', 'State/PointGuardServiceModel.swift']
    subprocess.run(['swiftc', '-parse-as-library', '-strict-concurrency=complete', '-warnings-as-errors',
                    *[str(fixture_service) if path == 'State/PointGuardServiceModel.swift' else str(source / path) for path in paths], str(root / 'verification/PointGuardAuthChecks.swift'),
                    '-o', binary], check=True)
    subprocess.run([binary], check=True, timeout=30)
