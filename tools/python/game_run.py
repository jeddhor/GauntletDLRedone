#!/usr/bin/env python3
"""Headless INTERIM native game run for The Poison Fields (levels G1-G4).

Resolves packages from the verified private extraction and runs
`gdl_tools game-run` with an optional scripted interim player. Script format:
LEVELS_HEX:UPDATES,... (e.g. 0:20,2000200:3 = idle 20 updates, attack 3).
Reports are written once (new file) under private/reports/.
"""
import argparse
import json
import pathlib
import subprocess
import sys

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from run_play import ELF, resolve  # noqa: E402

GAME_START = 'private/reference/start-states/poison-fields-game-v5.json'
SIM_START = 'private/reference/start-states/poison-fields-sor-v2.json'


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('report')
    ap.add_argument('--frames', type=int, default=600)
    ap.add_argument('--level', type=int, default=1, help='G level number 1-4')
    ap.add_argument('--script')
    a = ap.parse_args()
    names = [f'LEVELS/LEVELG{a.level}/WORLDS.PS2', 'WDATA/TOWN.WAD', 'ITEMS/LEVELG/ANIM.PS2', 'MONSTERS/ZOM/ANIM.PS2',
             'MONSTERS/MAG/ANIM.PS2', 'MONSTERS/GHO/ANIM.PS2', 'WEAPONS/ANIM.PS2', 'POWERUPS/ANIM.PS2',
             'PLAYERS/SOR/ANIM/ANIM.PS2']
    w, wd, ia, zom, mag, gho, wp, pw, clips = resolve(names)
    cmd = ['target/release/gdl_tools', 'game-run', str(w), str(wd), str(a.level - 1), str(ELF), GAME_START, str(ia),
           str(a.frames), a.report, f'13={zom}:ZOM', f'12={mag}:MAG', f'20={gho}:GHO', f'weapons={wp}',
           f'powerups={pw}']
    if a.script:
        cmd += [f'sim={SIM_START}', f'clips={clips}', f'script={a.script}']
    subprocess.run(cmd, check=True)
    r = json.loads(pathlib.Path(a.report).read_text())
    for u in r['unsupported']:
        print('UNSUPPORTED', u)
    last = r['samples'][-1]
    alive = [e for e in last['enemies'] if e['state'] in (1, 6)]
    print('frame', last['frame'], 'health', round(last['player_health'], 1), 'pos',
          [round(x, 1) for x in last['player_pos']], 'enemies alive', len(alive), 'driver', last.get('driver'))


if __name__ == '__main__':
    main()
