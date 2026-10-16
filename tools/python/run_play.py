#!/usr/bin/env python3
"""Launch the INTERIM native playtest (Yellow Sorceress, The Poison Fields)
with the native gameplay simulation (enemies, generators, items, missiles).

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
GAME_START = pathlib.Path('private/reference/start-states/poison-fields-game-v6.json')
CONFIG = pathlib.Path('private/reports/play-config.json')
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
    if not GAME_START.exists():
        raise ValueError('run tools/python/export_game_state.py first')

    def package(prefix, anim=True):
        names = [f'{prefix}/OBJECTS.PS2', f'{prefix}/TEXDEF.PS2', f'{prefix}/TEXTURES.PS2'] + ([f'{prefix}/ANIM.PS2'] if anim else [])
        paths = resolve(names)
        out = dict(objects=str(paths[0]), texdef=str(paths[1]), textures=str(paths[2]))
        if anim:
            out['anim'] = str(paths[3])
        return out

    wdata, rig, clips = resolve(['WDATA/TOWN.WAD', 'PLAYERS/SOR/YEL00/ANIM.PS2', 'PLAYERS/SOR/ANIM/ANIM.PS2'])
    # The Poison Fields levels G1-G4 (WDATA/TOWN.WAD records 0-3); G5 is the boss level.
    levels = [dict(name=f'G{n}', worlds=str(resolve([f'LEVELS/LEVELG{n}/WORLDS.PS2'])[0]),
                   level=package(f'LEVELS/LEVELG{n}', anim=False)) for n in range(1, 5)]
    config = dict(
        sim_start=str(START), game_start=str(GAME_START), elf=str(ELF), wdata=str(wdata), levels=levels,
        character=package('PLAYERS/SOR/YEL00', anim=False),
        character_rig=str(rig), character_clips=str(clips),
        packages=dict(ZOM=package('MONSTERS/ZOM'), MAG=package('MONSTERS/MAG'), GHO=package('MONSTERS/GHO'),
                      ITEMS=package('ITEMS/LEVELG'),
                      POWERUPS=package('POWERUPS'), WEAPONS=package('WEAPONS'), SFXYEL=package('PLAYERS/SOR/SFXYEL')),
        # Enemy types of The Poison Fields (GetEnemyTypes, town "G" levels 0-3;
        # prefixes from the ELF enemy_table).
        monsters=[dict(type=13, package='ZOM'), dict(type=12, package='MAG'), dict(type=20, package='GHO')])
    CONFIG.write_text(json.dumps(config, indent=1) + '\n')
    env = os.environ.copy()
    if args.virtual:
        from virtual_display import environment
        env = environment()
    command = ['target/release/gdl_play', str(CONFIG)]
    raise SystemExit(subprocess.run(command, env=env).returncode)


if __name__ == '__main__':
    main()
