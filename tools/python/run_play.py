#!/usr/bin/env python3
"""Launch the INTERIM native playtest (Yellow Sorceress, The Poison Fields).

Inputs are resolved from the verified private extraction by package label and
decoded hash, plus the user's executable and the private start state exported
from the preserved save. Runs on the current desktop by default; --virtual uses
the project's authenticated Xvfb session for automated smoke tests.
"""
import argparse
import hashlib
import json
import os
import pathlib
import subprocess

START = pathlib.Path('private/reference/start-states/poison-fields-sor-v2.json')
ELF = pathlib.Path('private/disc/SLUS_200.47')


def resolve(targets):
    proposed = json.loads(pathlib.Path('private/reports/wad-independent.json').read_text())
    archive = json.loads(pathlib.Path('private/wad/manifest.json').read_text())
    if proposed['source_sha256'] != archive['source_sha256']:
        raise ValueError('name candidates and extraction refer to different archives')
    paths = []
    for target in targets:
        entries = [e for e in proposed['entries'] if target in [n.upper() for n in e['proposed_names']]]
        if len(entries) != 1:
            raise ValueError(f'unknown/ambiguous package label: {target}')
        e = entries[0]
        path = pathlib.Path('private/wad') / archive['entries'][e['index']]['output_file']
        if hashlib.sha256(path.read_bytes()).hexdigest() != e['decoded_sha256']:
            raise ValueError(f'decoded source identity differs: {target}')
        paths.append(path)
    return paths


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--virtual', action='store_true', help='run inside the authenticated Xvfb session')
    args = parser.parse_args()
    source = json.loads(pathlib.Path('profiles/ps2/slus-20047.json').read_text())
    if hashlib.sha256(ELF.read_bytes()).hexdigest() != source['executable']['sha256']:
        raise ValueError('executable identity differs from the supported profile')
    if not START.exists():
        raise ValueError('run tools/python/export_start_state.py first')
    level = resolve([f'LEVELS/LEVELG1/{n}' for n in ('OBJECTS.PS2', 'WORLDS.PS2', 'TEXDEF.PS2', 'TEXTURES.PS2')])
    character = resolve(['PLAYERS/SOR/YEL00/OBJECTS.PS2', 'PLAYERS/SOR/YEL00/ANIM.PS2', 'PLAYERS/SOR/ANIM/ANIM.PS2',
                         'PLAYERS/SOR/YEL00/TEXDEF.PS2', 'PLAYERS/SOR/YEL00/TEXTURES.PS2'])
    env = os.environ.copy()
    if args.virtual:
        from virtual_display import environment
        env = environment()
    command = ['target/release/gdl_play', str(START), str(ELF), *map(str, level), *map(str, character)]
    raise SystemExit(subprocess.run(command, env=env).returncode)


if __name__ == '__main__':
    main()
