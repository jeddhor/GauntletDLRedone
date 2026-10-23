#!/usr/bin/env python3
"""Client smoke run of every level (M14/M17).

  level_smoke.py TAG [LEVEL ...]

For each level (default: every level of the play configuration) gdl_play
starts on the private Xvfb through client_driver.py with GDL_TEST_AUTOSTART
and GDL_DEBUG_LEVEL=<level> (a new TEST character, no input), runs for a
fixed wall time and takes frames. The UI trace (GDL_UI_TRACE) records the
screen, world/level, simulation errors and unported paths; the client log
is scanned for panics. Report: private/reports/level-smoke-TAG.json (new
file). Synthetic runs on the actual disc: no reference comparison.
"""
import json
import os
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]


def levels():
    cfg = json.loads((ROOT / 'private/reports/play-config.json').read_text())
    return [l['name'] for l in cfg['levels']]


def run(tag, level):
    run_tag = f'{tag}-{level}'
    d = ROOT / 'private' / 'runs' / f'client-{run_tag}'
    if d.exists():
        raise SystemExit(f'{d} exists')
    d.mkdir(parents=True)
    steps = d / 'steps.json'
    steps.write_text(json.dumps([["wait", 6], ["shot", "start"], ["wait", 10], ["shot", "later"], ["quit"]]))
    trace = d / 'ui.txt'
    env = dict(os.environ, GDL_UI_TRACE=str(trace), GDL_TEST_AUTOSTART='1', GDL_DEBUG_LEVEL=level)
    try:
        subprocess.run([sys.executable, str(ROOT / 'tools/python/client_driver.py'), 'serve', run_tag, str(steps)], cwd=ROOT, env=env, timeout=600)
        timed_out = False
    except subprocess.TimeoutExpired:
        timed_out = True
    lines = trace.read_text().splitlines() if trace.exists() else []
    log = d / 'client.log'
    text = log.read_text(errors='replace') if log.exists() else ''
    panics = [l for l in text.splitlines() if 'panicked' in l][:5]
    errors = sorted({m.group(1) for l in lines for m in [re.search(r'\| ERROR (.*)', l)] if m})
    unsupported = sorted({m.group(1) for l in lines for m in [re.search(r'\| UNSUPPORTED (.*)', l)] if m})
    worlds = sorted({m.group(0) for l in lines for m in [re.search(r'world \d+ level \d+', l)] if m})
    shots = sorted(p.name for p in d.glob('*.png'))
    ok = not panics and not errors and not timed_out and len(shots) == 2 and bool(worlds)
    return dict(ok=ok, worlds=worlds, errors=errors, unsupported=unsupported, panics=panics, shots=shots, timed_out=timed_out, trace=lines[-6:])


def main():
    if len(sys.argv) < 2:
        raise SystemExit(__doc__)
    tag = sys.argv[1]
    report = ROOT / 'private' / 'reports' / f'level-smoke-{tag}.json'
    if report.exists():
        raise SystemExit(f'{report} exists')
    names = sys.argv[2:] or levels()
    out = {}
    for n in names:
        out[n] = run(tag, n)
        r = out[n]
        print(n, 'ok' if r['ok'] else 'FAIL', r['worlds'], r['errors'][:1], r['unsupported'][:1], r['panics'][:1], flush=True)
    report.write_text(json.dumps(dict(scope='client smoke: autostart at each level, no input, ~16 s wall time; synthetic on the actual disc', levels=out), indent=1) + '\n')
    print('report', report)


if __name__ == '__main__':
    main()
