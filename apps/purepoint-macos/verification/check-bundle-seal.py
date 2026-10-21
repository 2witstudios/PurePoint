#!/usr/bin/env python3
"""Bundle seal checks against throwaway ad-hoc signed bundles; never builds the app."""
from pathlib import Path
import subprocess
import tempfile
root = Path(__file__).resolve().parents[1]
source = root / 'purepoint-macos'
with tempfile.TemporaryDirectory(prefix='bundle-seal-', dir='/tmp') as folder:
    binary = str(Path(folder) / 'checks')
    subprocess.run(['swiftc', '-parse-as-library', '-strict-concurrency=complete', '-warnings-as-errors',
                    str(source / 'Services/PiPairingSecret.swift'), str(source / 'Services/BundleSeal.swift'),
                    str(root / 'verification/BundleSealChecks.swift'), '-o', binary], check=True)
    subprocess.run([binary], check=True, timeout=60)
