#!/usr/bin/env python3
"""Traversal report (M14) from traversal-bot runs and state searches.

  traversal_report.py NEW_REPORT.json RUN.json [RUN.json ...]

Each RUN is a game_run.py --bot report (gdl_tools bot.rs) or a game_run.py
--explore report (explore.rs: its pad script replayed from the load; the
level counts when the replay ends it); the level code is taken from the
file name's last "-XY" part. A level ended by a death (the bot's exit
state 8 or 0xb) does not count. The level's rune stone and legend item are
reported with whether they were taken (game-run treasures). Per level: whether the level
ended (exit portal, treasure-room timer, boss auto exit) and at which update
and destination wave, the closest 3-D approach to an exit item relative to
the start distance, goals reached, stuck events, health assists (DEBUG) and
whether the legend item was held (boss levels; labelled precondition).
When several runs cover a level, the one that ended (else the closest
approach) is kept. Research report: a level the bot does not finish is not
evidence that it cannot be traversed.
"""
import json
import math
import pathlib
import sys


def summary(path):
    r = json.loads(pathlib.Path(path).read_text())
    if 'explore' in r:
        x = r['explore']
        ended = bool(x.get('ended')) and r.get('level_end') is not None
        return dict(source=str(path), method='search', ended=ended, end_update=r.get('level_end'), progress=[x.get('start_progress'), x.get('max_progress')],
                    health_assists=r.get('assists'), assist=x.get('health_assist', True), treasures=r.get('treasures'), frames=x.get('script_updates'))
    b = r['bot']
    exits = [e[3] for e in r.get('exits', [])]
    d0 = min((math.dist(b['start'], e) for e in exits), default=None)
    dmin = min((min(math.dist(s['pos'], e) for e in exits) for s in r['samples'] if 'pos' in s), default=None) if exits else None
    died = b.get('exit_state') in (8, 11)
    return dict(source=str(path), method='bot', ended=b['exit_update'] is not None and not died, died=died, end_update=b['exit_update'], end_wave=b.get('exit_wave'),
                treasures=r.get('treasures'),
                closest_exit_ratio=None if b['exit_update'] is not None or not d0 else round(dmin / d0, 3),
                goals_reached=sum('reached' in e for e in b['log']), stuck_events=sum('stuck' in e for e in b['log']),
                health_assists=b['health_assists'], legend_item_held=any(s.get('legend') for s in r['samples'][:1]),
                start_level=b.get('start_level'), route_hints=b.get('hints', 0), frames=r.get('frames'))


def main():
    if len(sys.argv) < 3:
        raise SystemExit(__doc__)
    target = pathlib.Path(sys.argv[1])
    if target.exists():
        raise SystemExit(f'{target} exists')
    levels = {}
    for p in sys.argv[2:]:
        code = pathlib.Path(p).stem.rsplit('-', 1)[-1].upper()
        if not (len(code) == 2 and code[0] in 'ABCDEFGHIJKLS' and code[1].isdigit()):
            continue
        try:
            s = summary(p)
        except (OSError, ValueError, KeyError, TypeError) as e:
            s = dict(source=p, error=str(e)[:200], ended=False)
        old = levels.get(code)
        better = old is None or (s['ended'] and not old['ended']) or (
            not s['ended'] and not old['ended'] and (s.get('closest_exit_ratio') or 9) < (old.get('closest_exit_ratio') or 9))
        if better:
            levels[code] = s
    out = dict(scope='traversal bot (game_run.py --bot) and state search (game_run.py --explore, replayed): port 0 through the ordinary pad '
                     'path; health assists, the boss levels\' legend item, a raised start level (pexp, ModifyExp) and route hints '
                     '(tools/bot-routes) are labelled DEBUG/preconditions; unfinished levels are not evidence either way',
               levels_ended=sorted(k for k, v in levels.items() if v['ended']), levels=levels)
    with target.open('x') as f:
        json.dump(out, f, indent=1)
    print(len(out['levels_ended']), 'of', len(levels), 'levels ended:', ' '.join(out['levels_ended']))


if __name__ == '__main__':
    main()
