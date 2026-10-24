#!/usr/bin/env python3
"""Exit check of every level (M14 evidence; DEBUG placement).

  exit_check.py TAG [LEVEL ...]

For each level (default: every level of the play configuration) a
one-update game-run lists the level's exits; then player 0 — the
record-based player with an idle pad script — starts on the first exit
(game-run's DEBUG teleport, place_player) and the level runs for 300
updates. The level counts as ended when a sample shows level_done or the
exit flags (players done). Levels without exit portals (boss levels end
through the boss's auto exit, the timed secret levels through their timer)
are listed as such. This checks the exit machinery, not that the exit can
be reached on foot. Report: private/reports/exit-check-TAG.json (new file).
GDL_TOOLS selects the gdl_tools binary (game_run.py).
"""
import json
import pathlib
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]


def levels():
    cfg = json.loads((ROOT / 'private/reports/play-config.json').read_text())
    return [l['name'] for l in cfg['levels']]


def game_run(out, level, *extra):
    cmd = [sys.executable, str(ROOT / 'tools/python/game_run.py'), str(out), '--world', level[0], '--level', level[1:], *extra]
    subprocess.run(cmd, cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=900, check=False)
    return json.loads(out.read_text()) if out.exists() else None


def check(level, tmp):
    first = game_run(tmp / f'{level}-load.json', level, '--frames', '1')
    if first is None:
        return dict(result='failed', stage='load')
    exits = first.get('exits') or []
    if not exits:
        return dict(result='no exits')
    at = exits[0][3]
    r = game_run(tmp / f'{level}-exit.json', level, '--frames', '300', '--teleport', ','.join(str(v) for v in at), '--pads', '0:300')
    if r is None:
        return dict(result='failed', stage='run', exit=exits[0])
    ended = [s for s in r['samples'] if s.get('level_done') is not None or (s.get('exit') or [False])[0]]
    unsupported = sorted({u['path'] for u in r.get('unsupported', []) if 'enemy_load' not in u['path']})
    return dict(result='ended' if ended else 'not ended', exit=exits[0], update=ended[0]['frame'] if ended else None,
                level_done=ended[0].get('level_done') if ended else None, unsupported=unsupported)


def main():
    if len(sys.argv) < 2:
        raise SystemExit(__doc__)
    report = ROOT / 'private' / 'reports' / f'exit-check-{sys.argv[1]}.json'
    if report.exists():
        raise SystemExit(f'{report} exists')
    out = {}
    with tempfile.TemporaryDirectory() as tmp:
        for level in sys.argv[2:] or levels():
            out[level] = check(level, pathlib.Path(tmp))
            print(level, out[level]['result'], out[level].get('level_done'), flush=True)
    report.write_text(json.dumps(dict(scope='DEBUG teleport onto each level\'s first exit, idle record-based player, 300 updates; synthetic on the actual disc', levels=out), indent=1) + '\n')
    print('report', report)


if __name__ == '__main__':
    main()
