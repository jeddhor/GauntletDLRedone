#!/usr/bin/env python3
"""Content coverage matrix (M14) from the private reports.

  coverage_matrix.py SWEEP_LOG [SWEEP_LOG ...] REGRESSION_LOG NEW_REPORT.json

Per level (play-config.json, all 61): the pad sweep's outcome (game_run
--pads, 1400 updates: unsupported paths or OK), the whole-update replays
against reference traces from the regression log (name, updates, equal
updates per part), and whether a scene comparison frame exists
(scene-<code>-v*-native.png). Per enemy type/AI pair and per item
definition kind across the levels (enemy-logic-scan-v1.json; world-inspect
item definitions): the levels using it and whether any replayed level uses
it. Research report; the status words are: supported (runs, no unported
path), compared (also in a passing reference replay), blocked (an unported
path or failure).
"""
import collections
import glob
import json
import os
import pathlib
import re
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
R = ROOT / 'private' / 'reports'

# Regression names → level code (the replay's --world/--level).
REPLAY_LEVEL = {
    'frame-fight-g1': 'G1', 'frame-combat-g1': 'G1', 'frame-varied-g1': 'G1', 'frame-tower': 'L1', 'frame-a1': 'A1',
    'frame-b1': 'B1', 'frame-k2': 'K2', 'frame-g1new': 'G1', 'frame-b6': 'B6', 'frame-g5': 'G5', 'frame-C5': 'C5',
    'frame-D5': 'D5', 'frame-I5': 'I5', 'frame-J5': 'J5', 'frame-K5': 'K5', 'frame-E2': 'E2', 'frame-F2': 'F2',
    'frame-H4': 'H4', 'frame-A5': 'A5', 'frame-A1statue': 'A1', 'frame-A1golem': 'A1', 'frame-A1death': 'A1',
    'frame-B6kill': 'B6', 'frame-B6wiz2': 'B6', 'frame-B6wiz3': 'B6', 'frame-B6wiz4': 'B6', 'frame-G1exit': 'G1',
    'frame-C1water': 'C1', 'frame-C1water2': 'C1', 'frame-G1potion': 'G1', 'frame-G1xbow': 'G1', 'frame-A1trans': 'A1',
}


def sweep(paths):
    out = {}
    for p in paths:
        for line in pathlib.Path(p).read_text().splitlines():
            m = re.match(r'^(\w+) (OK|FAILED|\[.*?\])', line)
            if m:
                out[m.group(1)] = 'OK' if m.group(2) == 'OK' else line[len(m.group(1)) + 1:][:200]
    return out


def regression(path):
    out = collections.defaultdict(list)
    for line in pathlib.Path(path).read_text().splitlines():
        m = re.match(r"^(\S+) (\{.*\}) items", line)
        if m and m.group(1) in REPLAY_LEVEL:
            d = eval(m.group(2), {'__builtins__': {}})  # the log prints Python dict literals
            out[REPLAY_LEVEL[m.group(1)]].append(dict(name=m.group(1), **d))
    return out


def main():
    if len(sys.argv) < 4:
        raise SystemExit(__doc__)
    sweeps, reg, target = sys.argv[1:-2], sys.argv[-2], pathlib.Path(sys.argv[-1])
    if target.exists():
        raise SystemExit(f'{target} exists')
    cfg = json.loads((R / 'play-config.json').read_text())
    sw = sweep(sweeps)
    rep = regression(reg)
    scenes = {pathlib.Path(p).name.split('-')[1].upper() for p in glob.glob(str(R / 'scene-*-v*-native.png'))}
    levels = []
    for lv in cfg['levels']:
        code = lv['name']
        replays = rep.get(code, [])
        all_equal = bool(replays) and all(r['equal_updates'] == r['updates'] for r in replays)
        status = 'blocked' if sw.get(code, 'missing') != 'OK' else ('compared' if replays else 'supported')
        levels.append(dict(code=code, world=lv['world'], sweep=sw.get(code, 'missing'), replays=replays,
                           replays_all_equal=all_equal, scene_compared=code in scenes, status=status))
    # Enemy type/AI pairs and item kinds per level.
    scan = json.loads((R / 'enemy-logic-scan-v1.json').read_text())['levels']
    compared_levels = {l['code'] for l in levels if l['replays']}
    pairs = collections.defaultdict(set)
    for code, d in scan.items():
        for key in list(d.get('generators_type_ai', {})) + list(d.get('placed_type_ai', {})):
            pairs[key].add(code)
    enemy = [dict(type_ai=k, levels=sorted(v), compared=bool(v & compared_levels)) for k, v in sorted(pairs.items(), key=lambda kv: tuple(int(x) for x in kv[0].split(':')))]
    names = json.loads((R / 'wad-independent.json').read_text())['entries']
    files = {int(os.path.basename(p)[:6]): p for p in glob.glob(str(ROOT / 'private/wad/*.dat'))}
    kinds = collections.defaultdict(set)
    with tempfile.TemporaryDirectory() as tmp:
        for x in names:
            nm = [n for n in x['proposed_names'] if n.upper().startswith('LEVELS/') and n.upper().endswith('/WORLDS.PS2')]
            if not nm:
                continue
            code = nm[0].split('/')[1][5:].upper()
            out = os.path.join(tmp, f"{x['index']}.json")
            subprocess.run([str(ROOT / 'target/release/gdl_tools'), 'world-inspect', files[x['index']], out], check=True, capture_output=True)
            w = json.load(open(out))['world']
            defs = {q['index']: q for q in w['item_definitions']}
            for it in w['items']:
                q = defs.get(it['definition'])
                if q:
                    kinds[f"{q['kind']}:{q['subtype']}"].add(code)
    items = [dict(kind_subtype=k, levels=sorted(v), compared=bool(v & compared_levels)) for k, v in sorted(kinds.items(), key=lambda kv: tuple(int(x) for x in kv[0].split(':')))]
    summary = dict(levels=len(levels), levels_supported=sum(l['status'] != 'blocked' for l in levels),
                   levels_compared=sum(l['status'] == 'compared' for l in levels), levels_scene_compared=sum(l['scene_compared'] for l in levels),
                   enemy_type_ai=len(enemy), enemy_type_ai_compared=sum(e['compared'] for e in enemy),
                   item_kind_subtypes=len(items), item_kind_subtypes_compared=sum(i['compared'] for i in items))
    with target.open('x') as f:
        json.dump(dict(summary=summary, levels=levels, enemy_type_ai=enemy, item_kinds=items), f, indent=1)
    print(json.dumps(summary))


if __name__ == '__main__':
    main()
