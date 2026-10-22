#!/usr/bin/env python3
"""Exercise the real deferred scroll coordinator without app build phases."""
from pathlib import Path
import subprocess
import tempfile

app = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="pi-transcript-scroll-") as folder:
    executable = Path(folder) / "checks"
    subprocess.run([
        "xcrun", "swiftc", "-parse-as-library", "-strict-concurrency=complete",
        "-warnings-as-errors", "-swift-version", "5",
        str(app / "purepoint-macos/Views/Chat/PiTranscriptScrollCoordinator.swift"),
        str(app / "verification/PiTranscriptScrollChecks.swift"),
        "-o", str(executable),
    ], check=True)
    subprocess.run([str(executable)], check=True, timeout=30)
