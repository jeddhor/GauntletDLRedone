#!/usr/bin/env python3
"""Compare a native scene with the reference at the reference's camera.

  scene_compare.py LEVEL SOURCE TAG [--steps N]

SOURCE is a bound PCSX2 slot number (reference_slots.py) or a preserved
capture name (staged through slot 9). The reference frame (N idle vblanks
after the reload, default 400) and its window camera go to
private/reports/scene-TAG-ref.png / -camera.json (kept and reused when they
exist); then gdl_play starts LEVEL (GDL_DEBUG_LEVEL, a new character) with
that camera pinned (GDL_DEBUG_CAMERA) through client_driver.py, and its frame
about 12 s into the level goes to private/reports/scene-TAG-native.png
(frames at 12 and 20 s in private/runs/client-sceneTAG/). The native moment
is not the reference's: compare layout, models and lighting, not animation
phase or enemy positions. ImageMagick's `convert`/`montage` build
private/reports/scene-TAG.png (reference left, native right). Research only.
"""
import argparse
import json
import os
import pathlib
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
PY = sys.executable

STEPS = [["wait", 30], ["key", "Return", 0.8], ["wait", 4], ["key", "Return", 0.8], ["wait", 3],
         ["key", "Return", 0.8], ["wait", 10],
         ["key", "space", 0.6], ["wait", 4], ["key", "space", 0.6], ["wait", 4], ["key", "space", 0.6], ["wait", 12],
         ["shot", "t12"], ["key", "F2", 0.2], ["wait", 0.5], ["shot", "native"], ["wait", 8], ["shot", "t20"], ["quit"]]


def run(*args, env=None):
    subprocess.run(args, cwd=ROOT, check=True, env=env)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('level')
    parser.add_argument('source')
    parser.add_argument('tag')
    parser.add_argument('--steps', type=int, default=400)
    args = parser.parse_args()
    if not args.tag.replace('-', '').isalnum():
        raise SystemExit('tag must be alphanumeric')
    reports = ROOT / 'private' / 'reports'
    ref, cam = reports / f'scene-{args.tag}-ref.png', reports / f'scene-{args.tag}-camera.json'
    native, sheet = reports / f'scene-{args.tag}-native.png', reports / f'scene-{args.tag}.png'
    sel = ['--slot', args.source] if args.source.isdigit() else ['--stage', args.source]
    if not ref.exists():
        run(PY, 'tools/python/reference_screenshot.py', *sel, '--steps', str(args.steps), '--out', str(ref))
    if not cam.exists():
        run(PY, 'tools/python/reference_camera.py', '--out', str(cam))
    tag = f'scene{args.tag}'
    out = ROOT / 'private' / 'runs' / f'client-{tag}'
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)
    steps = out / 'steps.json'
    steps.write_text(json.dumps(STEPS))
    env = dict(os.environ, GDL_DEBUG_LEVEL=args.level, GDL_DEBUG_CAMERA=str(cam))
    run(PY, 'tools/python/client_driver.py', 'serve', tag, str(steps), env=env)
    frame = out / '002-native.png'
    if not frame.exists():
        raise SystemExit(f'{frame}: the client did not reach the frame')
    if native.exists():
        native.unlink()
    shutil.copyfile(frame, native)
    tmp = out / 'ref-trim.png'
    run('convert', str(ref), '-trim', '+repage', '-resize', '800x', str(tmp))
    tmp2 = out / 'native-small.png'
    run('convert', str(native), '-resize', '800x', str(tmp2))
    if sheet.exists():
        sheet.unlink()
    run('montage', str(tmp), str(tmp2), '-tile', '2x1', '-geometry', '+4+4', str(sheet))
    print(sheet)


if __name__ == '__main__':
    main()
