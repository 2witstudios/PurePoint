#!/usr/bin/env python3
"""Import immutable git blobs at the pinned revision; never modify the source checkout."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

PIN = 'a130ad906a0cac5e9183e5b9d0bf4b0b78f8921f'
ROOT = Path(__file__).resolve().parents[1]


def git(source, *args):
    return subprocess.check_output(['git', '-C', str(source), *args])


def metadata(content):
    front = content.decode('utf-8').split('---', 2)[1]
    name = re.search(r'^name: (.+)$', front, re.M).group(1).strip()
    match = re.search(r'^description: (.+)((?:\n[ \t]+[^\n]*)*)', front, re.M)
    description = match.group(1).strip()
    if description in ('>', '|', '>-', '|-'):
        description = ' '.join(line.strip() for line in match.group(2).splitlines())
    if not re.fullmatch(r'[a-z0-9]+(?:-[a-z0-9]+)*', name) or not description:
        raise ValueError('Invalid upstream skill metadata')
    return name, description


def adapter(relative, content):
    name, description = metadata(content)
    if name.startswith('pagespace-'):
        dependency = 'PageSpace access needs an available authenticated CLI, SDK or configured MCP server; none is bundled.'
    elif name in ('review', 'triage'):
        dependency = 'Optional AIDD guidance is not bundled; use repository guidance and report that gap.'
    elif name in ('task', 'plan', 'orchestrate', 'pr', 'handoff'):
        dependency = 'PageSpace/GitHub/PurePoint integrations require available tools and credentials; report missing capabilities.'
    else:
        dependency = 'Usable with native Pi tools; optional integrations are checked when needed.'
    # The catalog is Pi's own SKILL.md frontmatter, never a second skill registry.
    text = f'''---
name: {name}
description: {json.dumps(description + ' ' + dependency, ensure_ascii=False)}
---

# {name} in Pi

Read [Pi dependency adapters](../../PI-ADAPTERS.md), then read the complete
[pinned original workflow](../../source/{relative}) before using this skill.
The original is the workflow authority; this file adapts only host capabilities.
Resolve references in the original against its own directory, not this adapter.
Original license and attribution are in ../../source/LICENSE and ../../source/NOTICE.md.

{dependency}

Use `/skill:{name}` for native Pi invocation. Slash commands in the original
refer to skills or external workflows; they are not automatically installed tools.
'''
    return name, text.encode('utf-8')


def imported_files(source):
    resolved = git(source, 'rev-parse', PIN + '^{commit}').decode().strip()
    if resolved != PIN:
        raise ValueError('Source commit does not match pinned revision')
    files = {}
    for row in git(source, 'ls-tree', '-rz', '--full-tree', PIN).split(b'\0'):
        if not row:
            continue
        header, raw_path = row.split(b'\t', 1)
        mode, kind, oid = header.decode().split()
        name = raw_path.decode('utf-8')
        if kind != 'blob' or mode not in ('100644', '100755') or '..' in Path(name).parts or Path(name).is_absolute():
            raise ValueError('Unsafe or unsupported source tree entry: ' + name)
        files[name] = (mode[-3:], git(source, 'cat-file', 'blob', oid))
    return files


def regenerate(source, output):
    files = imported_files(source)
    # Stage both generated trees before replacing anything; dirty source files are ignored.
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.import-', dir=output) as folder:
        staged = Path(folder)
        proof = {'schemaVersion': 1, 'repository': 'https://github.com/2witstudios/pointguard',
                 'commit': PIN, 'files': {}}
        names = set()
        for name, (mode, content) in sorted(files.items()):
            destination = staged / 'source' / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(content)
            destination.chmod(int(mode, 8))
            proof['files'][name] = {'sha256': hashlib.sha256(content).hexdigest(), 'mode': mode}
            if name.startswith(('skills/', 'codex-skills/')) and name.endswith('/SKILL.md'):
                skill_name, text = adapter(name, content)
                if skill_name in names:
                    raise ValueError('Duplicate upstream skill name: ' + skill_name)
                names.add(skill_name)
                path = staged / 'skills' / skill_name / 'SKILL.md'
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(text)
        if not names:
            raise ValueError('No upstream workflows found')
        for name in ('source', 'skills'):
            destination = output / name
            if destination.is_symlink():
                raise ValueError('Refusing symlink output: ' + str(destination))
            if destination.exists():
                shutil.rmtree(destination)
            shutil.move(str(staged / name), destination)
        for name in ('LICENSE', 'NOTICE.md'):
            (output / name).write_bytes(files[name][1])
        (output / 'source-manifest.json').write_text(json.dumps(proof, indent=2, ensure_ascii=False) + '\n')


def verify(root=ROOT):
    proof = json.loads((root / 'source-manifest.json').read_text())
    if proof['schemaVersion'] != 1 or proof['commit'] != PIN:
        raise ValueError('Unsupported provenance or revision')
    source = root / 'source'
    if source.is_symlink() or any(p.is_symlink() for p in source.rglob('*')):
        raise ValueError('Source tree contains a symlink')
    actual = {p.relative_to(source).as_posix() for p in source.rglob('*') if p.is_file()}
    if actual != set(proof['files']):
        raise ValueError('Source tree inventory mismatch')
    expected_skills = set()
    for name, entry in proof['files'].items():
        if Path(name).is_absolute() or '..' in Path(name).parts:
            raise ValueError('Unsafe provenance path')
        path = source / name
        if path.is_symlink() or hashlib.sha256(path.read_bytes()).hexdigest() != entry['sha256']:
            raise ValueError('Source hash mismatch: ' + name)
        if path.stat().st_mode & 0o777 != int(entry['mode'], 8):
            raise ValueError('Source mode mismatch: ' + name)
        if name.startswith(('skills/', 'codex-skills/')) and name.endswith('/SKILL.md'):
            skill_name, expected = adapter(name, path.read_bytes())
            adapter_path = root / 'skills' / skill_name / 'SKILL.md'
            expected_skills.add(adapter_path)
            if adapter_path.read_bytes() != expected:
                raise ValueError('Adapter differs from reproducible import: ' + skill_name)
    skills = root / 'skills'
    if skills.is_symlink() or any(p.is_symlink() for p in skills.rglob('*')):
        raise ValueError('Native skill tree contains a symlink')
    if {p for p in skills.rglob('*') if p.is_file()} != expected_skills:
        raise ValueError('Native skill inventory mismatch')
    for name in ('LICENSE', 'NOTICE.md'):
        if (root / name).read_bytes() != (source / name).read_bytes():
            raise ValueError('Package attribution mismatch: ' + name)
    print(f'Verified {len(actual)} pinned source files and {len(expected_skills)} native skills')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument('--source', type=Path, help='Explicit git checkout containing the pinned commit')
    group.add_argument('--verify', action='store_true', help='Verify bundled hashes and generated adapters offline')
    parser.add_argument('--output', type=Path, default=ROOT, help='Package root for generated source/skills/provenance')
    args = parser.parse_args()
    if args.source:
        if args.output.resolve() == args.source.resolve() or args.source.resolve() in args.output.resolve().parents:
            parser.error('Output must not be inside the source checkout')
        for generated in ('source', 'skills'):
            if args.source.resolve().is_relative_to(args.output.resolve() / generated):
                parser.error('Generated output must not contain the source checkout')
        regenerate(args.source.resolve(), args.output.resolve())
    verify(args.output.resolve())
