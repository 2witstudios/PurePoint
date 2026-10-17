"""Shared source provenance and bounded process cleanup for native QA probes."""
import hashlib
import subprocess

SOURCE_PATHS = ['crates', 'Cargo.toml', 'Cargo.lock', '.cargo']


def source_state_sha256(repo):
    value = hashlib.sha256(subprocess.check_output(
        ['git', 'diff', '--binary', 'HEAD', '--', *SOURCE_PATHS], cwd=repo,
    ))
    names = subprocess.check_output(
        ['git', 'ls-files', '--others', '--exclude-standard', '-z', '--', *SOURCE_PATHS],
        cwd=repo,
    )
    for name in sorted(filter(None, names.split(b'\0'))):
        contents = (repo / name.decode('utf-8', errors='surrogateescape')).read_bytes()
        value.update(len(name).to_bytes(8, 'big'))
        value.update(name)
        value.update(len(contents).to_bytes(8, 'big'))
        value.update(contents)
    return value.hexdigest()


def stop_process(process, timeout=3):
    if process.poll() is not None:
        return
    try:
        process.terminate()
    except ProcessLookupError:
        return
    try:
        process.wait(timeout=timeout)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=timeout)
