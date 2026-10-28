#!/usr/bin/env python3
"""M20 performance benchmark: gdl_play on the real GPU, off the desktop.

  perf_bench.py TAG [--binary PATH] [--scenes NAME ...] [--updates N]
                [--width W --height H] [--baseline] [--env K=V ...]

Each scene runs the client in its own `kwin_wayland --virtual --xwayland`
session (an invisible output whose Xwayland presents through DRI3, so the
client renders and presents on the machine's GPU — RADV here — without a
window on the user's desktop; the private Xvfb display cannot present a
hardware Vulkan swapchain). GDL_TEST_AUTOSTART creates the test character,
GDL_DEBUG_LEVEL picks the level, GDL_INPUT_SCRIPT drives port 0 for walking
scenes, GDL_EXIT_AFTER ends the run after N stepped updates and
GDL_FRAME_LOG records every frame (the client's diagnostic; written to a
tmpfs scratch file so disk writeback cannot stall the frame loop).

Report private/reports/perf-bench-TAG.json: the binary's SHA-256, the
adapter line the client logged, the window size, and per scene the frame
time statistics after the first 60 frames (mean, median, p5/p95/p99, max,
frames over 16.7 / 33.4 / 50 ms), sim updates per frame, the load time to
the first update and the peak resident memory (VmHWM) the client prints.
"""
import argparse
import hashlib
import json
import os
import pathlib
import statistics
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
CONFIG = ROOT / 'private/reports/play-config.json'
RADV = '/usr/share/vulkan/icd.d/radeon_icd.json'

# name: (GDL_DEBUG_LEVEL or None for the Tower, input script or None, note)
SCENES = {
    'tower': (None, None, 'the Tower at the start of a new game: crystals, fire bowls, Sumner'),
    'g1-walk': ('G1', [[200, 600, [1]], [601, 900, [2]]], 'Poison Fields: walking right then up through enemies'),
    'c2': ('C2', None, 'Desert C2 start: GRU generators and swarms'),
    'f1': ('F1', None, 'Hell F1 start: 181 generators in the level'),
    'i1': ('I1', None, 'Ice I1 start'),
    'j5': ('J5', None, 'Dream J5: the wraith boss arena'),
    'h4': ('H4', None, 'Battle H4: Garm, the last boss'),
}


def sha256(p):
    h = hashlib.sha256()
    with open(p, 'rb') as f:
        for chunk in iter(lambda: f.read(1 << 20), b''):
            h.update(chunk)
    return h.hexdigest()


def stats(frames_log):
    rows = [line.split() for line in open(frames_log) if line.strip()]
    ms = [float(r[1]) for r in rows][60:]
    if not ms:
        return {'frames': 0}
    s = sorted(ms)
    ups = [int(r[3]) for r in rows]
    per = {}
    for a, b in zip(ups, ups[1:]):
        per[b - a] = per.get(b - a, 0) + 1
    pct = lambda q: round(s[min(len(s) - 1, int(len(s) * q))], 2)
    return {
        'frames': len(ms),
        'mean_ms': round(statistics.mean(ms), 2), 'median_ms': round(statistics.median(ms), 2),
        'p5_ms': pct(0.05), 'p95_ms': pct(0.95), 'p99_ms': pct(0.99), 'max_ms': round(max(ms), 2),
        'over_16_7': sum(1 for m in ms if m > 16.7), 'over_33_4': sum(1 for m in ms if m > 33.4), 'over_50': sum(1 for m in ms if m > 50.0),
        'fps_mean': round(1000.0 / statistics.mean(ms), 1),
        'updates_per_frame': {str(k): v for k, v in sorted(per.items())},
        'entities_last': int(rows[-1][2]) if len(rows[-1]) > 2 else None,
    }


def run_scene(tag, name, binary, updates, width, height, extra_env, scratch):
    level, script, note = SCENES[name]
    d = scratch / name
    d.mkdir(parents=True, exist_ok=True)
    env_lines = {
        'VK_DRIVER_FILES': RADV, 'VK_ICD_FILENAMES': RADV, 'GDL_NO_AUDIO': '1', 'GDL_NO_GAMEPADS': '1',
        'GDL_TEST_AUTOSTART': '1', 'GDL_EXIT_AFTER': str(updates), 'GDL_FRAME_LOG': str(d / 'frames.log'),
    }
    if level:
        env_lines['GDL_DEBUG_LEVEL'] = level
    if script:
        (d / 'input.json').write_text(json.dumps(script))
        env_lines['GDL_INPUT_SCRIPT'] = str(d / 'input.json')
    env_lines.update(extra_env)
    session = d / 'session.sh'
    exports = ''.join(f"export {k}='{v}'\n" for k, v in env_lines.items())
    session.write_text(f"""#!/bin/bash
cd '{ROOT}'
unset WAYLAND_DISPLAY
{exports}timeout 900 '{binary}' '{CONFIG}' > '{d / "client.log"}' 2>&1
echo "exit $?" > '{d / "exit.txt"}'
""")
    session.chmod(0o755)
    socket = f'gdl-bench-{os.getpid()}-{name}'
    subprocess.run(['kwin_wayland', '--virtual', '--xwayland', '--socket', socket, '--width', str(width), '--height', str(height),
                    '--no-lockscreen', '--exit-with-session', str(session)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=1200)
    log = (d / 'client.log').read_text(errors='replace') if (d / 'client.log').exists() else ''
    out = {'scene': name, 'level': level or 'TOWER', 'note': note, 'input_script': script,
           'exit': (d / 'exit.txt').read_text().strip() if (d / 'exit.txt').exists() else 'missing'}
    out.update(stats(d / 'frames.log') if (d / 'frames.log').exists() else {'frames': 0})
    for line in log.splitlines():
        if line.startswith('PERF load_to_first_update_ms'):
            out['load_to_first_update_ms'] = float(line.split()[-1])
        elif line.startswith('PERF VmHWM'):
            out['peak_rss_kb'] = int(line.split()[2])
        elif 'AdapterInfo' in line:
            out['adapter'] = line[line.index('AdapterInfo'):][:200]
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('tag')
    ap.add_argument('--binary', default=str(ROOT / 'target/release/gdl_play'))
    ap.add_argument('--scenes', nargs='*', default=list(SCENES))
    ap.add_argument('--updates', type=int, default=900)
    ap.add_argument('--width', type=int, default=1280)
    ap.add_argument('--height', type=int, default=896)
    ap.add_argument('--baseline', action='store_true', help='GDL_BASELINE=1: the original presentation (no blending)')
    ap.add_argument('--env', nargs='*', default=[], help='extra client environment K=V')
    a = ap.parse_args()
    report = ROOT / 'private/reports' / f'perf-bench-{a.tag}.json'
    if report.exists():
        raise SystemExit(f'{report} exists')
    extra = dict(kv.split('=', 1) for kv in a.env)
    if a.baseline:
        extra['GDL_BASELINE'] = '1'
    scratch = pathlib.Path(tempfile.mkdtemp(prefix=f'perf-{a.tag}-'))
    results = []
    for name in a.scenes:
        r = run_scene(a.tag, name, pathlib.Path(a.binary).resolve(), a.updates, a.width, a.height, extra, scratch)
        print(json.dumps({k: r.get(k) for k in ('scene', 'exit', 'frames', 'mean_ms', 'median_ms', 'p95_ms', 'p99_ms', 'max_ms', 'over_33_4', 'load_to_first_update_ms', 'peak_rss_kb')}), flush=True)
        results.append(r)
    uname = subprocess.run(['uname', '-r'], capture_output=True, text=True).stdout.strip()
    cpu = next((l.split(':', 1)[1].strip() for l in open('/proc/cpuinfo') if l.startswith('model name')), '?')
    report.write_text(json.dumps({
        'scope': 'M20 performance benchmark (actual disc; real GPU through a virtual kwin session; DEBUG test character and level start)',
        'binary': str(a.binary), 'binary_sha256': sha256(a.binary), 'window': [a.width, a.height], 'kernel': uname, 'cpu': cpu,
        'updates': a.updates, 'env': extra, 'scenes': results, 'scratch': str(scratch),
    }, indent=1) + '\n')
    print(report)


if __name__ == '__main__':
    main()
