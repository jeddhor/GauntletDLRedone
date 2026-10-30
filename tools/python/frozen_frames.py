#!/usr/bin/env python3
"""Frozen frames of several levels on the real GPU, for renderer comparisons.

  frozen_frames.py TAG [--binary PATH] [--levels G1 J5 ...]
                   [--baseline | --settings FILE] [--size 1920x1080]
                   [--freeze 200] [--frames 500,700]

Inside one `kwin_wayland --virtual --xwayland` output of --size on the
real GPU (RADV; nothing reaches the user's desktop), gdl_play starts each
level with a new TEST character (GDL_TEST_AUTOSTART, GDL_DEBUG_LEVEL; TOWER:
a new game with the welcome scrolls closed by a menu script), its
own HOME and empty saves directory, no audio and no controllers; the
simulation stops at update --freeze (GDL_FREEZE_AT: texmods and particles
then follow stepped updates, so frames are independent of frame timing)
and the client saves rendered frames --frames (GDL_SHOT_FRAMES; counted
from the start, GDL_RENDER_FPS 60; the Tower's 300 later) before it is stopped. --baseline runs
the original presentation (GDL_BASELINE=1, a 1280×896 window); --settings
passes a settings file (GDL_SETTINGS: read, never written).

Frames: private/runs/frozen-TAG/<LEVEL>/frame-NNNNN.png; a summary with
each frame's SHA-256 in private/runs/frozen-TAG/frames.json. Compare two
tags with --compare OTHER (pixel-identical or the differing pixel count
and mean difference per level, Pillow). Research only.
"""
import argparse
import hashlib
import json
import os
import pathlib
import shlex
import shutil
import subprocess
import sys
import time

ROOT = pathlib.Path(__file__).resolve().parents[2]
RADV = '/usr/share/vulkan/icd.d/radeon_icd.json'
LEVELS = ['TOWER', 'G1', 'J5', 'C2', 'H4']
# TOWER: a new game (no GDL_DEBUG_LEVEL) with triangle every 30 menu ticks
# from tick 30, eight times, closing the welcome scrolls (perf_bench.py).
WELCOME = [[30 + 30 * k, 32 + 30 * k, [0xc]] for k in range(8)]


def capture(args, out):
    """Run every level (inside the kwin session)."""
    summary = {'binary': str(args.binary), 'binary_sha256': hashlib.sha256(pathlib.Path(args.binary).read_bytes()).hexdigest(),
               'baseline': args.baseline, 'settings': args.settings, 'size': args.size, 'freeze': args.freeze, 'levels': {}}
    if args.settings:
        summary['settings_json'] = json.loads(pathlib.Path(args.settings).read_text())
    for level in args.levels:
        run = out / level
        if run.exists():
            shutil.rmtree(run)
        (run / 'saves').mkdir(parents=True)
        (run / 'home').mkdir()
        cfg = json.loads((ROOT / 'private/reports/play-config.json').read_text())
        cfg['saves'] = str(run / 'saves')
        (run / 'config.json').write_text(json.dumps(cfg))
        env = dict(os.environ, VK_DRIVER_FILES=RADV, VK_ICD_FILENAMES=RADV, HOME=str(run / 'home'), GDL_NO_AUDIO='1',
                   GDL_NO_ATTRACT='1', GDL_NO_GAMEPADS='1', GDL_TEST_AUTOSTART='1',
                   GDL_FREEZE_AT=str(args.freeze), GDL_SHOT_FRAMES=args.frames, GDL_SHOT_DIR=str(run), GDL_RENDER_FPS='60')
        if level == 'TOWER':
            (run / 'menu.json').write_text(json.dumps(WELCOME))
            env['GDL_MENU_SCRIPT'] = str(run / 'menu.json')
        else:
            env['GDL_DEBUG_LEVEL'] = level
        env.pop('WAYLAND_DISPLAY', None)
        env.pop('XDG_DATA_HOME', None)
        env.pop('GDL_SETTINGS', None)
        env.pop('GDL_BASELINE', None)
        if args.baseline:
            env['GDL_BASELINE'] = '1'
        if args.settings:
            env['GDL_SETTINGS'] = str(pathlib.Path(args.settings).resolve())
        # The Tower's welcome scrolls hold the updates: its shots come later.
        frames = [int(f) + (300 if level == 'TOWER' else 0) for f in args.frames.split(',')]
        env['GDL_SHOT_FRAMES'] = ','.join(str(f) for f in frames)
        log = open(run / 'client.log', 'w')
        client = subprocess.Popen([str(args.binary), str(run / 'config.json')], cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
        last = run / f'frame-{frames[-1]:05d}.png'
        deadline = time.time() + 120
        while time.time() < deadline and client.poll() is None and not last.exists():
            time.sleep(0.5)
        time.sleep(1.5)
        if client.poll() is None:
            client.terminate()
            try:
                client.wait(10)
            except subprocess.TimeoutExpired:
                client.kill()
        log.close()
        shots = {}
        for f in frames:
            p = run / f'frame-{f:05d}.png'
            shots[f] = hashlib.sha256(p.read_bytes()).hexdigest() if p.exists() else None
        summary['levels'][level] = shots
        print(level, shots, flush=True)
    (out / 'frames.json').write_text(json.dumps(summary, indent=1))


def compare(out, other):
    """Per level and frame: identical, or the differing pixels."""
    from PIL import Image, ImageChops, ImageStat
    a, b = json.loads((out / 'frames.json').read_text()), json.loads((other / 'frames.json').read_text())
    for level, shots in a['levels'].items():
        for f, h in shots.items():
            h2 = b['levels'].get(level, {}).get(f)
            if h is None or h2 is None:
                print(level, f, 'missing')
                continue
            if h == h2:
                print(level, f, 'identical')
                continue
            ia = Image.open(out / level / f'frame-{int(f):05d}.png').convert('RGB')
            ib = Image.open(other / level / f'frame-{int(f):05d}.png').convert('RGB')
            if ia.size != ib.size:
                print(level, f, 'size', ia.size, ib.size)
                continue
            d = ImageChops.difference(ia, ib)
            # A pixel differs where any channel does (the max of the three).
            cr, cg, cb = d.split()
            px = ImageChops.lighter(ImageChops.lighter(cr, cg), cb).point(lambda v: 255 if v else 0).histogram()[255]
            mean = sum(ImageStat.Stat(d).mean) / 3
            print(level, f, f'{px} pixels differ, mean {mean:.3f}/255')


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('tag')
    ap.add_argument('--binary', default=str(ROOT / 'target/release/gdl_play'))
    ap.add_argument('--levels', nargs='+', default=LEVELS)
    ap.add_argument('--baseline', action='store_true')
    ap.add_argument('--settings')
    ap.add_argument('--size', default='1920x1080')
    ap.add_argument('--freeze', type=int, default=200)
    ap.add_argument('--frames', default='500,700')
    ap.add_argument('--compare', help='another TAG to compare with (no capture)')
    ap.add_argument('--inner', action='store_true', help=argparse.SUPPRESS)
    args = ap.parse_args()
    if not args.tag.replace('-', '').isalnum():
        raise SystemExit('tag must be alphanumeric')
    out = ROOT / 'private' / 'runs' / f'frozen-{args.tag}'
    if args.compare:
        compare(out, ROOT / 'private' / 'runs' / f'frozen-{args.compare}')
        return
    if args.inner:
        capture(args, out)
        return
    out.mkdir(parents=True, exist_ok=True)
    args.binary = str(pathlib.Path(args.binary).resolve())
    w, h = args.size.split('x')
    inner = [sys.executable, str(pathlib.Path(__file__).resolve()), args.tag, '--inner', '--binary', args.binary, '--size', args.size,
             '--freeze', str(args.freeze), '--frames', args.frames, '--levels', *args.levels]
    if args.baseline:
        inner.append('--baseline')
    if args.settings:
        inner += ['--settings', str(pathlib.Path(args.settings).resolve())]
    session = out / 'session.sh'
    session.write_text(f"""#!/bin/bash
unset WAYLAND_DISPLAY
cd {shlex.quote(str(ROOT))}
{' '.join(shlex.quote(c) for c in inner)} > {shlex.quote(str(out / 'session.log'))} 2>&1
""")
    session.chmod(0o755)
    env = dict(os.environ)
    env.pop('WAYLAND_DISPLAY', None)
    subprocess.run(['kwin_wayland', '--virtual', '--xwayland', '--socket', f'gdl-frozen-{os.getpid()}', '--width', w, '--height', h,
                    '--no-lockscreen', '--exit-with-session', str(session)], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    print((out / 'session.log').read_text())


if __name__ == '__main__':
    main()
