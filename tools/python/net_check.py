#!/usr/bin/env python3
"""Network play: two machines' games stay equal, in the real client.

  net_check.py TAG [--binary PATH] [--join-binary PATH] [--wine] [--delay N]
                   [--updates N]

Two gdl_play processes on this machine, on the private Xvfb with lavapipe
(virtual_display.py; software rendering, the real GPU and the desktop are
not involved), each with its own HOME, empty saves, no audio, no gamepads
and the original presentation: the host (GDL_NET_HOST, begins once two
machines are in: GDL_NET_MACHINES=2) and a machine joining it on
127.0.0.1 (GDL_NET_JOIN). Their pads come from GDL_NET_SCRIPT by network
step, on each machine's first port: on the select screen the host's player
1 makes a default character with cross; the joining machine's player
presses Start on its own port (1: player 2 joins as on the couch) and does
the same; in the Tower triangle closes the welcome scrolls and both walk (the
host attacking); the joining machine pauses everyone, opens Settings in the
shared menu, goes back and resumes (GDL_NET_SCRIPT menu events). --level
puts both in that level after the select screen (GDL_DEBUG_LEVEL).
Each machine writes its per-update state hashes (GDL_STATE_OUT,
GDL_EXIT_AFTER) and dumps at updates 300 and 880 (players, ports,
positions). Passes when both reach the last update with equal hashes for
every update, equal dumps, two players on the two machines' ports, and
both players moved between the dumps. The host runs --host-extra updates longer, so
the joining machine reaches its end before the host leaves (fewer than the
0x3c steps after which the game asks for the departed player's controller). --join-binary runs another build for the
joining machine (--wine: a Windows build under Wine, prefix in /tmp;
cross-platform lockstep). Results: private/runs/net-check-TAG/result.json.
Research only.
"""
import argparse
import json
import os
import pathlib
import shutil
import subprocess
import sys
import time

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from virtual_display import environment  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parents[2]
CROSS, START, TRIANGLE, LEFT, RIGHT, UP, DOWN = 0xd, 0x14, 0xc, 0, 1, 2, 3
# Menu events (0x100 + gdl_play's NET_KEYS index + 1, or NET_TOGGLE).
M_UP, M_DOWN, M_ACCEPT, M_BACK, M_TOGGLE = 0x101, 0x102, 0x105, 0x106, 0x110
DUMPS = (300, 880)


def presses(first, gap, count, entry, length=2):
    return [[first + gap * k, first + gap * k + length - 1, [entry]] for k in range(count)]


def scripts():
    """Steps are network steps (GDL_NET_SCRIPT); the select screen ends near
    step 450 and update 300 is near step 520 (net-check-v4)."""
    host = presses(60, 45, 8, CROSS) + presses(560, 30, 16, TRIANGLE)
    join = presses(30, 1, 1, START) + presses(90, 45, 8, CROSS)
    # Both walk from step 700, turning every 100 steps; the host attacks;
    # triangle closes the scroll boxes walking may open (a realm door's).
    for k in range(16):
        host.append([700 + 100 * k, 799 + 100 * k, [[RIGHT, UP, LEFT, DOWN][k % 4]]])
        join.append([700 + 100 * k, 799 + 100 * k, [[LEFT, DOWN, RIGHT, UP][k % 4]]])
    host += presses(710, 20, 60, CROSS) + presses(1075, 50, 20, TRIANGLE)
    # The joining machine pauses everyone, moves in the shared menu, opens
    # Settings, goes back and resumes.
    join += [[950, 950, [M_TOGGLE]], [970, 970, [M_DOWN]], [980, 980, [M_UP]], [995, 995, [M_ACCEPT]], [1025, 1025, [M_BACK]], [1050, 1050, [M_BACK]]]
    return host, join


def machine(run, name, binary, env_extra, args, updates):
    d = run / name
    (d / 'saves').mkdir(parents=True)
    (d / 'home').mkdir()
    cfg = json.loads((ROOT / 'private/reports/play-config.json').read_text())
    cfg['saves'] = str(d / 'saves')
    (d / 'config.json').write_text(json.dumps(cfg))
    (d / 'actions.json').write_text(json.dumps([{'at': u, 'dump': f'u{u}'} for u in DUMPS]))
    if args.level:
        env_extra = dict(env_extra, GDL_DEBUG_LEVEL=args.level)
    env = dict(environment(), HOME=str(d / 'home'), GDL_NO_AUDIO='1', GDL_NO_ATTRACT='1', GDL_NO_GAMEPADS='1', GDL_BASELINE='1',
               GDL_EXIT_AFTER=str(updates), GDL_STATE_OUT=str(d / 'state.txt'), GDL_TEST_ACTIONS=str(d / 'actions.json'),
               GDL_NET_DELAY=str(args.delay), GDL_UI_TRACE=str(d / 'ui.trace'), GDL_RENDER_FPS='60', RUST_BACKTRACE='1',
               # llvmpipe's rasteriser threads (it takes every core by default).
               LP_NUM_THREADS='4', **env_extra)
    for k in ('WAYLAND_DISPLAY', 'XDG_DATA_HOME', 'GDL_SETTINGS'):
        env.pop(k, None)
    command = [binary, str(d / 'config.json')]
    if args.wine and name == 'join':
        prefix = pathlib.Path('/tmp/gdl-wine-net')
        # Wine refuses to create a prefix whose parent (/tmp) is not the user's.
        prefix.mkdir(exist_ok=True)
        # (No Mono or Gecko: their install prompts would wait on the display.)
        env.update(WINEPREFIX=str(prefix), WINEDEBUG='-all', WINEDLLOVERRIDES='mscoree,mshtml=',
                   # Wine's Direct3D 12 fails wgpu's compute pipelines here; Vulkan
                   # goes to lavapipe through winevulkan.
                   WGPU_BACKEND='vulkan')
        # The Windows build reads the same configuration through Z:. In a
        # Wine virtual desktop: on the bare Xvfb Wine's display modes lack
        # fields winit asserts (monitor.rs, net-check-v6-wine-monitor).
        command = ['wine', 'explorer', '/desktop=gdlnet,1280x960', binary, 'Z:' + str(d / 'config.json').replace('/', '\\')]
    log = open(d / 'client.log', 'w')
    return d, subprocess.Popen(command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT), log


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('tag')
    ap.add_argument('--binary', default=str(ROOT / 'target/release/gdl_play'))
    ap.add_argument('--join-binary')
    ap.add_argument('--wine', action='store_true')
    ap.add_argument('--delay', type=int, default=4)
    ap.add_argument('--updates', type=int, default=900)
    ap.add_argument('--host-extra', type=int, default=20, help='updates the host runs on (so the joining machine ends first)')
    ap.add_argument('--port', type=int, default=28790)
    ap.add_argument('--level', help='GDL_DEBUG_LEVEL on both machines: the level after the select screen instead of the Tower')
    ap.add_argument('--timeout', type=int, default=1500)
    args = ap.parse_args()
    run = ROOT / 'private/runs' / f'net-check-{args.tag}'
    if run.exists():
        raise SystemExit(f'{run} exists')
    run.mkdir(parents=True)
    host_script, join_script = scripts()
    (run / 'host-script.json').write_text(json.dumps(host_script))
    (run / 'join-script.json').write_text(json.dumps(join_script))
    binary = str(pathlib.Path(args.binary).resolve())
    join_binary = str(pathlib.Path(args.join_binary).resolve()) if args.join_binary else binary
    procs = [machine(run, 'host', binary, dict(GDL_NET_HOST=str(args.port), GDL_NET_MACHINES='2', GDL_NET_SCRIPT=str(run / 'host-script.json')), args, args.updates + args.host_extra)]
    time.sleep(3)
    procs.append(machine(run, 'join', join_binary, dict(GDL_NET_JOIN=f'127.0.0.1:{args.port}', GDL_NET_SCRIPT=str(run / 'join-script.json')), args, args.updates))
    deadline = time.time() + args.timeout
    while time.time() < deadline and any(p.poll() is None for _, p, _ in procs):
        time.sleep(1)
    timed_out = [d.name for d, p, _ in procs if p.poll() is None]
    for _, p, log in procs:
        if p.poll() is None:
            p.terminate()
            try:
                p.wait(10)
            except subprocess.TimeoutExpired:
                p.kill()
        log.close()
    if args.wine:
        shutil.rmtree('/tmp/gdl-wine-net', ignore_errors=True)
    fail = []
    hashes = {}
    for d, p, _ in procs:
        text = (d / 'client.log').read_text(errors='replace')
        panics = [ln for ln in text.splitlines() if 'panicked at' in ln]
        ended = [ln for ln in text.splitlines() if ln.startswith('network game:') and 'machine' not in ln]
        if p.returncode != 0 or panics or ended:
            fail.append(f'{d.name}: status {p.returncode}, panics {panics[:2]}, network {ended[:2]}')
        out = d / 'state.txt'
        hashes[d.name] = out.read_text().split() if out.exists() else []
    if timed_out:
        fail.append(f'timed out: {timed_out}')
    h, j = hashes['host'][:args.updates], hashes['join']
    first_diff = next((k for k, (a, b) in enumerate(zip(h, j)) if a != b), None)
    if len(h) != args.updates or len(j) != args.updates:
        fail.append(f'updates: host {len(h)}, join {len(j)} of {args.updates}')
    if first_diff is not None:
        fail.append(f'states differ from update {first_diff + 1}')
    dumps = {}
    for d, _, _ in procs:
        for u in DUMPS:
            f = d / f'state.u{u}.json'
            if f.exists():
                dumps[(d.name, u)] = [(p['state'], p['port'], p['name'], p['pos']) for p in json.loads(f.read_text())['players']]
    for u in DUMPS:
        if dumps.get(('host', u)) is None or dumps.get(('host', u)) != dumps.get(('join', u)):
            fail.append(f'dumps at {u} differ or are missing')
    players = {u: dumps.get(('host', u)) for u in DUMPS}
    first, last = players[DUMPS[0]] or [], players[DUMPS[1]] or []
    ports = sorted(port for state, port, _, _ in first if state != 0)
    if ports != [0, 1]:
        fail.append(f'players in play on ports {ports}, expected [0, 1]')
    moved = [k for k in range(min(len(first), len(last))) if first[k][0] != 0 and first[k][3] != last[k][3]]
    if moved != [0, 1]:
        fail.append(f'players that moved between the dumps: {moved}')
    # The joining machine's pause showed on both.
    for d, _, _ in procs:
        trace = (d / 'ui.trace').read_text(errors='replace') if (d / 'ui.trace').exists() else ''
        if 'paused=true Paused' not in trace or 'paused=true Options' not in trace:
            fail.append(f'{d.name}: the shared pause menu and Settings not in its trace')
    result = dict(passed=not fail, failures=fail, binary=binary, join_binary=join_binary, wine=args.wine, delay=args.delay,
                  updates=[len(h), len(j)], equal_updates=first_diff if first_diff is not None else min(len(h), len(j)),
                  last_hash=h[-1] if h else None, players=players)
    (run / 'result.json').write_text(json.dumps(result, indent=1))
    print('PASS' if not fail else 'FAIL', fail, players)
    sys.exit(0 if not fail else 1)


if __name__ == '__main__':
    main()
