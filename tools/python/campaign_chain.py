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
--difficulty N plays every level at the Game Options difficulty N (0 Easy,
1 Medium — the fixture's, 2 Hard), an ordinary game setting.
--crystals: bot runs collect the crystals of the sets not yet full
(game_run.py --seek-crystals).
--gates: before each level the Tower's tests (from a two-update probe of the level from the carried
state: the report's realms_open — WorldOpen — and level_open — HideExits'
portal rule) must pass; a closed portal in an open realm stops the chain; a realm opened by a crystal set (WorldCrystal) that
is not full yet is filled by replays — the --replay-pool level with the
most crystals of that set shown at load, played again (bot, --crystals,
no --require: its rune or legend item is already held) — at most
--max-replays per gate. A closed Temple/Hell/Battle (shards, runes) stops
the chain.
"""
import argparse
import json
import os
import pathlib
import signal
import subprocess
import sys
import time

ROOT = pathlib.Path(__file__).resolve().parents[2]
BOSSES = {'A5', 'B6', 'C5', 'D5', 'E2', 'F2', 'G5', 'H4', 'I5', 'J5', 'K5'}
# Idle updates before the bot starts, per try (arbitrary variations).
BOT_DELAYS = [0, 61, 149, 263, 397, 541]
# game_run.py's world letters (ELF worlddatas 0x23deb0).
WORLDS = {'A': 1, 'B': 2, 'C': 3, 'D': 4, 'E': 5, 'F': 6, 'G': 7, 'H': 8, 'I': 9, 'J': 10, 'K': 11, 'S': 12, 'L': 0xd}


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
    ap.add_argument('--loot', action='store_true', help='bot runs pick up nearby gold, food and potions (game_run.py --loot)')
    ap.add_argument('--shop-buy', help='chained starts: the Tower shop purchases (game_run.py --shop-buy, e.g. heal,potion)')
    ap.add_argument('--require', action='store_true', help='rune stones and legend items must be taken')
    ap.add_argument('--new-character', help='without --start: the first level entered by a new character of class CLS[:COLOUR] (game_run.py --new-character, e.g. WAR:YEL)')
    ap.add_argument('--difficulty', type=int, choices=(0, 1, 2), help='Game Options difficulty for every level: 0 Easy, 1 Medium, 2 Hard (a game setting, game_run.py --difficulty)')
    ap.add_argument('--crystals', action='store_true', help='bot runs collect crystals of unfilled sets (game_run.py --seek-crystals)')
    ap.add_argument('--gates', action='store_true', help="each level must pass the Tower's door test (WorldOpen); crystal gates are filled by replays from --replay-pool")
    ap.add_argument('--replay-pool', nargs='*', default=[], help='--gates: levels that may be replayed for crystals')
    ap.add_argument('--max-replays', type=int, default=8, help='--gates: replays per closed gate before the chain stops')
    ap.add_argument('--played', nargs='*', default=[], help='--gates: pool levels already played before --start (only played levels are replayed)')
    a = ap.parse_args()
    if not a.tag.replace('-', '').isalnum():
        raise SystemExit('tag must be alphanumeric')
    reports = ROOT / 'private' / 'reports'
    summary_path = reports / f'campaign-{a.tag}.json'
    if summary_path.exists():
        raise SystemExit(f'{summary_path} exists')
    start = a.start
    steps = []
    seen = {}
    queue = list(a.levels)
    replays = set()
    probes = {}
    gate_replays = {}

    def probe(code, from_start):
        # Two updates of the level from the carried state: the report's
        # realms_open (WorldOpen per world at load), WorldCrystal,
        # MAX_CRYSTALS and the level's crystals per set.
        n = len(probes) + 1
        out = reports / f'campaign-{a.tag}-probe{n}-{code}.json'
        cmd = [sys.executable, 'tools/python/game_run.py', str(out), '--frames', '2', '--world', code[0], '--level', code[1:], '--pads', '0:1']
        if from_start:
            cmd += ['--start', from_start, '--next-level']
        if a.difficulty is not None:
            cmd += ['--difficulty', str(a.difficulty)]
        subprocess.run(cmd, cwd=ROOT, env=os.environ, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
        r = json.loads(out.read_text())
        probes[(code, from_start)] = r
        return r

    while queue:
        code = queue.pop(0)
        world, level = code[0], code[1:]
        # game_main's level end returns a normal exit to the Tower (the
        # shop), whatever the exit's destination; only E1/F1 (to E2/F2, which
        # have no portal) and world 12 go on by the map, which the probe's
        # level_open covers (Temple and Hell: the realm only; secret levels:
        # none).
        if a.gates and start:
            r = probe(code, start)
            if r.get('level_open') is False and WORLDS[world] in r['realms_open']:
                # The realm is open but the portal is not (level n needs
                # bit n−1 of the world's wave byte; Battle 4 all runes).
                gate = {'level': code, 'gate': 'level closed', 'start': start}
                steps.append(gate)
                print(json.dumps(gate), flush=True)
                break
            if WORLDS[world] not in r['realms_open']:
                need = r['world_crystal'][WORLDS[world]]
                gate = {'level': code, 'gate': 'closed', 'set': need, 'start': start}
                if need <= 0 or gate_replays.get(code, 0) >= a.max_replays or not a.replay_pool:
                    steps.append(gate)
                    print(json.dumps(gate), flush=True)
                    break
                # The pool level with the most crystals of the set (shown
                # for this player count) at load.
                def shown(c):
                    key = (c, None)
                    q = probes.get(key) or probe(c, None)
                    return q['crystals_at_load'].get(str(need), [0, 0])[0]
                # Only levels already played (open from the Tower).
                pool = [c for c in a.replay_pool if c in seen or c in a.played]
                if not pool:
                    gate['replay'] = None
                    steps.append(gate)
                    print(json.dumps(gate), flush=True)
                    break
                best = max(pool, key=shown)
                gate_replays[code] = gate_replays.get(code, 0) + 1
                gate.update(replay=best, shown=shown(best), replays=gate_replays[code])
                steps.append(gate)
                print(json.dumps(gate), flush=True)
                replays.add((best, seen.get(best, 0) + 1))
                queue[:0] = [best, code]
                continue
        is_replay = (code, seen.get(code, 0) + 1) in replays
        # A level played again (a replay from the Tower, e.g. G1 G1 G2): its
        # reports take the play count (campaign-TAG-G1-2.json).
        seen[code] = seen.get(code, 0) + 1
        name = code if seen[code] == 1 else f'{code}-{seen[code]}'
        report = reports / f'campaign-{a.tag}-{name}.json'
        end = reports / f'campaign-{a.tag}-{name}-end.json'
        if report.exists() or end.exists():
            raise SystemExit(f'{report} or its end state exists')
        common = []
        if start:
            # The Tower between levels: its shop opened from the menu
            # restores health (PlayersRestoreHealth); not simulated.
            common += ['--start', start, '--next-level', '--tower-shop']
            if a.shop_buy:
                common += ['--shop-buy', a.shop_buy]
        elif a.new_character:
            # A new game with another class: player 0 made as the select
            # screen makes a new character, entering the first level.
            common += ['--next-level', '--new-character', a.new_character]
        if a.legend_bosses and code in BOSSES:
            common += ['--legend']
        if a.no_assist:
            common += ['--no-assist']
        if a.difficulty is not None:
            common += ['--difficulty', str(a.difficulty)]
        if a.loot:
            common += ['--loot']
        if a.crystals:
            common += ['--seek-crystals']
        step = {'level': code, 'play': seen[code], 'start': start, 'replay': is_replay}
        won_by_bot = False
        tries = []
        for t in range(min(a.bot_tries, len(BOT_DELAYS)) if a.bot_first else 0):
            sfx = '' if t == 0 else str(t + 1)
            bot_report = reports / f'campaign-{a.tag}-{name}-bot{sfx}.json'
            bot_end = reports / f'campaign-{a.tag}-{name}-bot{sfx}-end.json'
            cmd = [sys.executable, 'tools/python/game_run.py', str(bot_report), '--frames', str(a.bot_first), '--world', world, '--level', level,
                   '--pads', '0:1', '--bot', '--end-state', str(bot_end)] + common
            if BOT_DELAYS[t]:
                cmd += ['--bot-delay', str(BOT_DELAYS[t])]
            if a.require and not is_replay:
                # The bot does not take an exit while a rune or legend item is left.
                cmd += ['--require']
            # The tries run side by side; the first to finish with a win counts.
            err_path = bot_report.with_suffix('.err')
            with open(err_path, 'x') as err_file:
                tries.append((t, bot_report, bot_end, err_path, subprocess.Popen(cmd, cwd=ROOT, env=os.environ, stdout=subprocess.DEVNULL, stderr=err_file,
                                                                                 start_new_session=True)))
        # The first try to finish with a win counts (any try's run is
        # reproducible from its recorded delay); the others are stopped.
        pending = list(tries)
        while pending and not won_by_bot:
            done = [x for x in pending if x[4].poll() is not None]
            if not done:
                time.sleep(5)
                continue
            for t, bot_report, bot_end, err_path, proc in sorted(done, key=lambda x: x[0]):
                pending.remove((t, bot_report, bot_end, err_path, proc))
                if won_by_bot:
                    continue
                err = err_path.read_text(errors='replace')
                key = 'bot' if t == 0 else f'bot{t + 1}'
                if bot_report.exists():
                    br = json.loads(bot_report.read_text())
                    b = br.get('bot') or {}
                    missed = [x for x in br.get('treasures', []) if not x['taken']]
                    step[key] = {'exit_update': b.get('exit_update'), 'exit_wave': b.get('exit_wave'), 'exit_state': b.get('exit_state'),
                                 'assists': b.get('health_assists'), 'treasures': br.get('treasures'), 'delay': BOT_DELAYS[t]}
                    # Through the level, not ended by a death (the player dying,
                    # state 8, or waiting to continue, 0xb).
                    won = b.get('exit_update') is not None and b.get('exit_state') not in (8, 11, None)
                    if won and bot_end.exists() and not (a.require and missed and not is_replay):
                        step.update(ended=True, by=key, replay_level_end=b['exit_update'], assists=b.get('health_assists'),
                                    legend_debug=a.legend_bosses and code in BOSSES)
                        steps.append(step)
                        print(json.dumps(step), flush=True)
                        start = str(bot_end.relative_to(ROOT))
                        won_by_bot = True
                else:
                    step[key] = {'error': (err.strip().splitlines() or [''])[-1][:300]}
        for t, bot_report, bot_end, err_path, proc in pending:
            # A try won: the rest are stopped (their reports are partial and
            # not read).
            try:
                os.killpg(proc.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            proc.wait()
        if won_by_bot:
            continue
        if is_replay:
            # A replay is for crystals only: no search (it collects none); the
            # gate is tested again from the unchanged state.
            step['ended'] = False
            steps.append(step)
            print(json.dumps(step), flush=True)
            continue
        cmd = [sys.executable, 'tools/python/game_run.py', str(report), '--frames', '10', '--world', world, '--level', level,
               '--pads', '0:1', '--explore', str(a.budget), '--cells', str(a.cells), '--nav', '2', '--end-state', str(end)] + common
        if a.require and not is_replay:
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
                                        'difficulty': a.difficulty,
                                        'new_character': a.new_character,
                                        'crystals': a.crystals, 'gates': a.gates, 'replay_pool': a.replay_pool,
                                        'levels': a.levels, 'steps': steps}, indent=1) + '\n')
    print(summary_path)


if __name__ == '__main__':
    main()
