#!/usr/bin/env python3
"""A treasure room played through from its trap door, in the real client on
the real GPU (playtest 2026-11-01: G2's room unlocked its character and ended
on the first coin, MaxLevelCoins and SecretWorld being the boot state's).
Actual disc check; needs a build with the DEBUG action next_coin.

  secret_level_check.py TAG [--binary BIN]

G2's secret exit (item 478, game_run.py --itemdump) leads to S8 (cur_level 7;
secret_char 14, unlock bit 0x40; LEVL +0xc 70 seconds). Two runs, each with its
own HOME and saves under private/runs/secret-level-TAG:

  some  3 coins, then the timer runs out: no unlock, back in G2;
  all   every coin on show: the unlock, WaveComplete and back in G2.

The expected values were read from the reference's level captures
(lvl-S8-start-v1: MaxLevelCoins 25, SecretWorld 1, WaveTimer 70.99; lvl-G2:
SecretWorld 0). Report: result.json; exit status 1 on any failure.
"""
import argparse
import json
import os
import pathlib
import subprocess
import sys
import time

ROOT = pathlib.Path(__file__).resolve().parents[2]
RADV = '/usr/share/vulkan/icd.d/radeon_icd.json'
G2_SECRET_EXIT = [-171.3, 24.69, -194.89]
S8_LEVEL, S8_SECONDS, S8_COINS, S8_UNLOCK = 7, 70, 25, 0x40
SCENARIOS = {'some': (3, 3300), 'all': (S8_COINS, 1500)}
SHOTS = list(range(240, 6001, 240))
# Triangle every 30 menu ticks closes any scroll (the AllCoins one holds the game).
SCROLLS = [[30 + 30 * k, 32 + 30 * k, [0xc]] for k in range(40)]


def actions(coins, exit_after):
    a = [{'at': u, 'health': 5000} for u in range(40, exit_after, 40)]
    a.append({'at': 60, 'teleport': G2_SECRET_EXIT})
    a += [{'at': u, 'next_coin': coins} for u in range(70, 1400, 10)]
    a += [{'at': u, 'dump': f'u{u:05d}'} for u in range(30, exit_after, 30)]
    return a


def judge(name, coins, dumps):
    """(failures, summary) of one run's timeline."""
    fail = []
    room = [d for d in dumps if d['world'] == 12]
    if not room:
        return ['never reached the treasure room'], {}
    before = [d for d in dumps if d['update'] < room[0]['update']]
    after = [d for d in dumps if room[-1]['update'] < d['update']]
    s = [d['secret'] for d in room]
    timers = [x['wave_timer'] for x in s]
    summary = dict(levels=sorted({d['level'] for d in room}), updates_in_room=room[-1]['update'] - room[0]['update'], coins=max(x['coins'] for x in s),
                   max_level_coins=sorted({x['max_level_coins'] for x in s}), secret_world=sorted({x['secret_world'] for x in s}),
                   first_timer=timers[0], last_timer=timers[-1], unlocks=s[-1]['unlocks'], wave_complete=s[-1]['wave_complete'],
                   after=[(d['world'], d['level'], d['secret']['coins'], d['secret']['secret_world'], d['secret']['unlocks']) for d in after[-1:]])
    if summary['levels'] != [S8_LEVEL]:
        fail.append(f"room levels {summary['levels']}")
    if summary['max_level_coins'] != [S8_COINS]:
        fail.append(f"MaxLevelCoins {summary['max_level_coins']}")
    if summary['secret_world'] != [1]:
        fail.append(f"SecretWorld in the room {summary['secret_world']}")
    if any(d['secret']['secret_world'] != 0 for d in before + after):
        fail.append('SecretWorld outside the room')
    if timers != sorted(timers, reverse=True):
        fail.append('timer not counting down')
    if S8_SECONDS + 1 < timers[0]:
        fail.append(f'timer started at {timers[0]}')
    if not after or after[-1]['world'] != 7 or after[-1]['level'] != 1:
        fail.append('not back in G2')
    elif after[-1]['secret']['coins'] != 0:
        fail.append('coin count kept after the room')
    if name == 'some':
        if summary['coins'] != coins or summary['unlocks'] & S8_UNLOCK or summary['wave_complete']:
            fail.append(f"partial run: {summary['coins']} coins, unlocks {summary['unlocks']:#x}, complete {summary['wave_complete']}")
        if summary['updates_in_room'] < (S8_SECONDS - 5) * 30:
            fail.append(f"left the room after {summary['updates_in_room']} updates")
    else:
        if summary['coins'] != S8_COINS or not summary['unlocks'] & S8_UNLOCK or summary['wave_complete'] != 1:
            fail.append(f"full run: {summary['coins']} coins, unlocks {summary['unlocks']:#x}, complete {summary['wave_complete']}")
        if not after or not after[-1]['secret']['unlocks'] & S8_UNLOCK:
            fail.append('unlock lost after the room')
    return fail, summary


def scenario(args, run, name):
    coins, exit_after = SCENARIOS[name]
    d = run / name
    (d / 'saves').mkdir(parents=True)
    (d / 'home').mkdir()
    cfg = json.loads((ROOT / 'private/reports/play-config.json').read_text())
    cfg['saves'] = str(d / 'saves')
    (d / 'config.json').write_text(json.dumps(cfg))
    (d / 'actions.json').write_text(json.dumps(actions(coins, exit_after)))
    (d / 'menu.json').write_text(json.dumps(SCROLLS))
    env = dict(os.environ, VK_DRIVER_FILES=RADV, VK_ICD_FILENAMES=RADV, HOME=str(d / 'home'), GDL_NO_AUDIO='1',
               GDL_NO_ATTRACT='1', GDL_NO_GAMEPADS='1', GDL_TEST_AUTOSTART='1', GDL_DEBUG_LEVEL='G2',
               GDL_TEST_ACTIONS=str(d / 'actions.json'), GDL_MENU_SCRIPT=str(d / 'menu.json'),
               GDL_EXIT_AFTER=str(exit_after), GDL_STATE_OUT=str(d / 'state.txt'),
               GDL_SHOT_FRAMES=','.join(map(str, SHOTS)), GDL_SHOT_DIR=str(d), GDL_RENDER_FPS='60', RUST_BACKTRACE='1')
    for k in ('WAYLAND_DISPLAY', 'XDG_DATA_HOME', 'GDL_SETTINGS', 'GDL_BASELINE'):
        env.pop(k, None)
    with open(d / 'client.log', 'w') as log:
        client = subprocess.Popen([args.binary, str(d / 'config.json')], cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
        deadline = time.time() + 300
        while time.time() < deadline and client.poll() is None:
            time.sleep(0.5)
        timed_out = client.poll() is None
        if timed_out:
            client.terminate()
            try:
                client.wait(10)
            except subprocess.TimeoutExpired:
                client.kill()
    panics = [ln for ln in (d / 'client.log').read_text(errors='replace').splitlines() if 'panicked at' in ln]
    dumps = sorted((json.loads(f.read_text()) for f in d.glob('state.u*.json')), key=lambda j: j['update'])
    fail, summary = judge(name, coins, dumps)
    if timed_out or client.returncode != 0 or panics:
        fail.append(f'client: status {client.returncode}, timed out {timed_out}, panics {panics}')
    timeline = [(j['update'], j['world'], j['level'], j['secret']['coins'], round(j['secret']['wave_timer'], 1), j['secret']['max_level_coins'],
                 j['secret']['secret_world'], j['secret']['unlocks']) for j in dumps]
    return dict(passed=not fail, failures=fail, summary=summary, timeline_update_world_level_coins_timer_max_secret_unlocks=timeline)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('tag')
    ap.add_argument('--binary', default=str(ROOT / 'target/release/gdl_play'))
    ap.add_argument('--inner', action='store_true')
    args = ap.parse_args()
    args.binary = str(pathlib.Path(args.binary).resolve())
    run = ROOT / 'private/runs' / f'secret-level-{args.tag}'
    if args.inner:
        result = dict(binary=args.binary, runs={name: scenario(args, run, name) for name in SCENARIOS})
        result['passed'] = all(r['passed'] for r in result['runs'].values())
        (run / 'result.json').write_text(json.dumps(result, indent=1))
        for name, r in result['runs'].items():
            print(name, 'PASS' if r['passed'] else 'FAIL', r['failures'], json.dumps(r['summary']))
        sys.exit(0 if result['passed'] else 1)
    if run.exists():
        raise SystemExit(f'{run} exists')
    run.mkdir(parents=True)
    status = subprocess.run([sys.executable, str(ROOT / 'tools/python/kwin_session.py'), str(run / 'session.log'), '--',
                             sys.executable, str(pathlib.Path(__file__).resolve()), args.tag, '--inner', '--binary', args.binary], cwd=ROOT).returncode
    print((run / 'session.log').read_text())
    sys.exit(status)


if __name__ == '__main__':
    main()
