#!/usr/bin/env python3
"""Persistent-progress scenario suite (M13) on the native client.

  scenarios.py TAG [NAME ...]

Each scenario runs gdl_play on the private Xvfb through client_driver.py
with the TEST harness: GDL_TEST_AUTOSTART (a new default character),
GDL_DEBUG_LEVEL (the starting level), GDL_INPUT_SCRIPT (port 0 while the
level plays; empty = no input), GDL_MENU_SCRIPT (port 0 outside play: tally,
shop, select screen, message boxes), GDL_TEST_ACTIONS (teleport / health /
progress dumps at given updates) and GDL_EXIT_AFTER. The dumps
(<run>/state.<label>.json: player 1's experience, gold of the class, keys,
potions, shards, runes, completed-level bytes, the save region's SHA-256)
are checked against each scenario's expectations; the report goes to
private/reports/scenarios-TAG.json. Frames are taken every 15 s.
Teleports and health writes are DEBUG shortcuts (labelled in the report);
the progression code they lead into is the game's own. Actions start after
the level-start sequence (the player is placed and active from about update
100).
"""
import json
import os
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]


def presses(entry, first, count, gap, hold=3):
    """count presses of joyGetStatus entry, gap ticks apart from tick first."""
    return [[first + k * gap, first + k * gap + hold - 1, [entry]] for k in range(count)]


CROSS, TRIANGLE, UP = 0xd, 0xc, 2

SCENARIOS = {
    # A key picked up in G1 does not survive a death: TowerWave restores the
    # level-start state (PlayerRestoreState) and the Tower follows.
    'death-g1': dict(
        level='G1', updates=900,
        actions=[{"at": 5, "dump": "start"}, {"at": 150, "teleport": [20.99, 0.6, 10.77]}, {"at": 200, "dump": "key"},
                 {"at": 220, "health": 0.5}, {"at": 880, "dump": "after"}],
        menu=presses(CROSS, 40, 12, 90) + presses(TRIANGLE, 85, 12, 90),
        checks=[("key", "keys", "==", 1), ("after", "keys", "==", "start.keys"), ("after", "exp", "==", "start.exp"),
                ("after", "waves", "==", "start.waves"), ("after", "state", "==", 1), ("after", "world", "==", 13)]),
    # G1's exit records the level as completed for the class and leads to the
    # Tower; the completion survives into the next level.
    'exit-g1': dict(
        level='G1', updates=900,
        actions=[{"at": 5, "dump": "start"}, {"at": 150, "teleport": [120.7, 86.6, -476.1]}, {"at": 880, "dump": "after"}],
        menu=presses(CROSS, 60, 4, 90) + [[420, 422, [UP]], [480, 482, [CROSS]]] + presses(TRIANGLE, 600, 12, 60),
        checks=[("after", "world", "==", 13), ("after", "waves", "!=", "start.waves"), ("after", "state", "==", 1)]),
}


def value(dumps, ref):
    if isinstance(ref, str) and '.' in ref:
        label, field = ref.split('.', 1)
        return dumps.get(label, {}).get(field)
    return ref


def run(tag, name, sc):
    run_tag = f'{tag}-{name}'
    d = ROOT / 'private' / 'runs' / f'client-{run_tag}'
    if d.exists():
        raise SystemExit(f'{d} exists')
    d.mkdir(parents=True)
    files = {}
    for key in ('actions', 'menu'):
        files[key] = d / f'{key}.json'
        files[key].write_text(json.dumps(sc[key]))
    script = d / 'script.json'
    script.write_text(json.dumps(sc.get('script', [])))
    out = d / 'state.txt'
    steps = d / 'steps.json'
    s = []
    for k in range(200):
        s += [["wait", 3]] + ([["shot", f"t{k:03d}"]] if k % 5 == 4 else [])
    steps.write_text(json.dumps(s))
    env = dict(os.environ, GDL_TEST_AUTOSTART='1', GDL_DEBUG_LEVEL=sc['level'], GDL_INPUT_SCRIPT=str(script),
               GDL_MENU_SCRIPT=str(files['menu']), GDL_TEST_ACTIONS=str(files['actions']),
               GDL_EXIT_AFTER=str(sc['updates']), GDL_STATE_OUT=str(out))
    subprocess.run([sys.executable, str(ROOT / 'tools/python/client_driver.py'), 'serve', run_tag, str(steps)],
                   cwd=ROOT, env=env, timeout=2400)
    dumps = {}
    for f in d.glob('state.*.json'):
        j = json.loads(f.read_text())
        dumps[j['label']] = j
    results = []
    for label, field, op, ref in sc['checks']:
        got = dumps.get(label, {}).get(field)
        want = value(dumps, ref)
        ok = got is not None and want is not None and ((got == want) if op == '==' else (got != want))
        results.append(dict(dump=label, field=field, op=op, expected=ref, expected_value=want, value=got, ok=ok))
    return dict(level=sc['level'], updates_run=len(out.read_text().split()) if out.exists() else 0,
                debug_shortcuts=[a for a in sc['actions'] if 'teleport' in a or 'health' in a],
                dumps=dumps, checks=results, passed=all(r['ok'] for r in results))


def main():
    if len(sys.argv) < 2:
        raise SystemExit(__doc__)
    tag = sys.argv[1]
    names = sys.argv[2:] or list(SCENARIOS)
    report = ROOT / 'private' / 'reports' / f'scenarios-{tag}.json'
    if report.exists():
        raise SystemExit(f'{report} exists')
    out = {name: run(tag, name, SCENARIOS[name]) for name in names}
    with report.open('x') as f:
        json.dump(out, f, indent=1)
    for name, r in out.items():
        print(name, 'PASS' if r['passed'] else 'FAIL', [(c['dump'], c['field'], c['value'], c['op'], c['expected_value']) for c in r['checks'] if not c['ok']])


if __name__ == '__main__':
    main()
