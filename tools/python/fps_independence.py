#!/usr/bin/env python3
"""Render-FPS independence check (M10): the same scripted input at several
render rates must give the same simulation, update by update.

  fps_independence.py TAG LEVEL UPDATES SCRIPT.json FPS [FPS ...]   (0 = uncapped)

For each FPS, gdl_play runs on the private Xvfb through client_driver.py
with GDL_TEST_AUTOSTART (a default character starts at once, no menu time)
and GDL_DEBUG_LEVEL=LEVEL,
GDL_INPUT_SCRIPT=SCRIPT (port 0 by stepped-update index; the keyboard is
ignored once the level runs), GDL_RENDER_FPS=FPS and GDL_EXIT_AFTER=UPDATES;
the client writes the SHA-256 of its simulation records after every update
(gdl_play TestHarness) to private/runs/client-TAG-fpsN/state.txt. The
report private/reports/fps-independence-TAG.json lists, per rate, the
hashes' agreement with the first rate and the first differing update.
"""
import json
import os
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]


def main():
    if len(sys.argv) < 6:
        raise SystemExit(__doc__)
    tag, level, updates, script = sys.argv[1], sys.argv[2], int(sys.argv[3]), pathlib.Path(sys.argv[4]).resolve()
    rates = [int(v) for v in sys.argv[5:]]
    report = ROOT / 'private' / 'reports' / f'fps-independence-{tag}.json'
    if report.exists():
        raise SystemExit(f'{report} exists')
    results = {}
    achieved = {}
    for fps in rates:
        run_tag = f'{tag}-fps{fps}'
        d = ROOT / 'private' / 'runs' / f'client-{run_tag}'
        if d.exists():
            raise SystemExit(f'{d} exists')
        d.mkdir(parents=True)
        out = d / 'state.txt'
        steps = d / 'steps.json'
        # The client exits by itself after UPDATES updates; short waits let
        # the driver notice (it checks the client between steps).
        steps.write_text(json.dumps([["wait", 3]] * 400))
        env = dict(os.environ, GDL_TEST_AUTOSTART='1', GDL_DEBUG_LEVEL=level, GDL_INPUT_SCRIPT=str(script), GDL_EXIT_AFTER=str(updates),
                   GDL_STATE_OUT=str(out), GDL_RENDER_FPS=str(fps))
        subprocess.run([sys.executable, str(ROOT / 'tools/python/client_driver.py'), 'serve', run_tag, str(steps)],
                       cwd=ROOT, env=env, timeout=1500)
        hashes = out.read_text().split() if out.exists() else []
        results[fps] = hashes
        frames = out.with_suffix('.frames')
        if frames.exists():
            n, secs = frames.read_text().split()
            achieved[fps] = round(int(n) / float(secs), 2)
        print(fps, 'updates hashed', len(hashes), 'achieved fps', achieved.get(fps), flush=True)
    base = results[rates[0]]
    summary = {}
    for fps in rates:
        h = results[fps]
        first = next((i for i, (a, b) in enumerate(zip(base, h)) if a != b), None)
        summary[str(fps)] = dict(updates=len(h), equal_to_first_rate=(h == base and len(h) == updates), first_difference=first,
                                 achieved_render_fps=achieved.get(fps))
    with report.open('x') as f:
        json.dump(dict(level=level, updates=updates, script=str(script), rates=rates, summary=summary,
                       final_hash={str(k): (v[-1] if v else None) for k, v in results.items()}), f, indent=1)
    print(json.dumps(summary))


if __name__ == '__main__':
    main()
