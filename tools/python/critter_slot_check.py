#!/usr/bin/env python3
"""A critter taking over the instance slot of one with a larger tree, in the
real client on the real GPU (playtest 2026-11-01, G2: a gargoyle after a
golem panicked in tree_pose::node_world, index 56 of 55, from
present_critters). Actual disc check; needs a build with the DEBUG action
critter_kill.

  critter_slot_check.py TAG [--binary BIN]

G2's one-player statues on the disc (game_run.py --itemdump, +0xf0; +0xe0 1,
golem item 75 has 2): golem item 91 (tree 59 nodes), gargoyle item 74 (55
nodes). The TEST actions wake the golem,
kill it, wait for its slot to empty and wake the gargoyle. The run has its own
HOME and saves (private/runs/critter-slot-TAG). PASS: the client ran to
GDL_EXIT_AFTER without a panic, with the golem in slot 0 and then the gargoyle
in slot 0. Report: result.json there; exit status 1 otherwise.
"""
import argparse
import json
import os
import pathlib
import subprocess
import sys
import time

ROOT = pathlib.Path(__file__).resolve().parents[2]
RADV = '/usr/share/vulkan/icd.d/radeon_icd.json'
GOLEM_ITEM, GOLEM_AT = 91, [-146.9, 0.1, -258.6]
GARGOYLE_ITEM, GARGOYLE_AT = 74, [-183.5, 0.1, -134.7]
EXIT_AFTER = 1500
# Rendered frames: at most 60 a second against 30 updates, fewer on a loaded
# machine (1950 in one run), so the shots span both: the golem from about 420,
# the gargoyle from about 1500.
SHOTS = [420, 520, 620, 720, 800, 1500, 1600, 1700, 1800, 1900, 2200, 2400, 2600, 2800]


def actions():
    a = [{'at': u, 'health': 5000} for u in range(40, EXIT_AFTER, 40)]
    a += [{'at': 60, 'teleport': GOLEM_AT},
          {'at': 150, 'damage_item': [GOLEM_ITEM, 1000]},
          {'at': 420, 'dump': 'golem'},
          {'at': 440, 'critter_kill': 0},
          {'at': 860, 'dump': 'empty'},
          {'at': 880, 'teleport': GARGOYLE_AT},
          {'at': 970, 'damage_item': [GARGOYLE_ITEM, 1000]},
          {'at': 1400, 'dump': 'gargoyle'}]
    return a


def inner(args, run):
    (run / 'saves').mkdir(parents=True)
    (run / 'home').mkdir()
    cfg = json.loads((ROOT / 'private/reports/play-config.json').read_text())
    cfg['saves'] = str(run / 'saves')
    (run / 'config.json').write_text(json.dumps(cfg))
    (run / 'actions.json').write_text(json.dumps(actions()))
    env = dict(os.environ, VK_DRIVER_FILES=RADV, VK_ICD_FILENAMES=RADV, HOME=str(run / 'home'), GDL_NO_AUDIO='1',
               GDL_NO_ATTRACT='1', GDL_NO_GAMEPADS='1', GDL_TEST_AUTOSTART='1', GDL_DEBUG_LEVEL='G2',
               GDL_TEST_ACTIONS=str(run / 'actions.json'), GDL_EXIT_AFTER=str(EXIT_AFTER), GDL_STATE_OUT=str(run / 'state.txt'),
               GDL_SHOT_FRAMES=','.join(map(str, SHOTS)), GDL_SHOT_DIR=str(run), GDL_RENDER_FPS='60', RUST_BACKTRACE='1')
    for k in ('WAYLAND_DISPLAY', 'XDG_DATA_HOME', 'GDL_SETTINGS', 'GDL_BASELINE'):
        env.pop(k, None)
    with open(run / 'client.log', 'w') as log:
        client = subprocess.Popen([args.binary, str(run / 'config.json')], cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
        deadline = time.time() + 300
        while time.time() < deadline and client.poll() is None:
            time.sleep(0.5)
        timed_out = client.poll() is None
        if timed_out:
            client.terminate()
            try:
                client.wait(10)
            except subprocess.TimeoutExpired:
                client.kill()
    text = (run / 'client.log').read_text(errors='replace')
    panics = [ln for ln in text.splitlines() if 'panicked at' in ln or 'index out of bounds' in ln]
    dumps = {}
    for label in ('golem', 'empty', 'gargoyle'):
        f = run / f'state.{label}.json'
        if f.exists():
            dumps[label] = json.loads(f.read_text()).get('critters')
    def slot0_nodes(label):
        return next((c['nodes'] for c in dumps.get(label) or [] if c['slot'] == 0), None)
    ok = (not timed_out and client.returncode == 0 and not panics and slot0_nodes('golem') == 59
          and slot0_nodes('empty') is None and slot0_nodes('gargoyle') == 55)
    result = dict(binary=args.binary, exit_status=client.returncode, timed_out=timed_out, panics=panics, critters=dumps, passed=ok)
    (run / 'result.json').write_text(json.dumps(result, indent=1))
    print(json.dumps(result, indent=1))
    return 0 if ok else 1


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('tag')
    ap.add_argument('--binary', default=str(ROOT / 'target/release/gdl_play'))
    ap.add_argument('--inner', action='store_true')
    args = ap.parse_args()
    args.binary = str(pathlib.Path(args.binary).resolve())
    run = ROOT / 'private/runs' / f'critter-slot-{args.tag}'
    if args.inner:
        sys.exit(inner(args, run))
    if run.exists():
        raise SystemExit(f'{run} exists')
    outer = run.with_name(run.name + '-session')
    outer.mkdir(parents=True)
    status = subprocess.run([sys.executable, str(ROOT / 'tools/python/kwin_session.py'), str(outer / 'session.log'), '--',
                             sys.executable, str(pathlib.Path(__file__).resolve()), args.tag, '--inner', '--binary', args.binary], cwd=ROOT).returncode
    print((outer / 'session.log').read_text())
    sys.exit(status)


if __name__ == '__main__':
    main()
