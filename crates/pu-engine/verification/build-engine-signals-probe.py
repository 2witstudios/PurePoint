#!/usr/bin/env python3
"""Compile the public Engine proof runner against exact Cargo compiler artifacts.

Does not install binaries, start a daemon, or change Cargo's target directory.
The supplied output directory retains the runner and its provenance record.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('output_directory', type=Path)
args = parser.parse_args()
repo = Path(__file__).resolve().parents[3]
output = args.output_directory.resolve()
output.mkdir(parents=True, exist_ok=True)
source = Path(__file__).with_name('owned-native-signals.rs').resolve()
result = subprocess.run(
    ['cargo', 'build', '-p', 'pu-engine', '--lib', '--message-format=json'],
    cwd=repo, capture_output=True, text=True,
)
(output / 'cargo-artifacts.jsonl').write_text(result.stdout)
(output / 'cargo-build.log').write_text(result.stderr)
result.check_returncode()
names = {'pu_engine', 'pu_core', 'serde', 'serde_json', 'chrono', 'tokio'}
deps = {}
for line in result.stdout.splitlines():
    record = json.loads(line)
    if record.get('reason') == 'compiler-artifact' and record['target']['name'] in names:
        for path in record['filenames']:
            if path.endswith('.rlib'):
                deps[record['target']['name']] = Path(path)
if deps.keys() != names:
    raise RuntimeError(f'Incomplete compiler artifact list: {deps.keys()}')
binary = output / 'owned-native-signals'
command = ['rustc', '--edition=2024', str(source)]
for directory in sorted({path.parent for path in deps.values()}):
    command.extend(['-L', f'dependency={directory}'])
for name, path in deps.items():
    command.extend(['--extern', f'{name}={path}'])
subprocess.run(command + ['-o', str(binary)], cwd=repo, check=True)

def digest(path):
    value = hashlib.sha256()
    with path.open('rb') as file:
        for chunk in iter(lambda: file.read(1024 * 1024), b''):
            value.update(chunk)
    return value.hexdigest()

provenance = {
    'source_sha': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=repo, text=True).strip(),
    'source_diff_sha256': hashlib.sha256(subprocess.check_output(['git', 'diff', 'HEAD', '--', 'crates'], cwd=repo)).hexdigest(),
    'runner_source_sha256': digest(source),
    'runner': str(binary),
    'runner_sha256': digest(binary),
    'artifacts': [{'name': name, 'path': str(path), 'sha256': digest(path)} for name, path in deps.items()],
}
(output / 'provenance.json').write_text(json.dumps(provenance, indent=2) + '\n')
print(binary)
