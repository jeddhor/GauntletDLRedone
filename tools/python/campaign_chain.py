#!/usr/bin/env python3
"""Chained state-search runs (M15 tooling; DEBUG, not a fidelity input).

  campaign_chain.py TAG LEVEL [LEVEL ...] [--budget N] [--cells N]
                    [--start START.json] [--legend-bosses] [--no-assist]
                    [--bot-first FRAMES] [--bot-tries N] [--require]

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
--bot-first FRAMES runs the traversal bot (bot.rs, one deterministic run of
at most FRAMES updates) before the search and keeps its end state when it
ends the level; the search runs only for the levels the bot does not end.
--bot-tries N runs the bot N times side by side, each standing idle a
different number of updates after the stage start (BOT_DELAYS: chosen
variations, not game data; the simulation is deterministic, so a retry needs
different input); the first that wins counts; reports -bot.json,
-bot2.json, ...
--require: a level counts as ended only with its rune stone and legend item
taken (the bot's run otherwise falls back to the search with require=1).
"""
import argparse
import json
import os
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
BOSSES = {'A5', 'B6', 'C5', 'D5', 'E2', 'F2', 'G5', 'H4', 'I5', 'J5', 'K5'}
# Idle updates before the bot starts, per try (arbitrary variations).
BOT_DELAYS = [0, 61, 149, 263, 397, 541]


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('tag')
    ap.add_argument('levels', nargs='+')
    ap.add_argument('--budget', type=int, default=800000)
    ap.add_argument('--cells', type=int, default=500)
    ap.add_argument('--start')
    ap.add_argument('--legend-bosses', action='store_true')
    ap.add_argument('--no-assist', action='store_true')
    ap.add_argument('--bot-first', type=int, help='try the traversal bot for this many updates first')
    ap.add_argument('--bot-tries', type=int, default=1, help='bot runs per level before the search (different start delays)')
    ap.add_argument('--require', action='store_true', help='rune stones and legend items must be taken')
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
        common = []
        if start:
            common += ['--start', start, '--next-level']
        if a.legend_bosses and code in BOSSES:
            common += ['--legend']
        if a.no_assist:
            common += ['--no-assist']
        step = {'level': code, 'start': start}
        won_by_bot = False
        tries = []
        for t in range(min(a.bot_tries, len(BOT_DELAYS)) if a.bot_first else 0):
            sfx = '' if t == 0 else str(t + 1)
            bot_report = reports / f'campaign-{a.tag}-{code}-bot{sfx}.json'
            bot_end = reports / f'campaign-{a.tag}-{code}-bot{sfx}-end.json'
            cmd = [sys.executable, 'tools/python/game_run.py', str(bot_report), '--frames', str(a.bot_first), '--world', world, '--level', level,
                   '--pads', '0:1', '--bot', '--end-state', str(bot_end)] + common
            if BOT_DELAYS[t]:
                cmd += ['--bot-delay', str(BOT_DELAYS[t])]
            # The tries run side by side; the first (lowest delay) that wins counts.
            err_path = bot_report.with_suffix('.err')
            with open(err_path, 'x') as err_file:
                tries.append((t, bot_report, bot_end, err_path, subprocess.Popen(cmd, cwd=ROOT, env=os.environ, stdout=subprocess.DEVNULL, stderr=err_file)))
        for t, bot_report, bot_end, err_path, proc in tries:
            proc.wait()
            err = err_path.read_text(errors='replace')
            key = 'bot' if t == 0 else f'bot{t + 1}'
            if won_by_bot:
                continue
            if bot_report.exists():
                br = json.loads(bot_report.read_text())
                b = br.get('bot') or {}
                missed = [x for x in br.get('treasures', []) if not x['taken']]
                step[key] = {'exit_update': b.get('exit_update'), 'exit_wave': b.get('exit_wave'), 'exit_state': b.get('exit_state'),
                             'assists': b.get('health_assists'), 'treasures': br.get('treasures'), 'delay': BOT_DELAYS[t]}
                # Through the level, not ended by a death (the player dying,
                # state 8, or waiting to continue, 0xb).
                won = b.get('exit_update') is not None and b.get('exit_state') not in (8, 11, None)
                if won and bot_end.exists() and not (a.require and missed):
                    step.update(ended=True, by=key, replay_level_end=b['exit_update'], assists=b.get('health_assists'),
                                legend_debug=a.legend_bosses and code in BOSSES)
                    steps.append(step)
                    print(json.dumps(step), flush=True)
                    start = str(bot_end.relative_to(ROOT))
                    won_by_bot = True
            else:
                step[key] = {'error': (err.strip().splitlines() or [''])[-1][:300]}
        if won_by_bot:
            continue
        cmd = [sys.executable, 'tools/python/game_run.py', str(report), '--frames', '10', '--world', world, '--level', level,
               '--pads', '0:1', '--explore', str(a.budget), '--cells', str(a.cells), '--nav', '2', '--end-state', str(end)] + common
        if a.require:
            cmd += ['--require']
        p = subprocess.run(cmd, cwd=ROOT, env=os.environ, capture_output=True, text=True)
        step['returncode'] = p.returncode
        if report.exists():
            r = json.loads(report.read_text())
            x = r.get('explore', {})
            step.update(ended=x.get('ended'), by='search', replay_level_end=r.get('level_end'), script_updates=x.get('script_updates'), treasures=r.get('treasures'),
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
