#!/usr/bin/env python3
"""A chest's content grows as the lid opens, in the real client.

  chest_content_check.py TAG [--binary PATH]

On the private Xvfb with lavapipe (virtual_display.py; software rendering,
so the real GPU and the desktop are not involved — a kwin session on the
RX 580 faulted the GPU on 2026-11-02), gdl_play starts G1 with a TEST character (GDL_TEST_AUTOSTART, its own HOME
and saves), is moved onto the key at (20.99, 0.6, 10.77) at update 160 and
onto chest 43 at update 200 (GDL_TEST_ACTIONS teleports, the setup of the
frozen-chestfix frame), beside it at update 212 (the reference's warp
position, so the open chest keeps its content) and dumps every update from
200 to 290. ProcessItems
0x155bb4–0x155c94 sets the content node's scale from the chest's state
(+0xdc): 0.2 while closed, (frame + 1) / frames × 0.8 + 0.2 while opening
and full size (flag 8 cleared) once open. Reference: chest43-open-ref-a-v1
(state 1, frame 34 of 50, node scale 0.76; playtest.md). Passes when the
chest opens, the opening scale follows the rule and never falls, and the
content reaches full size. Results: private/runs/chest-content-TAG/result.json.
Research only.
"""
import argparse
import json
import os
import pathlib
import subprocess
import sys
import time

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from virtual_display import environment  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parents[2]
FIRST, LAST = 200, 290


def judge(dumps):
    fail = []
    rows = [(j['update'], c) for j in dumps for c in j.get('chests', []) if c['slot'] == 43]
    states = [c['state'] for _, c in rows]
    if 1 not in states or not any(s >= 2 for s in states):
        fail.append(f'chest 43 did not open: states {sorted(set(states))}')
    last = 0.0
    for u, c in rows:
        if c['state'] == 0:
            want = 0.2
        elif c['state'] >= 2:
            want = 1.0
        elif c['frames'] < 2:
            want = 1.0
        else:
            want = (c['frame'] + 1.0) / c['frames'] * 0.8 + 0.2
        if abs(c['scale'] - want) > 1e-5:
            fail.append(f'update {u}: scale {c["scale"]} for {c}, rule {want}')
        if c['scale'] < last - 1e-6:
            fail.append(f'update {u}: scale fell {last} -> {c["scale"]} ({c})')
        last = c['scale']
    if rows and rows[-1][1]['scale'] != 1.0:
        fail.append(f'content not full size at the end: {rows[-1]}')
    return fail, [(u, c['state'], c['clip'], c['frame'], c['frames'], round(c['scale'], 4)) for u, c in rows]


def run_client(args, d):
    (d / 'saves').mkdir(parents=True)
    (d / 'home').mkdir()
    cfg = json.loads((ROOT / 'private/reports/play-config.json').read_text())
    cfg['saves'] = str(d / 'saves')
    (d / 'config.json').write_text(json.dumps(cfg))
    actions = [{'at': 150, 'health': 5000}, {'at': 160, 'teleport': [20.99, 0.6, 10.77]},
               {'at': 200, 'teleport': [63.95, 10.5, -25.54]}, {'at': 212, 'teleport': [66.95, 10.0, -25.54]}]
    actions += [{'at': u, 'dump': f'u{u:05}'} for u in range(FIRST, LAST + 1)]
    (d / 'actions.json').write_text(json.dumps(actions))
    env = dict(environment(), HOME=str(d / 'home'), GDL_NO_AUDIO='1',
               GDL_NO_ATTRACT='1', GDL_NO_GAMEPADS='1', GDL_TEST_AUTOSTART='1', GDL_DEBUG_LEVEL='G1',
               GDL_TEST_ACTIONS=str(d / 'actions.json'), GDL_EXIT_AFTER=str(LAST + 5),
               GDL_STATE_OUT=str(d / 'state.txt'), GDL_RENDER_FPS='60', RUST_BACKTRACE='1')
    for k in ('WAYLAND_DISPLAY', 'XDG_DATA_HOME', 'GDL_SETTINGS', 'GDL_BASELINE'):
        env.pop(k, None)
    with open(d / 'client.log', 'w') as log:
        client = subprocess.Popen([args.binary, str(d / 'config.json')], cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
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
    panics = [ln for ln in (d / 'client.log').read_text(errors='replace').splitlines() if 'panicked at' in ln]
    dumps = sorted((json.loads(f.read_text()) for f in d.glob('state.u*.json')), key=lambda j: j['update'])
    fail, timeline = judge(dumps)
    if timed_out or client.returncode != 0 or panics:
        fail.append(f'client: status {client.returncode}, timed out {timed_out}, panics {panics}')
    return dict(binary=args.binary, passed=not fail, failures=fail,
                timeline_update_state_clip_frame_frames_scale=timeline)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('tag')
    ap.add_argument('--binary', default=str(ROOT / 'target/release/gdl_play'))
    args = ap.parse_args()
    args.binary = str(pathlib.Path(args.binary).resolve())
    run = ROOT / 'private/runs' / f'chest-content-{args.tag}'
    if run.exists():
        raise SystemExit(f'{run} exists')
    run.mkdir(parents=True)
    result = run_client(args, run / 'G1')
    (run / 'result.json').write_text(json.dumps(result, indent=1))
    print('PASS' if result['passed'] else 'FAIL', result['failures'][:5])
    sys.exit(0 if result['passed'] else 1)

if __name__ == '__main__':
    main()
