#!/usr/bin/env python3
"""The original-presentation gate: a build's frozen frames against 1.0.

  baseline_gate.py TAG [--binary PATH] [--levels TOWER G1 ...] [--retries 2]

Runs frozen_frames.py TAG --baseline (all enhancements off; real GPU,
nothing on the user's desktop) with the scene dump diagnostic
(GDL_SCENE_DUMP) at the shot frames, then judges every level and frame
against the 1.0 build's frames (private/runs/frozen-base-ref) and scene
dumps (private/runs/frozen-dump-ref, binary gdl_play-d100e5f):

  identical    the frame is pixel-identical to 1.0;
  draw order   pixels differ but the scene dump is equal: every drawn mesh,
               transform, texture and material parameter is the same, only
               the order of tied draws differs (G1's spearmen: Bevy breaks
               equal-depth ties by its own render schedule; the user's
               decision, Checkpoint C 2026-10-31);
  particles    only particle sprites differ (meshes, frames): the harness
               steps particles per update but which emitters advance in a
               catch-up follows frame timing — the 1.0 binary itself lands
               in either state (H4). The level is captured again up to
               --retries times and passes if a retry is identical or
               draw-order;
  fixed        as draw order, but for draws 1.0 made that a fidelity fix since
               removed (REMOVED_SINCE_1_0: each named by its mesh and texture
               content and its reason); the pixels may also differ where
               those draws showed;
  different    anything else: the gate fails.

Dumps are compared as line multisets after mapping the current format to
1.0's: the image fields added after 1.0 (mip level count, sampler) are
dropped, and particle sprites' light_dir.w −1 (Phase 4: psys sprites take
no VU1 lighting, enhancements.md "Lighting") reads as 1.0's 0.

--judge-only judges an existing capture of TAG again (no retries).
Report private/reports/baseline-gate-TAG.json; exit status 1 when a level
fails. Research tooling: frames and dumps stay in private/runs.
"""
import argparse
import collections
import json
import os
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
RUNS = ROOT / 'private' / 'runs'
LEVELS = ['TOWER', 'G1', 'J5', 'C2', 'H4']
# frozen_frames.py's shots (the Tower's 300 later).
SHOTS = {lv: ([800, 1000] if lv == 'TOWER' else [500, 700]) for lv in LEVELS}
IMAGE = re.compile(r'Some\(\((\d+), (\d+), (Some\(\d+\)|None), \d+, (?:Some\(\([^()]*\)\)|None)\)\)')
PSYS = 'light_dir: Vec4(0.0, 0.0, 0.0, -1.0)'
# Draws of 1.0 that later fidelity fixes removed: (text in a 1.0 dump line —
# mesh and base texture by content —, the fix). Only 1.0's lines may match.
REMOVED_SINCE_1_0 = [
    ('mesh Some((4, 1865697815464219562)) base Some((8, 8, Some(11174574554518817798)))',
     "kind-2 items' NULL1 node, the chests' content anchor quad, hidden by SetItem (MBTreeSetFlags 1; dd03c26, research/player.md)"),
]


def norm(line):
    return IMAGE.sub(r'Some((\1, \2, \3))', line).replace(PSYS, 'light_dir: Vec4(0.0, 0.0, 0.0, 0.0)')


def pixels(a, b):
    from PIL import Image, ImageChops
    ia, ib = Image.open(a).convert('RGB'), Image.open(b).convert('RGB')
    if ia.size != ib.size:
        return -1
    r, g, bl = ImageChops.difference(ia, ib).split()
    return ImageChops.lighter(ImageChops.lighter(r, g), bl).point(lambda v: 255 if v else 0).histogram()[255]


def judge(run, level, frame, ref_frames, ref_dumps):
    """(verdict, details) for one level and frame of a capture."""
    shot = run / level / f'frame-{frame:05d}.png'
    ref = ref_frames / level / f'frame-{frame:05d}.png'
    if not shot.exists() or not ref.exists():
        return 'missing', {}
    px = pixels(shot, ref)
    if px == 0:
        return 'identical', {}
    dump, ref_dump = run / level / f'scene-{frame:05d}.txt', ref_dumps / level / f'scene-{frame:05d}.txt'
    if not dump.exists() or not ref_dump.exists():
        return 'different', {'pixels': px, 'dump': 'missing'}
    cur = dump.read_text().splitlines()
    a = collections.Counter(ref_dump.read_text().splitlines())
    b = collections.Counter(norm(line) for line in cur)
    only_ref, only_now = a - b, b - a
    removed = collections.Counter({line: n for line, n in only_ref.items() if any(m in line for m, _ in REMOVED_SINCE_1_0)})
    only_ref -= removed
    if not only_ref and not only_now:
        if removed:
            return 'fixed', {'pixels': px, 'removed': sum(removed.values()),
                             'fixes': sorted({why for line in removed for m, why in REMOVED_SINCE_1_0 if m in line})}
        return 'draw order', {'pixels': px}
    psys_now = collections.Counter(norm(line) for line in cur if PSYS in line)
    if sum(only_ref.values()) == sum(only_now.values()) and not (only_now - psys_now):
        return 'particles', {'pixels': px, 'lines': sum(only_now.values())}
    return 'different', {'pixels': px, 'only_1.0': sum(only_ref.values()), 'only_now': sum(only_now.values()),
                         'sample_now': [line[:400] for line in list(only_now)[:3]]}


def capture(tag, binary, levels):
    env = dict(os.environ, GDL_SCENE_DUMP='500,700,800,1000')
    subprocess.run([sys.executable, str(ROOT / 'tools/python/frozen_frames.py'), tag, '--baseline', '--binary', binary, '--levels', *levels],
                   env=env, check=True, stdout=subprocess.DEVNULL)
    return RUNS / f'frozen-{tag}'


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('tag')
    ap.add_argument('--binary', default=str(ROOT / 'target/release/gdl_play'))
    ap.add_argument('--levels', nargs='+', default=LEVELS)
    ap.add_argument('--retries', type=int, default=2)
    ap.add_argument('--ref-frames', default=str(RUNS / 'frozen-base-ref'))
    ap.add_argument('--ref-dumps', default=str(RUNS / 'frozen-dump-ref'))
    ap.add_argument('--judge-only', action='store_true')
    args = ap.parse_args()
    if not args.tag.replace('-', '').isalnum():
        raise SystemExit('tag must be alphanumeric')
    binary = str(pathlib.Path(args.binary).resolve())
    ref_frames, ref_dumps = pathlib.Path(args.ref_frames), pathlib.Path(args.ref_dumps)
    run = RUNS / f'frozen-{args.tag}' if args.judge_only else capture(args.tag, binary, args.levels)
    if args.judge_only:
        binary = json.loads((run / 'frames.json').read_text())['binary']
    report = {'scope': 'original presentation (GDL_BASELINE) frozen frames and scene dumps against the 1.0 build, real GPU',
              'binary': binary, 'binary_sha256': json.loads((run / 'frames.json').read_text())['binary_sha256'],
              'ref_frames': str(ref_frames), 'ref_dumps': str(ref_dumps), 'levels': {}}
    ok = True
    for level in args.levels:
        runs = [run]
        verdicts = [(f, *judge(run, level, f, ref_frames, ref_dumps)) for f in SHOTS[level]]
        attempt = 0
        while not args.judge_only and any(v == 'particles' for _, v, _ in verdicts) and attempt < args.retries:
            attempt += 1
            retry = capture(f'{args.tag}-r{attempt}', binary, [level])
            runs.append(retry)
            verdicts = [(f, *judge(retry, level, f, ref_frames, ref_dumps)) for f in SHOTS[level]]
        passed = all(v in ('identical', 'draw order', 'fixed') for _, v, _ in verdicts)
        ok &= passed
        report['levels'][level] = {'pass': passed, 'attempts': len(runs), 'runs': [str(r) for r in runs],
                                   'frames': {str(f): {'verdict': v, **d} for f, v, d in verdicts}}
        print(level, 'PASS' if passed else 'FAIL', f'({len(runs)} run{"s" if len(runs) > 1 else ""}):',
              ', '.join(f'{f} {v}' + (f' ({d["pixels"]} px)' if 'pixels' in d else '') for f, v, d in verdicts))
    report['pass'] = ok
    out = ROOT / 'private' / 'reports' / f'baseline-gate-{args.tag}.json'
    out.write_text(json.dumps(report, indent=1) + '\n')
    print('PASS' if ok else 'FAIL', out)
    sys.exit(0 if ok else 1)


if __name__ == '__main__':
    main()
