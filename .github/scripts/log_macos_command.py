"""Keep raw Xcode output and sample stalled CI processes before step cancellation."""

import os
from pathlib import Path
import shutil
import subprocess
import sys
import threading
import time


def run_logged(command, folder, interval=60, stall_seconds=120):
    folder = Path(folder)
    folder.mkdir(parents=True, exist_ok=True)
    stopped = threading.Event()
    last_output = time.monotonic()

    def monitor():
        last_sample = 0
        while not stopped.wait(interval):
            stamp = time.strftime("%Y%m%d-%H%M%S")
            snapshot = subprocess.run(
                ["ps", "-axo", "pid,ppid,etime,%cpu,command"],
                capture_output=True, text=True, timeout=10,
            ).stdout
            (folder / f"processes-{stamp}.txt").write_text(snapshot)
            silent = time.monotonic() - last_output
            print(f"CI heartbeat: {silent:.0f}s since Xcode output", flush=True)
            if silent < stall_seconds or time.monotonic() - last_sample < stall_seconds:
                continue
            last_sample = time.monotonic()
            sampler = shutil.which("sample")
            if not sampler:
                continue
            # Capture compiler/build-service/test-host stacks, never unrelated processes.
            names = {"swift-frontend", "SWBBuildService", "xcodebuild", "PurePoint"}
            for row in snapshot.splitlines()[1:]:
                fields = row.split(None, 4)
                if len(fields) < 5:
                    continue
                name = Path(fields[4].split()[0]).name
                if name not in names and not name.endswith("Tests-Runner"):
                    continue
                try:
                    subprocess.run(
                        [sampler, fields[0], "3", "-file", str(folder / f"sample-{fields[0]}-{stamp}.txt")],
                        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10,
                    )
                except subprocess.TimeoutExpired:
                    pass

    with (folder / "xcode.log").open("wb") as log:
        process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        watcher = threading.Thread(target=monitor, daemon=True)
        watcher.start()
        try:
            while chunk := os.read(process.stdout.fileno(), 65536):
                last_output = time.monotonic()
                log.write(chunk)
                log.flush()
                sys.stdout.buffer.write(chunk)
                sys.stdout.buffer.flush()
            return process.wait()
        finally:
            stopped.set()
            watcher.join(timeout=1)
            process.stdout.close()


if __name__ == "__main__":
    raise SystemExit(run_logged(sys.argv[2:], sys.argv[1]))
