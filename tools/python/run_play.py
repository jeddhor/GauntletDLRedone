#!/usr/bin/env python3
"""Launch the INTERIM native playtest (Yellow Sorceress; The Poison Fields by
default, or any realm's regular levels with --realm) with the native gameplay
simulation (enemies, generators, items, missiles).

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
import struct
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


def audio_config():
    """AUDATPS2.ROM, every AUDIO/*.VBK bank and STREAMS/*.ADS music, by verified identity."""
    proposed = json.loads(pathlib.Path('private/reports/wad-independent.json').read_text())
    names = sorted({n.upper() for e in proposed['entries'] for n in e['proposed_names']})
    banks = [n for n in names if n.startswith('AUDIO/') and n.endswith('.VBK')]
    streams = [n for n in names if n.startswith('STREAMS/') and n.endswith('.ADS')]
    rom, = resolve(['AUDIO/AUDATPS2.ROM'])
    return dict(rom=str(rom),
                banks={n[6:-4]: str(p) for n, p in zip(banks, resolve(banks))},
                streams={n[8:]: str(p) for n, p in zip(streams, resolve(streams))})


REALMS = {'A': (1, 'CASTLE'), 'B': (2, 'MOUNT'), 'C': (3, 'DESERT'), 'D': (4, 'FOREST'), 'E': (5, 'TEMPLE'),
          'F': (6, 'HELL'), 'G': (7, 'TOWN'), 'H': (8, 'BATTLE'), 'I': (9, 'ICE'), 'J': (10, 'DREAM'),
          'K': (11, 'SKY')}  # ELF worlddatas 0x23deb0


def levl_records(wdata):
    """LEVL records (0x10c each): (index, code, boss_type +0x44)."""
    directory, count = struct.unpack_from('<II', wdata, 0)
    for i in range(count):
        tag, off, n, _ = struct.unpack_from('<4I', wdata, directory + 16 * i)
        if tag.to_bytes(4, 'big') == b'LEVL':
            return [(k, wdata[off + 0x10c * k + 8: off + 0x10c * k + 12].split(b'\0')[0].decode(),
                     struct.unpack_from('<i', wdata, off + 0x10c * k + 0x44)[0]) for k in range(n)]
    raise ValueError('WDATA without LEVL')


def known_names():
    proposed = json.loads(pathlib.Path('private/reports/wad-independent.json').read_text())
    return {n.upper() for e in proposed['entries'] for n in e['proposed_names']}


# LoadGauntWorldStart 0x163ec0: critter enemy types → CRITTER/*.WAD.
CRITTER_FILES = {0x21: 'GENERAL', 0x22: 'DRAGON', 0x23: 'CHIMERA', 0x24: 'DJINN', 0x25: 'DRIDER', 0x26: 'PBOSS',
                 0x27: 'YETI', 0x28: 'WRAITH', 0x29: 'LICH', 0x2a: 'SKORNE1', 0x2b: 'SKORNE2', 0x2c: 'GARM'}


def critter_files(kinds, world, known):
    """[{wad, anim}] for the critter enemy types of a level. Boss models are
    "monsters/<DESC name>" (GetCritterDesc); golem/gargoyle/general model
    paths are not resolved yet (anim omitted)."""
    out = []
    for k in sorted(set(kinds)):
        if k == 0x1d:
            name = {9: 'GOLEMI', 6: 'GOLEMF'}.get(world, 'GOLEM')
        elif k == 0x20:
            continue  # gar_<EnemyData name>.wad: not resolved yet
        elif k in CRITTER_FILES:
            name = CRITTER_FILES[k]
        else:
            continue
        wad, = resolve([f'CRITTER/{name}.WAD'])
        b = wad.read_bytes()
        directory, count = struct.unpack_from('<II', b, 0)
        desc = None
        for i in range(count):
            tag, off, n, _ = struct.unpack_from('<4I', b, directory + 16 * i)
            if tag.to_bytes(4, 'big') == b'DESC' and n:
                desc = b[off:off + 16].split(b'\0')[0].decode().upper()
                kind = struct.unpack_from('<h', b, off + 0x20)[0]
        entry = dict(wad=str(wad))
        if desc and kind not in (3, 7, 8) and f'MONSTERS/{desc}/ANIM.PS2' in known:
            entry['anim'] = str(resolve([f'MONSTERS/{desc}/ANIM.PS2'])[0])
        out.append(entry)
    return out


def level_kinds(wdata_path, index):
    """GetEnemyTypes kinds of one LEVL record (`gdl_tools level-enemies`)."""
    tmp = CONFIG.with_name(f'play-enemies-{index}.json')
    if tmp.exists():
        tmp.unlink()
    subprocess.run(['target/release/gdl_tools', 'level-enemies', str(wdata_path), str(index), str(tmp)], check=True,
                   stdout=subprocess.DEVNULL)
    kinds = json.loads(tmp.read_text())['enemy_types']['kind']
    tmp.unlink()
    return kinds


def realm_monsters(wdata_path, indices, known):
    """(type, prefix) of every enemy type the realm's levels load (GetEnemyTypes
    via `gdl_tools level-enemies`; prefixes from the ELF enemy_table 0x22c7a0)."""
    from elf_read import read as elf_read
    kinds = set()
    for k in indices:
        kinds |= set(level_kinds(wdata_path, k))
    elf = ELF.read_bytes()
    out = {}
    for k in range(44):
        rec = elf_read(elf, 0x22c7a0 + 0x24 * k, 0x24)
        t = int.from_bytes(rec[:4], 'little', signed=True)
        prefix = rec[0x14:0x24].split(b'\0')[0].decode()
        if t in kinds and t >= 0 and f'MONSTERS/{prefix}/ANIM.PS2' in known:
            out[t] = prefix
    return sorted(out.items())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--virtual', action='store_true', help='run inside the authenticated Xvfb session')
    parser.add_argument('--realm', default='G', choices=sorted(REALMS), help='realm letter (G: The Poison Fields)')
    parser.add_argument('--level', help='first level code, e.g. A2 (default: the realm\'s first level)')
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

    world, wad = REALMS[args.realm]
    wdata, rig, clips = resolve([f'WDATA/{wad}.WAD', 'PLAYERS/SOR/YEL00/ANIM.PS2', 'PLAYERS/SOR/ANIM/ANIM.PS2'])
    known = known_names()
    # Every level in level-code order; the boss arena last (the critter engine
    # is ported for single-type bosses; the chimera's heads are not).
    records = sorted(levl_records(wdata.read_bytes()), key=lambda r: (0 <= r[2], int(r[1][1:])))
    if args.level:
        records = [r for r in records if r[1] >= args.level.upper()] or records
    packages = dict(POWERUPS=package('POWERUPS'), WEAPONS=package('WEAPONS'), SFXYEL=package('PLAYERS/SOR/SFXYEL'))
    levels = []
    for index, code, _ in records:
        # HYPOTHESIS: the level's own ITEMS package and the realm's are both loaded.
        items = [f'LEVEL{x}' for x in (code, args.realm) if f'ITEMS/LEVEL{x}/ANIM.PS2' in known]
        for name in items:
            packages.setdefault(f'ITEMS_{name[5:]}', package(f'ITEMS/{name}'))
        levels.append(dict(name=code, index=index, worlds=str(resolve([f'LEVELS/LEVEL{code}/WORLDS.PS2'])[0]),
                           level=package(f'LEVELS/LEVEL{code}', anim=False), items=[f'ITEMS_{n[5:]}' for n in items],
                           critters=critter_files(level_kinds(wdata, index), world, known)))
    monsters = realm_monsters(wdata, [r[0] for r in records], known)
    for _, prefix in monsters:
        packages[prefix] = package(f'MONSTERS/{prefix}')
    config = dict(
        sim_start=str(START), game_start=str(GAME_START), elf=str(ELF), wdata=str(wdata), levels=levels, world=world,
        # Realm label: the project's name for G, else the WDATA file name.
        title='The Poison Fields' if args.realm == 'G' else f'{wad.title()} realm ({args.realm})',
        character=package('PLAYERS/SOR/YEL00', anim=False),
        character_rig=str(rig), character_clips=str(clips), packages=packages,
        monsters=[dict(type=t, package=p) for t, p in monsters],
        audio=audio_config())
    CONFIG.write_text(json.dumps(config, indent=1) + '\n')
    env = os.environ.copy()
    if args.virtual:
        from virtual_display import environment
        env = environment()
        env['GDL_NO_AUDIO'] = '1'  # automated runs must not play on the host's speakers
    command = ['target/release/gdl_play', str(CONFIG)]
    raise SystemExit(subprocess.run(command, env=env).returncode)


if __name__ == '__main__':
    main()
