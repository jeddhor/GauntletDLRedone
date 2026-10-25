#!/usr/bin/env python3
"""Route hints from a winning traversal-bot run (DEBUG tooling, M14/M15).

  route_hints.py REPORT.json OUT.json

The bot's log lists every goal it reached ({"reached": slot}). A run that
ended the level shows which of them mattered: the triggers, in the order
they were first reached, plus the keys taken before a gate and the level's
rune stone / legend item. The hints (bot.rs Hint: {"item": slot}) let a
later run (game_run.py with GDL_BOT_HINTS) walk that route instead of
exploring; every step is still the bot's own pad input through the game.
Only runs that ended without the player dying are accepted.
"""
import json
import pathlib
import sys


def main():
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    report = json.loads(pathlib.Path(sys.argv[1]).read_text())
    out = pathlib.Path(sys.argv[2])
    if out.exists():
        raise SystemExit(f'{out} exists')
    bot = report.get('bot') or {}
    if bot.get('exit_update') is None or bot.get('exit_state') in (8, 11):
        raise SystemExit('the run did not end the level alive')
    kinds = {i[0]: (i[1][0], i[1][1]) for i in bot.get('all_items', [])}
    seen = set()
    hints = []
    for e in bot.get('log', []):
        s = e.get('reached')
        if s is None or s in seen:
            continue
        k = kinds.get(s)
        # Triggers (kind 5), the rune stone (1, 10) and the legend item (1, 13).
        if k is None or not (k[0] == 5 or k in ((1, 10), (1, 13))):
            continue
        seen.add(s)
        hints.append({'item': s, 'goto': None, 'radius': None, 'wait': None})
    # The way from the last of them to the exit, as the run walked it: its
    # sampled positions every 8 units (goto hints, 2-unit radius). The bot's
    # graph may hold a route the player cannot take there (G2's exit stair).
    last = max((e['update'] for e in bot.get('log', []) if e.get('reached') in seen), default=0)
    end = bot['exit_update']
    trail = []
    # The bot's own route (every 2 units) when the report has it, else the
    # report's position samples.
    points = [(r[0], r[1:]) for r in bot['route']] if bot.get('route') else [(x['frame'], x['pos']) for x in report.get('samples', [])]
    for frame, q in points:
        if last <= frame <= end:
            if not trail or max(abs(q[0] - trail[-1][0]), abs(q[2] - trail[-1][2])) >= 8.0 or abs(q[1] - trail[-1][1]) >= 2.0:
                trail.append(q)
    hints += [{'item': None, 'goto': q, 'radius': 2.0, 'wait': None} for q in trail[1:]]
    out.write_text(json.dumps(hints))
    print(out, len(hints), [h['item'] for h in hints if h['item'] is not None], len(trail), 'trail points')


if __name__ == '__main__':
    main()
