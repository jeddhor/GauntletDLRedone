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
import shutil
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
    # The G1 completion saved in the Tower (PlayerWriteSaveFile to card file
    # 0) is the same after a restart that loads save0001 (phase B).
    'save-reload': dict(
        level='G1', updates=900,
        actions=[{"at": 5, "dump": "start"}, {"at": 150, "teleport": [120.7, 86.6, -476.1]}, {"at": 870, "save": 0},
                 {"at": 875, "dump": "saved"}],
        menu=presses(CROSS, 60, 4, 90) + [[420, 422, [UP]], [480, 482, [CROSS]]] + presses(TRIANGLE, 600, 12, 60),
        then=dict(load_slot=1, updates=300, actions=[{"at": 200, "dump": "loaded"}], menu=presses(TRIANGLE, 30, 12, 60)),
        checks=[("saved", "world", "==", 13), ("loaded", "waves", "==", "saved.waves"), ("loaded", "exp", "==", "saved.exp"),
                ("loaded", "gold", "==", "saved.gold"), ("loaded", "keys", "==", "saved.keys"), ("loaded", "world", "==", 13),
                ("loaded", "waves", "!=", "start.waves")]),
}


# Two saves of card file 0 (save0001.json): before G1's exit (S0) and in the
# Tower after it (S1, whose .bak is S0). Used by the recovery scenarios.
TWO_SAVES = dict(
    level='G1', updates=900,
    actions=[{"at": 5, "dump": "start"}, {"at": 140, "save": 0}, {"at": 150, "teleport": [120.7, 86.6, -476.1]},
             {"at": 870, "save": 0}, {"at": 875, "dump": "saved"}],
    menu=presses(CROSS, 60, 4, 90) + [[420, 422, [UP]], [480, 482, [CROSS]]] + presses(TRIANGLE, 600, 12, 60))

SCENARIOS.update({
    # M18: a corrupted primary file (one region digit flipped, so the
    # envelope's SHA-256 fails) loads the previous save from .bak, and the
    # client says so.
    'recover-corrupt': dict(TWO_SAVES, mutate=[["corrupt", "save0001.json"]],
                            then=dict(load_slot=1, updates=300, actions=[{"at": 30, "dump": "loaded"}], menu=presses(TRIANGLE, 30, 12, 60)),
                            checks=[("saved", "waves", "!=", "start.waves"), ("loaded", "waves", "==", "start.waves"),
                                    ("loaded", "last_event", "contains", "recovered"), ("loaded", "world", "==", 13)]),
    # M18: a write interrupted after the old primary became .bak and before
    # the new .tmp was renamed into place: the complete .tmp (S1) loads.
    'recover-interrupt': dict(TWO_SAVES, mutate=[["rename", "save0001.json", "save0001.tmp"]],
                              then=dict(load_slot=1, updates=300, actions=[{"at": 30, "dump": "loaded"}], menu=presses(TRIANGLE, 30, 12, 60)),
                              checks=[("loaded", "waves", "==", "saved.waves"), ("loaded", "exp", "==", "saved.exp"),
                                      ("loaded", "last_event", "contains", "recovered"), ("loaded", "world", "==", 13)]),
    # M16: player 1's controller removed in G1 (the port reports no device):
    # after 60 polls joyGetStatus marks the player lost, DoOptions'
    # ControllerMessageBox holds the game, and START on the port (an
    # unassigned controller now) gives player 1 back its port.
    'controller-reconnect': dict(
        level='G1', updates=700,
        actions=[{"at": 5, "dump": "start"}, {"at": 150, "disconnect": 0, "reconnect_after": 600}, {"at": 160, "dump": "lost"},
                 {"at": 600, "dump": "after"}],
        menu=[],
        checks=[("after", "controller_lost", "==", 0), ("after", "joy_remap", "==", [0, -1, -1, -1]),
                ("after", "last_event", "contains", "reassigned"), ("lost", "update", "==", 160)]),
})


def mutate(saves, ops):
    """Damage a copied save directory as an interruption or corruption would."""
    for op in ops:
        if op[0] == 'corrupt':
            f = saves / op[1]
            text = f.read_text()
            at = text.index('region_hex') + 20
            text = text[:at] + ('1' if text[at] == '0' else '0') + text[at + 1:]
            f.write_text(text)
        elif op[0] == 'rename':
            (saves / op[1]).rename(saves / op[2])
        else:
            raise ValueError(op)


def value(dumps, ref):
    if isinstance(ref, str) and '.' in ref:
        label, field = ref.split('.', 1)
        return dumps.get(label, {}).get(field)
    return ref


def run(tag, name, sc, saves_from=None, mutations=()):
    run_tag = f'{tag}-{name}'
    d = ROOT / 'private' / 'runs' / f'client-{run_tag}'
    if d.exists():
        raise SystemExit(f'{d} exists')
    d.mkdir(parents=True)
    if saves_from:
        shutil.copytree(saves_from, d / 'saves')
        mutate(d / 'saves', mutations)
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
    env = dict(os.environ, GDL_TEST_AUTOSTART='1', GDL_INPUT_SCRIPT=str(script),
               GDL_MENU_SCRIPT=str(files['menu']), GDL_TEST_ACTIONS=str(files['actions']),
               GDL_EXIT_AFTER=str(sc['updates']), GDL_STATE_OUT=str(out))
    if sc.get('level'):
        env['GDL_DEBUG_LEVEL'] = sc['level']
    if sc.get('load_slot'):
        env['GDL_TEST_LOAD_SLOT'] = str(sc['load_slot'])
    subprocess.run([sys.executable, str(ROOT / 'tools/python/client_driver.py'), 'serve', run_tag, str(steps)],
                   cwd=ROOT, env=env, timeout=2400)
    dumps = {}
    for f in d.glob('state.*.json'):
        j = json.loads(f.read_text())
        dumps[j['label']] = j
    if 'then' in sc:
        b = run(tag, f'{name}-b', dict(sc['then'], checks=[]), saves_from=d / 'saves', mutations=sc.get('mutate', []))
        dumps.update(b['dumps'])
    results = []
    for label, field, op, ref in sc['checks']:
        got = dumps.get(label, {}).get(field)
        want = value(dumps, ref)
        if op == 'contains':
            ok = isinstance(got, str) and want in got
        else:
            ok = got is not None and want is not None and ((got == want) if op == '==' else (got != want))
        results.append(dict(dump=label, field=field, op=op, expected=ref, expected_value=want, value=got, ok=ok))
    return dict(level=sc.get('level'), load_slot=sc.get('load_slot'), updates_run=len(out.read_text().split()) if out.exists() else 0,
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
