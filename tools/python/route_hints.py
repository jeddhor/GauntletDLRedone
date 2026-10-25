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
    out.write_text(json.dumps(hints))
    print(out, len(hints), [h['item'] for h in hints])


if __name__ == '__main__':
    main()
