#!/usr/bin/env python3
"""M20 long co-op session: several players on virtual gamepads for a long
run on the real GPU, watching memory, entities, frame time and progress.

  soak_test.py TAG [--binary PATH] [--minutes M] [--players N] [--level L]
               [--width W --height H] [--settings FILE] [--seed S]

One gdl_play session in a `kwin_wayland --virtual --xwayland` output
(RADV): N virtual uinput gamepads ("gdl-test-pad-k"; the client sees only
them, GDL_GAMEPAD_ONLY), the test character in LEVEL (GDL_TEST_AUTOSTART,
GDL_DEBUG_LEVEL), players 2..N joined as copies of player 1 on the pads'
ports (GDL_TEST_PLAYERS; TEST), every player's health written to 5000
every 300 updates so the session does not end in deaths (DEBUG), and each
pad driven by a seeded random walk with cross/circle presses. Every 10 s
the client's resident memory (VmRSS) and the frame log's last line are
sampled; one screenshot at 50 s. The frame log is written to a tmpfs
scratch file (TMPDIR) and copied to the run directory at the end.

Report private/reports/soak-TAG.json: the binary's SHA-256, per 5-minute
window the frame-time statistics, updates stepped and entities, the memory
samples, and whether the client was alive at the end; frames.log and the
client log in private/runs/soak-TAG/.
"""
import argparse
import hashlib
import json
import os
import pathlib
import random
import statistics
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = pathlib.Path(__file__).resolve().parents[2]
RADV = '/usr/share/vulkan/icd.d/radeon_icd.json'


def sha256(p):
    h = hashlib.sha256()
    with open(p, 'rb') as f:
        for chunk in iter(lambda: f.read(1 << 20), b''):
            h.update(chunk)
    return h.hexdigest()


def pads(n):
    from evdev import AbsInfo, UInput, ecodes as e
    cap = {e.EV_KEY: [e.BTN_SOUTH, e.BTN_EAST, e.BTN_NORTH, e.BTN_WEST, e.BTN_TL, e.BTN_TR, e.BTN_SELECT, e.BTN_START, e.BTN_MODE, e.BTN_THUMBL, e.BTN_THUMBR],
           e.EV_ABS: [(e.ABS_X, AbsInfo(0, -32768, 32767, 16, 128, 0)), (e.ABS_Y, AbsInfo(0, -32768, 32767, 16, 128, 0)),
                      (e.ABS_RX, AbsInfo(0, -32768, 32767, 16, 128, 0)), (e.ABS_RY, AbsInfo(0, -32768, 32767, 16, 128, 0)),
                      (e.ABS_Z, AbsInfo(0, 0, 255, 0, 0, 0)), (e.ABS_RZ, AbsInfo(0, 0, 255, 0, 0, 0)),
                      (e.ABS_HAT0X, AbsInfo(0, -1, 1, 0, 0, 0)), (e.ABS_HAT0Y, AbsInfo(0, -1, 1, 0, 0, 0))]}
    out = []
    for k in range(n):
        out.append(UInput(cap, name=f'gdl-test-pad-{k}', vendor=0x1209, product=0x0001, version=0x100))
        time.sleep(0.3)
    return out, e


def inside(run, binary, a):
    rng = random.Random(a['seed'])
    devices, e = pads(a['players'])
    time.sleep(1.5)
    cfg = json.loads((ROOT / 'private/reports/play-config.json').read_text())
    (run / 'saves').mkdir(exist_ok=True)
    cfg['saves'] = str(run / 'saves')
    (run / 'config.json').write_text(json.dumps(cfg))
    actions = [{'at': u, 'health': 5000, 'player': k} for u in range(150, 200000, 300) for k in range(a['players'])]
    (run / 'actions.json').write_text(json.dumps(actions))
    env = dict(os.environ, VK_DRIVER_FILES=RADV, VK_ICD_FILENAMES=RADV, GDL_NO_AUDIO='1', GDL_GAMEPAD_ONLY='gdl-test-pad',
               GDL_TEST_AUTOSTART='1', GDL_DEBUG_LEVEL=a['level'], GDL_TEST_PLAYERS=str(a['players']),
               GDL_FRAME_LOG=a['log'], GDL_TEST_ACTIONS=str(run / 'actions.json'))
    if a.get('settings'):
        env['GDL_SETTINGS'] = a['settings']
    env.pop('WAYLAND_DISPLAY', None)
    client = subprocess.Popen([str(binary), str(run / 'config.json')], cwd=ROOT, env=env,
                              stdout=open(run / 'client.log', 'w'), stderr=subprocess.STDOUT)
    samples = []
    hats = [(0, 0), (0, -1), (1, -1), (1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1)]
    start = time.time()
    last_sample = 0.0
    try:
        while time.time() - start < a['minutes'] * 60 and client.poll() is None:
            for d in devices:
                if rng.random() < 0.3:
                    x, y = rng.choice(hats)
                    d.write(e.EV_ABS, e.ABS_HAT0X, x)
                    d.write(e.EV_ABS, e.ABS_HAT0Y, y)
                for code, p in ((e.BTN_SOUTH, 0.4), (e.BTN_EAST, 0.05)):
                    d.write(e.EV_KEY, code, 1 if rng.random() < p else 0)
                d.syn()
            time.sleep(0.5)
            now = time.time() - start
            if 50 <= now and not (run / 'at-50s.png').exists():
                win = subprocess.run(['xdotool', 'search', '--name', '^Gauntlet: Dark Legacy$'], capture_output=True, text=True).stdout.split()
                if win:
                    subprocess.run(['import', '-window', win[0], str(run / 'at-50s.png')], capture_output=True)
            if now - last_sample >= 10:
                last_sample = now
                try:
                    status = pathlib.Path(f'/proc/{client.pid}/status').read_text()
                    rss = int(next(l.split()[1] for l in status.splitlines() if l.startswith('VmRSS')))
                except (FileNotFoundError, StopIteration):
                    rss = None
                line = ''
                try:
                    with open(a['log'], 'rb') as f:
                        f.seek(max(0, f.seek(0, 2) - 400))
                        line = f.read().decode(errors='replace').strip().splitlines()[-1]
                except (FileNotFoundError, IndexError):
                    pass
                cols = line.split()
                samples.append({'t': round(now), 'rss_kb': rss, 'updates': int(cols[3]) if len(cols) > 3 else None,
                                'entities': int(cols[2]) if len(cols) > 2 else None})
    finally:
        alive = client.poll() is None
        if alive:
            client.terminate()
            try:
                client.wait(15)
            except subprocess.TimeoutExpired:
                client.kill()
        for d in devices:
            d.close()
    (run / 'inside.json').write_text(json.dumps({'samples': samples, 'alive_at_end': alive, 'seconds': round(time.time() - start)}) + '\n')


def windows(frames_log, seconds=300):
    rows = [l.split() for l in open(frames_log) if l.strip()]
    out, t, cur = [], 0.0, []
    for r in rows[60:]:
        ms = float(r[1])
        t += ms / 1000.0
        cur.append((ms, int(r[3]), int(r[2])))
        if seconds <= t:
            out.append(cur)
            cur, t = [], 0.0
    if cur:
        out.append(cur)
    res = []
    for k, w in enumerate(out):
        ms = sorted(x[0] for x in w)
        pct = lambda q: round(ms[min(len(ms) - 1, int(len(ms) * q))], 2)
        res.append({'window': k, 'frames': len(w), 'mean_ms': round(statistics.mean(ms), 2), 'p95_ms': pct(0.95), 'p99_ms': pct(0.99),
                    'max_ms': round(ms[-1], 2), 'over_33_4': sum(1 for m in ms if m > 33.4), 'updates': w[-1][1] - w[0][1],
                    'entities_last': w[-1][2]})
    return res


def main():
    if len(sys.argv) > 1 and sys.argv[1] == '--inside':
        return inside(pathlib.Path(sys.argv[2]), pathlib.Path(sys.argv[3]), json.loads(sys.argv[4]))
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('tag')
    ap.add_argument('--binary', default=str(ROOT / 'target/release/gdl_play'))
    ap.add_argument('--minutes', type=float, default=30)
    ap.add_argument('--players', type=int, default=4)
    ap.add_argument('--level', default='G1')
    ap.add_argument('--width', type=int, default=1280)
    ap.add_argument('--height', type=int, default=924)
    ap.add_argument('--settings', help='GDL_SETTINGS file for the client')
    ap.add_argument('--seed', type=int, default=20)
    a = ap.parse_args()
    report = ROOT / 'private/reports' / f'soak-{a.tag}.json'
    if report.exists():
        raise SystemExit(f'{report} exists')
    run = ROOT / 'private/runs' / f'soak-{a.tag}'
    run.mkdir(parents=True, exist_ok=False)
    binary = pathlib.Path(a.binary).resolve()
    # The frame log on tmpfs (TMPDIR): disk writeback must not stall frames.
    scratch = pathlib.Path(tempfile.mkdtemp(prefix=f'soak-{a.tag}-'))
    args = {'minutes': a.minutes, 'players': a.players, 'level': a.level, 'seed': a.seed,
            'settings': str(pathlib.Path(a.settings).resolve()) if a.settings else None, 'log': str(scratch / 'frames.log')}
    session = run / 'session.sh'
    session.write_text(f"""#!/bin/bash
cd '{ROOT}'
unset WAYLAND_DISPLAY
python3 '{pathlib.Path(__file__).resolve()}' --inside '{run}' '{binary}' '{json.dumps(args)}' > '{run / "driver.log"}' 2>&1
""")
    session.chmod(0o755)
    subprocess.run(['kwin_wayland', '--virtual', '--xwayland', '--socket', f'gdl-soak-{os.getpid()}', '--width', str(a.width), '--height', str(a.height),
                    '--no-lockscreen', '--exit-with-session', str(session)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                   timeout=a.minutes * 60 + 600)
    if (scratch / 'frames.log').exists():
        shutil.copyfile(scratch / 'frames.log', run / 'frames.log')
    inner = json.loads((run / 'inside.json').read_text()) if (run / 'inside.json').exists() else {}
    rss = [s['rss_kb'] for s in inner.get('samples', []) if s.get('rss_kb')]
    log = (run / 'client.log').read_text(errors='replace') if (run / 'client.log').exists() else ''
    report.write_text(json.dumps({
        'scope': 'M20 long co-op session (actual disc, real GPU via a virtual kwin session, Linux; virtual uinput gamepads, '
                 'TEST players joined as copies, DEBUG health writes)',
        'binary': str(binary), 'binary_sha256': sha256(binary), 'args': args, 'window': [a.width, a.height],
        'alive_at_end': inner.get('alive_at_end'), 'seconds': inner.get('seconds'),
        'rss_kb': {'first': rss[0] if rss else None, 'last': rss[-1] if rss else None, 'max': max(rss) if rss else None},
        'windows_5min': windows(run / 'frames.log') if (run / 'frames.log').exists() else [],
        'simulation_stopped': 'SIMULATION STOPPED' in log or 'panicked' in log,
        'samples': inner.get('samples', []),
    }, indent=1) + '\n')
    print(report)


if __name__ == '__main__':
    main()
