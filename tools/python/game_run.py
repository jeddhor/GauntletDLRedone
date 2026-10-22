#!/usr/bin/env python3
"""Headless INTERIM native game run for The Poison Fields (levels G1-G4).

Resolves packages from the verified private extraction and runs
`gdl_tools game-run` with an optional scripted interim player. Script format:
LEVELS_HEX:UPDATES,... (e.g. 0:20,2000200:3 = idle 20 updates, attack 3).
Reports are written once (new file) under private/reports/.
"""
import argparse
import os
import json
import pathlib
import subprocess
import sys

# GDL_TOOLS: an alternate gdl_tools binary (A/B comparisons against another build).
TOOLS = os.environ.get('GDL_TOOLS', 'target/release/gdl_tools')

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from run_play import ELF, resolve  # noqa: E402

GAME_START = 'private/reference/start-states/poison-fields-game-v6.json'
SIM_START = 'private/reference/start-states/poison-fields-sor-v2.json'


def level_index(wdata, code):
    import struct
    directory, count = struct.unpack_from('<II', wdata, 0)
    for i in range(count):
        tag, off, n, _ = struct.unpack_from('<4I', wdata, directory + 16 * i)
        if tag.to_bytes(4, 'big') == b'LEVL':
            for k in range(n):
                if wdata[off + 0x10c * k + 8: off + 0x10c * k + 12].split(b'\0')[0].decode() == code:
                    return k
    raise SystemExit(f'no LEVL record {code}')


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('report')
    ap.add_argument('--frames', type=int, default=600)
    ap.add_argument('--level', type=int, default=1, help='level number (1-based)')
    ap.add_argument('--world', default='G', help='world letter (A-K, S); G is The Poison Fields')
    ap.add_argument('--script')
    ap.add_argument('--legend', action='store_true', help='DEBUG: player 0 holds the legend item')
    ap.add_argument('--plevel', help='DEBUG: player 0 level')
    ap.add_argument('--quest', help='DEBUG: QuestItemState 2 at this update')
    ap.add_argument('--boss-kill', help='DEBUG: lethal CritterDamage on the boss at this update')
    ap.add_argument('--replay', help='per-update replay of a reference_trace --player trace')
    ap.add_argument('--pads', help='record-based players: MASK_HEX:UPDATES,... (bit i = joyGetStatus entry i of port 0)')
    ap.add_argument('--potions', help='DEBUG: set player 0 potion count')
    ap.add_argument('--crystals', help='DEBUG: player 0 crystal count N:COUNT')
    ap.add_argument('--boss-health', help='DEBUG: set the boss critter health')
    ap.add_argument('--teleport', help='DEBUG: start player 0 at x,y,z')
    ap.add_argument('--weapon', help='DEBUG: player 0 weapon powerup TYPE:SECONDS (kind 5)')
    ap.add_argument('--fall', help='DEBUG: player 0 drops below the world bottom at this update')
    ap.add_argument('--players', help='players in play (copies of player 0 without input, placed by load_player_geo)')
    ap.add_argument('--turbo', help='DEBUG: player 0 turbo (+0x800)')
    ap.add_argument('--front', action='store_true', help='DEBUG: player 1 stands 2 ahead of player 0 (with --players 2)')
    ap.add_argument('--flyby', action='store_true', help='the attract flyby on this level (no players; old camera)')
    ap.add_argument('--spawn', help='DEBUG: generate_enemy TYPE:LEVEL:AI:UPDATE[:DX:DZ[:DY]] beside player 0 (DY: floor probe height)')
    a = ap.parse_args()
    letters = {'A': (1, 'CASTLE'), 'B': (2, 'MOUNT'), 'C': (3, 'DESERT'), 'D': (4, 'FOREST'), 'E': (5, 'TEMPLE'),
               'F': (6, 'HELL'), 'G': (7, 'TOWN'), 'H': (8, 'BATTLE'), 'I': (9, 'ICE'), 'J': (10, 'DREAM'),
               'K': (11, 'SKY'), 'S': (12, 'SECRET'), 'L': (0xd, 'TOWER')}  # ELF worlddatas 0x23deb0
    number, wad = letters[a.world]
    w, wd = resolve([f'LEVELS/LEVEL{a.world}{a.level}/WORLDS.PS2', f'WDATA/{wad}.WAD'])
    # LoadWorldData 0x161b98: level_data = LEVL record [wave & 0xff]; the
    # level directory is the record's code (GetLevelDesc). Records are not
    # stored in level-number order (CASTLE: A1, A6, A2, ...).
    index = level_index(wd.read_bytes(), f'{a.world}{a.level}')
    proposed = json.loads(pathlib.Path('private/reports/wad-independent.json').read_text())
    known = {n.upper() for e in proposed['entries'] for n in e['proposed_names']}
    # HYPOTHESIS (as run_play.py): the level's own ITEMS package and the realm's.
    item_pkgs = [f'ITEMS/LEVEL{x}/ANIM.PS2' for x in (f'{a.world}{a.level}', a.world) if f'ITEMS/LEVEL{x}/ANIM.PS2' in known]
    items = item_pkgs[0]
    tmp = pathlib.Path(a.report + '.enemies.json')
    if tmp.exists():
        tmp.unlink()
    subprocess.run([TOOLS, 'level-enemies', str(wd), str(index), str(tmp)], check=True)
    level = json.loads(tmp.read_text())
    kinds = level['enemy_types']['kind']
    tmp.unlink()
    elf = ELF.read_bytes()
    prefixes = {}
    from elf_read import read as elf_read  # noqa: E402
    for k in range(44):  # ELF enemy_table 0x22c7a0: type +0, prefix +0x14, stride 0x24
        rec = elf_read(elf, 0x22c7a0 + 0x24 * k, 0x24)
        t = int.from_bytes(rec[:4], 'little', signed=True)
        if t in kinds and t >= 0:
            prefixes[t] = rec[0x14:0x24].split(b'\0')[0].decode()
    monsters = [(t, p) for t, p in prefixes.items() if f'MONSTERS/{p}/ANIM.PS2' in known]
    ia, wp, pw, clips, rom, pdata, *mons = resolve([items, 'WEAPONS/ANIM.PS2', 'POWERUPS/ANIM.PS2', 'PLAYERS/SOR/ANIM/ANIM.PS2',
                                                    'AUDIO/AUDATPS2.ROM', 'PDATA/SOR.WAD'] + [f'MONSTERS/{p}/ANIM.PS2' for _, p in monsters])
    cmd = [TOOLS, 'game-run', str(w), str(wd), str(index), str(ELF), GAME_START, str(ia),
           str(a.frames), a.report, f'weapons={wp}', f'powerups={pw}', f'rom={rom}', f'pdata={pdata}']
    cmd += [f'{t}={path}:{p}' for (t, p), path in zip(monsters, mons)]
    # HYPOTHESIS: the MBOX object tables loaded for the level (the original
    # load list is not traced); used by MBOX_FindObject_Sub lookups.
    obj = [n for n in [f'LEVELS/LEVEL{a.world}{a.level}/OBJECTS.PS2', f'ITEMS/LEVEL{a.world}{a.level}/OBJECTS.PS2',
                       f'ITEMS/LEVEL{a.world}/OBJECTS.PS2', 'WEAPONS/OBJECTS.PS2', 'POWERUPS/OBJECTS.PS2']
           + [f'MONSTERS/{p}/OBJECTS.PS2' for _, p in monsters] if n in known]
    cmd += ['objects=' + ','.join(str(x) for x in resolve(obj))]
    from run_play import critter_files  # noqa: E402
    for c in critter_files(level, number, known, wd.read_bytes()):
        cmd += [f"critter={c['wad']}" + (f":{c['anim']}" if 'anim' in c else '')]
    if len(item_pkgs) > 1:
        cmd += [f'items2={resolve([item_pkgs[1]])[0]}']
    if a.world != 'G':
        cmd += [f'world={number}']
    print('monsters', monsters, 'missing', sorted(set(prefixes.values()) - {p for _, p in monsters}))
    if a.script:
        cmd += [f'sim={SIM_START}', f'clips={clips}', f'script={a.script}']
    if a.pads:
        [sfx] = resolve(['PLAYERS/SOR/SFXYEL/ANIM.PS2'])
        cmd += [f'clips={clips}', f'pads={a.pads}', f'sfx={sfx}']
    if a.replay:
        cmd += [f'clips={clips}', f'replay={a.replay}']
    if a.potions:
        cmd += [f'potions={a.potions}']
    if a.crystals:
        cmd += [f'crystals={a.crystals}']
    if a.teleport:
        cmd += [f'teleport={a.teleport}']
    if a.players:
        cmd += [f'players={a.players}']
    if a.spawn:
        cmd += [f'spawn={a.spawn}']
    if a.flyby:
        cmd += ['flyby=1']
    if a.turbo:
        cmd += [f'turbo={a.turbo}']
    if a.front:
        cmd += ['front=1']
    if a.fall:
        cmd += [f'fall={a.fall}']
    if a.weapon:
        cmd += [f'weapon={a.weapon}']
    if a.legend:
        cmd += ['legend=1']
    if a.plevel:
        cmd += [f'plevel={a.plevel}']
    if a.quest:
        cmd += [f'quest={a.quest}']
    if a.boss_kill:
        cmd += [f'boss-kill={a.boss_kill}']
    if a.boss_health:
        cmd += [f'boss_health={a.boss_health}']
    subprocess.run(cmd, check=True)
    r = json.loads(pathlib.Path(a.report).read_text())
    if a.replay:
        print('updates', r['updates'], 'equal', r['equal_updates'])
        for u in r['results']:
            print(u['step'], u['error'], u['words_differing'], u['action'], [(d['offset'], d['native'], d['reference']) for d in u['diffs'][:8]])
        for x in r['unsupported']:
            print('UNSUPPORTED', x)
        return
    if a.flyby:
        print('flyby end', r['end_update'], 'points', len(r['path']))
        for x in r['unsupported']:
            print('UNSUPPORTED', x)
        return
    print('sounds', len(r.get('sounds', [])), r.get('sounds', [])[:6])
    for u in r['unsupported']:
        print('UNSUPPORTED', u)
    if a.pads:
        for smp in r['samples']:
            print(smp)
        return
    for smp in r['samples']:
        if smp['effects']:
            print(smp['frame'], [(e['slot'], e['node']['visual'], round(e['node']['radius'], 1)) for e in smp['effects']])
    last = r['samples'][-1]
    alive = [e for e in last['enemies'] if e['state'] in (1, 6)]
    print('frame', last['frame'], 'health', round(last['player_health'], 1), 'pos',
          [round(x, 1) for x in last['player_pos']], 'enemies alive', len(alive), 'driver', last.get('driver'))


if __name__ == '__main__':
    main()
