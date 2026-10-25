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
    # Exploration (no checks): two card files from one character (health
    # 500 and 123), then player 1 loads file 0 and player 2 joins on port 2
    # (keyboard set 2: U start, TFGH d-pad, R cross).
    'coop-explore': dict(
        level=None, updates=400,
        actions=[{"at": 100, "save": 0}, {"at": 110, "health": 123}, {"at": 120, "save": 1}, {"at": 130, "dump": "made"}],
        menu=presses(TRIANGLE, 30, 8, 30),
        then=dict(load_slot=1, level=None, updates=3000, actions=[{"at": 100, "dump": "p1"}, {"at": 900, "dump": "later"}], menu=presses(TRIANGLE, 30, 8, 30),
                  steps=[["wait", 25], ["shot", "tower"], ["key", "u", 0.3], ["wait", 6], ["shot", "join"], ["key", "r", 0.3], ["wait", 3], ["shot", "load"],
                         ["key", "g", 0.4], ["wait", 2], ["shot", "down"], ["key", "r", 0.3], ["wait", 3], ["shot", "pick"], ["key", "r", 0.3], ["wait", 3],
                         ["shot", "pick2"], ["key", "r", 0.3], ["wait", 4], ["shot", "pick3"]]),
        checks=[]),
    # M18 co-op ownership (GUI, keyboard set 2 for player 2): card files 0
    # and 1 hold the same character with health 500 and 123. Player 1
    # starts from file 0 (+0x2af4 = 0); player 2 joins (U), Load (R),
    # memory card slot 1 (R): the file list shows file 0 unavailable (owned
    # by player 1, verify_vmu_file_ok) and selects file 1 (R), then the
    # character and Done (R, R). Player 2 then holds file 1's character.
    'coop-ownership': dict(
        level=None, updates=400,
        actions=[{"at": 100, "save": 0}, {"at": 110, "health": 123}, {"at": 120, "save": 1}, {"at": 130, "dump": "made"}],
        menu=presses(TRIANGLE, 30, 8, 30),
        then=dict(load_slot=1, level=None, updates=1500, actions=[{"at": 100, "dump": "p1"}, {"at": 900, "dump": "both"}], menu=presses(TRIANGLE, 30, 8, 30),
                  steps=[["wait", 14], ["key", "u", 0.3], ["wait", 6], ["shot", "join"], ["key", "r", 0.3], ["wait", 3], ["key", "r", 0.3], ["wait", 3],
                         ["shot", "files"], ["key", "r", 0.3], ["wait", 4], ["key", "r", 0.3], ["wait", 4], ["shot", "menu"], ["key", "r", 0.3], ["wait", 6],
                         ["shot", "joined"]]),
        checks=[("made", "health", "==", 123.0), ("p1", "players.0.file", "==", 0), ("both", "players.0.file", "==", 0),
                ("both", "players.1.file", "==", 1), ("both", "players.1.health", "==", 123.0), ("both", "players.1.port", "==", 1),
                ("both", "players.0.health", "==", 500.0)]),
    # Exploration (no checks): the Sorceress throwing in G1, frames every
    # second (the thrown wand's textures come from another package).
    'throw-g1': dict(
        level='G1', updates=420, actions=[], menu=[], script=presses(CROSS, 160, 12, 18, hold=6),
        steps=[["wait", 4.5]] + [s for k in range(16) for s in (["shot", f"throw{k:02d}"], ["wait", 0.6])],
        checks=[]),
    # M15 ending (DEBUG boss_kill: lethal CritterDamage on Garm, labelled):
    # BossDeath, the good wizard and the auto exit run natively, then
    # game_main's BossType 0x2c level end plays /MPEG/victory.pss
    # (init_gamemovie, movietype 2), its end opens the shop (init_shop), and
    # the panels lead to the Tower. H4's completion bit (world 8, level 4)
    # is recorded and survives a save and a reload.
    'ending-h4': dict(
        level='H4', updates=2600,
        actions=[{"at": 5, "dump": "start"}, {"at": 150, "boss_kill": True}, {"at": 160, "boss_kill": True}, {"at": 600, "dump": "dead"},
                 {"at": 2500, "dump": "after"}, {"at": 2510, "save": 0}, {"at": 2520, "dump": "saved"}],
        menu=[e for t in range(1400, 6000, 400) for e in ([[t, t + 2, [UP]], [t + 60, t + 62, [CROSS]]] + presses(TRIANGLE, t + 120, 4, 60))],
        then=dict(load_slot=1, updates=300, actions=[{"at": 200, "dump": "loaded"}], menu=presses(TRIANGLE, 30, 12, 60)),
        checks=[("dead", "world", "==", 8), ("after", "world", "==", 13), ("after", "waves", "!=", "start.waves"), ("after", "state", "==", 1),
                ("loaded", "waves", "==", "saved.waves"), ("loaded", "world", "==", 13)]),
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
    s = list(sc.get('steps', []))
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
    if sc.get('level') is None:
        env.pop('GDL_DEBUG_LEVEL', None)
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
        got = dumps.get(label, {})
        for part in field.split('.'):
            got = got[int(part)] if isinstance(got, list) and part.isdigit() and int(part) < len(got) else (got.get(part) if isinstance(got, dict) else None)
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
