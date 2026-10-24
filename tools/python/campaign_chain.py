#!/usr/bin/env python3
"""Chained state-search runs (M15 tooling; DEBUG, not a fidelity input).

  campaign_chain.py TAG LEVEL [LEVEL ...] [--budget N] [--cells N]
                    [--start START.json] [--legend-bosses] [--no-assist]

For each level code in order (G1 G2 ...), game_run.py --explore (bot
macros, nav=2) from the previous level's end state: the run's own replay of
its pad script from the level load, written by --end-state as the start
state of the next level (players' records and globals carried; the exit
flow between levels — tally, shop, the Tower — is not simulated; game-run
--next-level runs init_next_level's record steps and places the player at
the next level's start locator). The chain stops at
the first level the search does not end. Reports: private/reports/
campaign-TAG-<L>.json (the game-run report), campaign-TAG-<L>-end.json
(the end state) and campaign-TAG.json (the summary). --legend-bosses gives
boss levels the legend item (DEBUG precondition, as the bot batches) when
the carried record lacks it. --no-assist searches and replays without the
DEBUG health assist (legitimate play: a dying branch is dropped).
"""
import argparse
import json
import os
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
BOSSES = {'A5', 'B6', 'C5', 'D5', 'E2', 'F2', 'G5', 'H4', 'I5', 'J5', 'K5'}


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('tag')
    ap.add_argument('levels', nargs='+')
    ap.add_argument('--budget', type=int, default=800000)
    ap.add_argument('--cells', type=int, default=500)
    ap.add_argument('--start')
    ap.add_argument('--legend-bosses', action='store_true')
    ap.add_argument('--no-assist', action='store_true')
    a = ap.parse_args()
    if not a.tag.replace('-', '').isalnum():
        raise SystemExit('tag must be alphanumeric')
    reports = ROOT / 'private' / 'reports'
    summary_path = reports / f'campaign-{a.tag}.json'
    if summary_path.exists():
        raise SystemExit(f'{summary_path} exists')
    start = a.start
    steps = []
    for code in a.levels:
        world, level = code[0], code[1:]
        report = reports / f'campaign-{a.tag}-{code}.json'
        end = reports / f'campaign-{a.tag}-{code}-end.json'
        if report.exists() or end.exists():
            raise SystemExit(f'{report} or its end state exists')
        cmd = [sys.executable, 'tools/python/game_run.py', str(report), '--frames', '10', '--world', world, '--level', level,
               '--pads', '0:1', '--explore', str(a.budget), '--cells', str(a.cells), '--nav', '2', '--end-state', str(end)]
        if start:
            cmd += ['--start', start, '--next-level']
        if a.legend_bosses and code in BOSSES:
            cmd += ['--legend']
        if a.no_assist:
            cmd += ['--no-assist']
        p = subprocess.run(cmd, cwd=ROOT, env=os.environ, capture_output=True, text=True)
        step = {'level': code, 'start': start, 'returncode': p.returncode}
        if report.exists():
            r = json.loads(report.read_text())
            x = r.get('explore', {})
            step.update(ended=x.get('ended'), replay_level_end=r.get('level_end'), script_updates=x.get('script_updates'),
                        assists=r.get('assists'), progress=[x.get('start_progress'), x.get('max_progress')],
                        legend_debug=a.legend_bosses and code in BOSSES,
                        unsupported=[u['path'] for u in r.get('unsupported', []) if 'enemy_load' not in u['path']][:5])
        else:
            step['error'] = (p.stderr.strip().splitlines() or [''])[-1][:300]
        steps.append(step)
        print(json.dumps(step), flush=True)
        if not (step.get('ended') and step.get('replay_level_end') is not None and end.exists()):
            break
        start = str(end.relative_to(ROOT))
    summary_path.write_text(json.dumps({'scope': 'DEBUG chained state-search runs (campaign_chain.py); the exit flow between levels is not simulated',
                                        'health_assist': not a.no_assist,
                                        'levels': a.levels, 'steps': steps}, indent=1) + '\n')
    print(summary_path)


if __name__ == '__main__':
    main()
