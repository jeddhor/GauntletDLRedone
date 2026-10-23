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
from run_play import ELF, levl_records, resolve  # noqa: E402

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
    ap.add_argument('--pexp', help='DEBUG precondition: player 0 gains the experience of level N (ModifyExp, PlayerUpdateAtts)')
    ap.add_argument('--quest', help='DEBUG: QuestItemState 2 at this update')
    ap.add_argument('--boss-kill', help='DEBUG: lethal CritterDamage on the boss at this update')
    ap.add_argument('--replay', help='per-update replay of a reference_trace --player trace')
    ap.add_argument('--enemy-replay', help='per-update replay of a reference_trace --player --enemies trace through do_enemies')
    ap.add_argument('--items-seed', help='export_live_items.py JSON replacing the loaded items (replays)')
    ap.add_argument('--items-replay', action='store_true', help='with --enemy-replay: run ProcessItems before do_enemies and compare the Items array')
    ap.add_argument('--frame-replay', action='store_true', help='with --enemy-replay: the whole update (ProcessItems, do_enemies, do_players, ProcessEffects) from the before state')
    ap.add_argument('--load-players', type=int, help='NumInGamePlayers at GauntWorldInit\'s world_update (0: entered from the select screen, as the Tower of a new game)')
    ap.add_argument('--pads', help='record-based players: MASK_HEX:UPDATES,... (bit i = joyGetStatus entry i of port 0)')
    ap.add_argument('--potions', help='DEBUG: set player 0 potion count')
    ap.add_argument('--crystals', help='DEBUG: player 0 crystal count N:COUNT')
    ap.add_argument('--boss-health', help='DEBUG: set the boss critter health')
    ap.add_argument('--teleport', help='DEBUG: start player 0 at x,y,z')
    ap.add_argument('--powerup', help='DEBUG: player 0 powerup KIND:FLAGS_HEX:SECONDS at update 0 (e.g. 9:2:60, X-ray)')
    ap.add_argument('--place0', action='store_true', help='DEBUG: place player 0 at the start locator through load_player_geo (placement comparisons)')
    ap.add_argument('--watch', help='collision objects OBJ,... whose matrix translations are sampled')
    ap.add_argument('--probe', help='X:Z;... points whose highest floor (object, y) is sampled')
    ap.add_argument('--itemw', help='item slots SLOT,... whose +0xd8/+0xde/+0xe1/+0xe2 are sampled')
    ap.add_argument('--vplayers', help='NumVirtualPlayers at the level load (co-op traces)')
    ap.add_argument('--weapon', help='DEBUG: player 0 weapon powerup TYPE:SECONDS (kind 5)')
    ap.add_argument('--fall', help='DEBUG: player 0 drops below the world bottom at this update')
    ap.add_argument('--players', help='players in play (copies of player 0 without input, placed by load_player_geo)')
    ap.add_argument('--turbo', help='DEBUG: player 0 turbo (+0x800)')
    ap.add_argument('--front', action='store_true', help='DEBUG: player 1 stands 2 ahead of player 0 (with --players 2)')
    ap.add_argument('--flyby', action='store_true', help='the attract flyby on this level (no players; old camera)')
    ap.add_argument('--bot', action='store_true', help='with --pads: the traversal bot drives port 0 (gdl_tools bot.rs; health assist labelled DEBUG)')
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
    # LoadItems (as run_play.py): the realm's package while BossType < 0,
    # then the level's own; gdl_tools searches POWERUPS between them.
    boss = next(b for _, c, b in levl_records(wd.read_bytes()) if c == f'{a.world}{a.level}')
    item_pkgs = [f'ITEMS/LEVEL{x}/ANIM.PS2' for x, use in ((a.world, boss < 0), (f'{a.world}{a.level}', True))
                 if use and f'ITEMS/LEVEL{x}/ANIM.PS2' in known]
    items = item_pkgs[0]
    tmp = pathlib.Path(a.report + '.enemies.json')
    if tmp.exists():
        tmp.unlink()
    subprocess.run([TOOLS, 'level-enemies', str(wd), str(index), str(tmp)], check=True)
    level = json.loads(tmp.read_text())
    tmp.unlink()
    # The level's enemy packages (run_play.level_monsters: GauntWorldInit's
    # LoadEnemy loop, GetEnemyDir by the slot subtype).
    from run_play import level_monsters  # noqa: E402
    monsters = level_monsters(level, known)
    ia, wp, pw, clips, rom, pdata, *mons = resolve([items, 'WEAPONS/ANIM.PS2', 'POWERUPS/ANIM.PS2', 'PLAYERS/SOR/ANIM/ANIM.PS2',
                                                    'AUDIO/AUDATPS2.ROM', 'PDATA/SOR.WAD'] + [f'{d}/ANIM.PS2' for _, _, d in monsters])
    cmd = [TOOLS, 'game-run', str(w), str(wd), str(index), str(ELF), GAME_START, str(ia),
           str(a.frames), a.report, f'weapons={wp}', f'powerups={pw}', f'rom={rom}', f'pdata={pdata}']
    cmd += [f'{t}={path}:{p}' for (t, p, _), path in zip(monsters, mons)]
    [text] = resolve(['TEXT/ENGLISH.ROM'])
    cmd += [f'text={text}']
    # HYPOTHESIS: the MBOX object tables loaded for the level (the original
    # load list is not traced); used by MBOX_FindObject_Sub lookups.
    obj = [n for n in [f'LEVELS/LEVEL{a.world}{a.level}/OBJECTS.PS2', f'ITEMS/LEVEL{a.world}{a.level}/OBJECTS.PS2',
                       f'ITEMS/LEVEL{a.world}/OBJECTS.PS2', 'WEAPONS/OBJECTS.PS2', 'POWERUPS/OBJECTS.PS2']
           + [f'{d}/OBJECTS.PS2' for _, _, d in monsters] if n in known]
    cmd += ['objects=' + ','.join(str(x) for x in resolve(obj))]
    from run_play import critter_files  # noqa: E402
    for c in critter_files(level, number, known, wd.read_bytes()):
        cmd += [f"critter={c['wad']}" + (f":{c['anim']}" if 'anim' in c else '')]
    if len(item_pkgs) > 1:
        cmd += [f'items2={resolve([item_pkgs[1]])[0]}']
    # The first package's role (InitEffects' item_anim = the realm's,
    # item_anim2 = the level's own).
    cmd += [f"items_role={'realm' if item_pkgs[0] == f'ITEMS/LEVEL{a.world}/ANIM.PS2' else 'level'}"]
    if a.world != 'G':
        cmd += [f'world={number}']
    print('monsters', monsters)
    if a.script:
        cmd += [f'sim={SIM_START}', f'clips={clips}', f'script={a.script}']
    if a.pads:
        [sfx] = resolve(['PLAYERS/SOR/SFXYEL/ANIM.PS2'])
        cmd += [f'clips={clips}', f'pads={a.pads}', f'sfx={sfx}']
        if a.bot:
            cmd += [f"bot={os.environ.get('GDL_BOT', '1')}"]
            # Route hints (tools/bot-routes/<W><L>.json); GDL_BOT_HINTS=0 runs without.
            hints = pathlib.Path('tools/bot-routes') / f'{a.world}{a.level}.json'
            if hints.exists() and os.environ.get('GDL_BOT_HINTS') != '0':
                cmd += [f'hints={hints}']
    if a.replay or a.enemy_replay:
        # The traced player's class SFX package (the fixture's sorceress,
        # yellow): InitPlayerMissiles' custom effects.
        [sfx] = resolve(['PLAYERS/SOR/SFXYEL/ANIM.PS2'])
        cmd += [f'sfx={sfx}']
    if a.replay:
        cmd += [f'clips={clips}', f'replay={a.replay}']
    if a.enemy_replay:
        cmd += [f'clips={clips}', f'enemy_replay={a.enemy_replay}']
        if a.items_replay:
            cmd += ['items_replay=1']
        if a.frame_replay:
            cmd += ['frame_replay=1']
    if a.items_seed:
        cmd += [f'items_seed={a.items_seed}']
    if a.load_players is not None:
        cmd += [f'load_players={a.load_players}']
    if a.potions:
        cmd += [f'potions={a.potions}']
    if a.crystals:
        cmd += [f'crystals={a.crystals}']
    if a.teleport:
        cmd += [f'teleport={a.teleport}']
    if a.place0:
        cmd += ['place0=1']
    if a.powerup:
        cmd += [f'powerup={a.powerup}']
    if a.watch:
        cmd += [f'watch={a.watch}']
    if a.probe:
        cmd += [f'probe={a.probe}']
    if a.itemw:
        cmd += [f'itemw={a.itemw}']
    if a.vplayers:
        cmd += [f'vplayers={a.vplayers}']
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
    if a.pexp:
        cmd += [f'pexp={a.pexp}']
    if a.quest:
        cmd += [f'quest={a.quest}']
    if a.boss_kill:
        cmd += [f'boss-kill={a.boss_kill}']
    if a.boss_health:
        cmd += [f'boss_health={a.boss_health}']
    subprocess.run(cmd, check=True)
    r = json.loads(pathlib.Path(a.report).read_text())
    if a.enemy_replay:
        print('updates', r['updates'], 'equal', r['equal_updates'], 'offsets', r['differing_offsets'])
        for u in r['results'][:12]:
            print(u['step'], 'seed', u['seed_solved'], u['error'], u['words_differing'], [(d['enemy'], d['offset'], d['native'], d['reference']) for d in u['diffs'][:6]])
        for x in r['unsupported']:
            print('UNSUPPORTED', x)
        return
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
